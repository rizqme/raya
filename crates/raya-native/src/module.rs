// #[module] proc-macro implementation
//
// Generates native module initialization code.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemFn, Result};

/// Expands the #[module] attribute macro.
///
/// Input: Init function that returns NativeModule
/// Output: Exported raya_module_init function for dynamic loading
///
/// Example expansion:
/// ```ignore
/// // Input:
/// #[module]
/// fn init() -> NativeModule {
///     let mut module = NativeModule::new("math", "1.0.0");
///     module.register_function("add", add_ffi);
///     module
/// }
///
/// // Output:
/// fn init() -> NativeModule {
///     let mut module = NativeModule::new("math", "1.0.0");
///     module.register_function("add", add_ffi);
///     module
/// }
///
/// #[no_mangle]
/// pub extern "C" fn raya_module_init() -> *mut raya_core::ffi::NativeModule {
///     let module = init();
///     Box::into_raw(Box::new(module))
/// }
/// ```
pub fn expand_module(func: ItemFn) -> Result<TokenStream> {
    // Validate function signature
    if func.sig.ident != "init" {
        return Err(syn::Error::new_spanned(
            &func.sig.ident,
            "#[module] must be applied to a function named 'init'",
        ));
    }

    if !func.sig.inputs.is_empty() {
        return Err(syn::Error::new_spanned(
            &func.sig.inputs,
            "Module init function must not have parameters",
        ));
    }

    // Check return type is NativeModule
    let returns_native_module = match &func.sig.output {
        syn::ReturnType::Type(_, ty) => {
            if let syn::Type::Path(type_path) = &**ty {
                type_path
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident == "NativeModule")
                    .unwrap_or(false)
            } else {
                false
            }
        }
        _ => false,
    };

    if !returns_native_module {
        return Err(syn::Error::new_spanned(
            &func.sig.output,
            "Module init function must return NativeModule",
        ));
    }

    // Generate the expanded code
    let expanded = quote! {
        // Keep original init function
        #func

        /// FFI entry point for dynamic library loading.
        ///
        /// This function is called by the VM when loading the native module.
        /// It creates and returns a boxed NativeModule.
        #[no_mangle]
        pub extern "C" fn raya_module_init() -> *mut raya_core::ffi::NativeModule {
            let module = init();
            Box::into_raw(Box::new(module))
        }

        /// Cleanup function called when module is unloaded.
        ///
        /// This function is called by the VM to free the module's resources.
        #[no_mangle]
        pub extern "C" fn raya_module_cleanup(module_ptr: *mut raya_core::ffi::NativeModule) {
            if !module_ptr.is_null() {
                unsafe {
                    // Take ownership and drop
                    let _ = Box::from_raw(module_ptr);
                }
            }
        }
    };

    Ok(expanded)
}
