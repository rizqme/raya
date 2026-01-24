// #[function] proc-macro implementation
//
// Generates FFI wrapper code for native module functions.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, Result};

use crate::traits::{
    generate_arg_extraction, generate_from_raya_impl, generate_panic_wrapper, generate_pin_code,
    generate_to_raya_impl,
};

/// Expands the #[function] attribute macro.
///
/// Input: Original Rust function
/// Output: Original function + FFI wrapper function
///
/// Example expansion:
/// ```ignore
/// // Input:
/// #[function]
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// // Output:
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[no_mangle]
/// pub extern "C" fn add_ffi(
///     args: *const raya_core::ffi::NativeValue,
///     arg_count: usize,
/// ) -> raya_core::ffi::NativeValue {
///     // Validation
///     // Argument extraction
///     // Panic catching
///     // Call original function
///     // Convert result
/// }
/// ```
pub fn expand_function(func: ItemFn) -> Result<TokenStream> {
    let func_name = &func.sig.ident;
    let ffi_name = format_ident!("{}_ffi", func_name);
    let inputs = &func.sig.inputs;
    let output = &func.sig.output;
    let is_async = func.sig.asyncness.is_some();

    // Extract argument names and types
    let mut arg_names = Vec::new();
    let mut arg_types = Vec::new();

    for (i, arg) in inputs.iter().enumerate() {
        match arg {
            syn::FnArg::Typed(pat_type) => {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    arg_names.push(pat_ident.ident.clone());
                    arg_types.push(pat_type.ty.clone());
                } else {
                    return Err(syn::Error::new_spanned(
                        arg,
                        "Only simple identifiers are supported as arguments",
                    ));
                }
            }
            syn::FnArg::Receiver(_) => {
                return Err(syn::Error::new_spanned(
                    arg,
                    "Methods (self) are not supported in #[function]",
                ));
            }
        }
    }

    let arg_count = arg_names.len();

    // Generate argument extraction code
    let arg_extractions = arg_names.iter().zip(arg_types.iter()).enumerate().map(|(i, (name, ty))| {
        quote! {
            let #name = match unsafe {
                let raw_arg = *args.add(#i);
                <#ty as raya_core::ffi::FromRaya>::from_raya(raw_arg)
            } {
                Ok(val) => val,
                Err(e) => {
                    return raya_core::ffi::NativeValue::error(
                        format!("Argument {} ({}): {}", #i, stringify!(#name), e)
                    );
                }
            };
        }
    });

    // Generate function call (handle async)
    let func_call = if is_async {
        quote! {
            // TODO: Async support requires VM Task spawning integration
            // For now, async functions are not supported
            return raya_core::ffi::NativeValue::error(
                format!("Async functions not yet supported: {}", stringify!(#func_name))
            );
        }
    } else {
        quote! {
            #func_name(#(#arg_names),*)
        }
    };

    // Wrap in panic catcher (only for sync functions)
    let panic_wrapped_call = if is_async {
        func_call
    } else {
        quote! {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                #func_call
            }))
        }
    };

    // Generate return value conversion
    let return_conversion = if is_async {
        quote! {}  // Already returned error above
    } else {
        match output {
            syn::ReturnType::Default => {
                quote! {
                    raya_core::ffi::NativeValue::null()
                }
            }
            syn::ReturnType::Type(_, ty) => {
                quote! {
                    <#ty as raya_core::ffi::ToRaya>::to_raya(result)
                }
            }
        }
    };

    // Generate the FFI wrapper
    let wrapper_body = if is_async {
        quote! {
            // Validate argument count
            if arg_count != #arg_count {
                return raya_core::ffi::NativeValue::error(
                    format!(
                        "Function '{}' expects {} arguments, got {}",
                        stringify!(#func_name),
                        #arg_count,
                        arg_count
                    )
                );
            }

            #panic_wrapped_call
        }
    } else {
        quote! {
            // Validate argument count
            if arg_count != #arg_count {
                return raya_core::ffi::NativeValue::error(
                    format!(
                        "Function '{}' expects {} arguments, got {}",
                        stringify!(#func_name),
                        #arg_count,
                        arg_count
                    )
                );
            }

            // Extract and convert arguments (with type checking)
            #(#arg_extractions)*

            // Call function with panic catching
            let result = match #panic_wrapped_call {
                Ok(value) => value,
                Err(e) => {
                    let panic_msg = if let Some(s) = e.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic".to_string()
                    };
                    return raya_core::ffi::NativeValue::error(
                        format!("Function '{}' panicked: {}", stringify!(#func_name), panic_msg)
                    );
                }
            };

            // Convert result to NativeValue
            #return_conversion
        }
    };

    let expanded = quote! {
        // Keep original function
        #func

        // Generate FFI wrapper
        #[no_mangle]
        pub extern "C" fn #ffi_name(
            args: *const raya_core::ffi::NativeValue,
            arg_count: usize,
        ) -> raya_core::ffi::NativeValue {
            #wrapper_body
        }
    };

    Ok(expanded)
}
