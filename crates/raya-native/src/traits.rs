// Type conversion traits for marshalling between Raya and Rust
//
// These traits enable automatic conversion of function arguments and return values
// with zero-copy semantics where possible.

use proc_macro2::TokenStream;
use quote::quote;

/// Generates FromRaya trait implementation code for proc-macros.
///
/// FromRaya converts RayaValue -> Rust type with type checking and unpinning.
pub fn generate_from_raya_impl(ty: &syn::Type) -> TokenStream {
    quote! {
        <#ty as raya_native::FromRaya>::from_raya(arg)?
    }
}

/// Generates ToRaya trait implementation code for proc-macros.
///
/// ToRaya converts Rust type -> RayaValue with automatic pinning.
pub fn generate_to_raya_impl(expr: &syn::Expr) -> TokenStream {
    quote! {
        raya_native::ToRaya::to_raya(#expr)
    }
}

/// Generates code to extract typed argument from NativeValue array.
///
/// Uses optimized unwrap functions when static type is known:
/// - i32: direct value access (~1-5ns)
/// - String: opaque handle with accessor (~1ns)
/// - Objects: opaque handle (~1ns)
pub fn generate_arg_extraction(
    arg_name: &syn::Ident,
    arg_type: &syn::Type,
    index: usize,
) -> TokenStream {
    quote! {
        let #arg_name = {
            let raw_arg = args[#index];
            <#arg_type as raya_native::FromRaya>::from_raya(raw_arg)?
        };
    }
}

/// Generates panic-catching wrapper around function call.
///
/// Catches panics and converts them to Raya errors:
/// - std::panic::catch_unwind() for safety
/// - AssertUnwindSafe for thread safety
/// - Error message extraction
pub fn generate_panic_wrapper(func_call: TokenStream) -> TokenStream {
    quote! {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #func_call
        }))
        .map_err(|panic| {
            let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic".to_string()
            };
            raya_native::NativeError::Panic(msg)
        })?
    }
}

/// Generates GC pinning code for arguments.
///
/// Pins all heap-allocated values before native call:
/// - Prevents GC from moving/freeing values
/// - Atomic increment of pin_count
/// - Automatic unpinning on scope exit (RAII)
pub fn generate_pin_code(args: &[syn::Ident]) -> TokenStream {
    let pin_calls = args.iter().map(|arg| {
        quote! {
            raya_native::pin_value(#arg);
        }
    });

    let unpin_calls = args.iter().map(|arg| {
        quote! {
            raya_native::unpin_value(#arg);
        }
    });

    quote! {
        // Pin all arguments
        #(#pin_calls)*

        // Ensure unpinning even on panic
        let _guard = scopeguard::guard((), |_| {
            #(#unpin_calls)*
        });
    }
}
