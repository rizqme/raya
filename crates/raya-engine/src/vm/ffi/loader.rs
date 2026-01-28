//! Dynamic library loading for native modules
//!
//! Cross-platform support for loading shared libraries (.so, .dylib, .dll)

use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

use super::NativeModule;

/// Errors that can occur during library loading
#[derive(Debug, Error)]
pub enum LoadError {
    /// Library file not found or could not be loaded
    #[error("Library not found: {path}")]
    NotFound {
        /// Path that was attempted
        path: String,
    },

    /// Symbol not found in library
    #[error("Symbol not found: {symbol} in {library}")]
    SymbolNotFound {
        /// Symbol name that was not found
        symbol: String,
        /// Library path
        library: String,
    },

    /// Module initialization failed
    #[error("Invalid module initialization: {0}")]
    InvalidInit(String),

    /// Platform-specific error
    #[error("Platform error: {0}")]
    PlatformError(String),

    /// Invalid path encoding
    #[error("Invalid UTF-8 in path: {0}")]
    InvalidPath(String),
}

/// Cross-platform dynamic library handle
pub struct Library {
    handle: LibraryHandle,
    path: String,
}

impl Library {
    /// Load a dynamic library from the given path.
    ///
    /// # Platform-specific behavior
    ///
    /// - **Linux**: Loads `.so` files using `dlopen(RTLD_NOW | RTLD_LOCAL)`
    /// - **macOS**: Loads `.dylib` files using `dlopen(RTLD_NOW | RTLD_LOCAL)`
    /// - **Windows**: Loads `.dll` files using `LoadLibraryW`
    ///
    /// # Example
    ///
    /// ```ignore
    /// let lib = Library::open("./libmath.so")?;
    /// ```
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, LoadError> {
        let path_ref = path.as_ref();
        let path_str = path_ref
            .to_str()
            .ok_or_else(|| LoadError::InvalidPath(format!("{:?}", path_ref)))?;

        let handle = LibraryHandle::load(path_str)?;

        Ok(Library {
            handle,
            path: path_str.to_string(),
        })
    }

    /// Get a function pointer by name.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The symbol name is correct
    /// - The function signature matches the type T
    /// - The library remains loaded while using the function
    ///
    /// # Example
    ///
    /// ```ignore
    /// type InitFn = extern "C" fn() -> *mut NativeModule;
    /// let init: InitFn = unsafe { lib.get("raya_module_init")? };
    /// ```
    pub unsafe fn get<T>(&self, symbol: &str) -> Result<T, LoadError> {
        self.handle.symbol(symbol, &self.path)
    }

    /// Load a native module from this library.
    ///
    /// Calls the `raya_module_init()` function to get the NativeModule.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let lib = Library::open("./libmath.so")?;
    /// let module = lib.load_module()?;
    /// println!("Loaded module: {}", module.name());
    /// ```
    pub fn load_module(&self) -> Result<Arc<NativeModule>, LoadError> {
        unsafe {
            // Get module initialization function
            type InitFn = extern "C" fn() -> *mut NativeModule;
            let init: InitFn = self.get("raya_module_init")?;

            // Call init to get module
            let module_ptr = init();

            if module_ptr.is_null() {
                return Err(LoadError::InvalidInit(
                    "raya_module_init returned null".to_string(),
                ));
            }

            // Take ownership of the module
            let module = Box::from_raw(module_ptr);

            Ok(Arc::new(*module))
        }
    }

    /// Get the path this library was loaded from
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // LibraryHandle handles cleanup
    }
}

// Platform-specific implementations

#[cfg(unix)]
type LibraryHandle = UnixLibrary;

#[cfg(windows)]
type LibraryHandle = WindowsLibrary;

// ============================================================================
// Unix Implementation (Linux, macOS, BSD)
// ============================================================================

#[cfg(unix)]
struct UnixLibrary {
    handle: *mut std::ffi::c_void,
}

#[cfg(unix)]
impl UnixLibrary {
    fn load(path: &str) -> Result<Self, LoadError> {
        let c_path = CString::new(path)
            .map_err(|e| LoadError::PlatformError(format!("Invalid path: {}", e)))?;

        let handle = unsafe {
            // RTLD_NOW: Resolve all symbols immediately
            // RTLD_LOCAL: Symbols not available for subsequently loaded libraries
            libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL)
        };

        if handle.is_null() {
            let error = unsafe {
                let err_ptr = libc::dlerror();
                if err_ptr.is_null() {
                    "Unknown error".to_string()
                } else {
                    CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                }
            };

            return Err(LoadError::NotFound {
                path: format!("{}: {}", path, error),
            });
        }

        Ok(UnixLibrary { handle })
    }

    unsafe fn symbol<T>(&self, name: &str, lib_path: &str) -> Result<T, LoadError> {
        let c_name = CString::new(name)
            .map_err(|e| LoadError::PlatformError(format!("Invalid symbol name: {}", e)))?;

        // Clear any previous errors
        libc::dlerror();

        let symbol = libc::dlsym(self.handle, c_name.as_ptr());

        // Check for errors
        let err_ptr = libc::dlerror();
        if !err_ptr.is_null() {
            let error = CStr::from_ptr(err_ptr).to_string_lossy().into_owned();
            return Err(LoadError::SymbolNotFound {
                symbol: name.to_string(),
                library: format!("{}: {}", lib_path, error),
            });
        }

        if symbol.is_null() {
            return Err(LoadError::SymbolNotFound {
                symbol: name.to_string(),
                library: lib_path.to_string(),
            });
        }

        Ok(std::mem::transmute_copy(&symbol))
    }
}

#[cfg(unix)]
impl Drop for UnixLibrary {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}

#[cfg(unix)]
unsafe impl Send for UnixLibrary {}
#[cfg(unix)]
unsafe impl Sync for UnixLibrary {}

// ============================================================================
// Windows Implementation
// ============================================================================

#[cfg(windows)]
struct WindowsLibrary {
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl WindowsLibrary {
    fn load(path: &str) -> Result<Self, LoadError> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        // Convert to wide string
        let wide: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };

        if handle.is_null() {
            let error = unsafe { GetLastError() };
            return Err(LoadError::NotFound {
                path: format!("{} (error code: {})", path, error),
            });
        }

        Ok(WindowsLibrary { handle })
    }

    unsafe fn symbol<T>(&self, name: &str, lib_path: &str) -> Result<T, LoadError> {
        let c_name = CString::new(name)
            .map_err(|e| LoadError::PlatformError(format!("Invalid symbol name: {}", e)))?;

        let symbol = GetProcAddress(self.handle, c_name.as_ptr());

        if symbol.is_null() {
            let error = GetLastError();
            return Err(LoadError::SymbolNotFound {
                symbol: name.to_string(),
                library: format!("{} (error code: {})", lib_path, error),
            });
        }

        Ok(std::mem::transmute_copy(&symbol))
    }
}

#[cfg(windows)]
impl Drop for WindowsLibrary {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.handle);
        }
    }
}

#[cfg(windows)]
unsafe impl Send for WindowsLibrary {}
#[cfg(windows)]
unsafe impl Sync for WindowsLibrary {}

// Windows FFI declarations
#[cfg(windows)]
extern "system" {
    fn LoadLibraryW(filename: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(
        module: *mut std::ffi::c_void,
        procname: *const i8,
    ) -> *mut std::ffi::c_void;
    fn FreeLibrary(module: *mut std::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_not_found() {
        let result = Library::open("/nonexistent/library.so");
        assert!(result.is_err());
        match result {
            Err(LoadError::NotFound { .. }) => {}
            _ => panic!("Expected NotFound error"),
        }
    }
}
