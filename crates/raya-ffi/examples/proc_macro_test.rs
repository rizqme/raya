// Test the #[function] and #[module] proc-macros

use raya_ffi::{FromRaya, NativeError, NativeModule, NativeValue, ToRaya};
use raya_native::{function, module};

#[function]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[function]
fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

#[function]
fn is_even(n: i32) -> bool {
    n % 2 == 0
}

#[function]
fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("Division by zero".to_string())
    } else {
        Ok(a / b)
    }
}

#[module]
fn init() -> NativeModule {
    let mut module = NativeModule::new("math", "1.0.0");

    module.register_function("add", add_ffi);
    module.register_function("multiply", multiply_ffi);
    module.register_function("isEven", is_even_ffi);
    module.register_function("divide", divide_ffi);

    module
}

fn main() {
    println!("Proc-Macro Test");
    println!("===============\n");

    // Test add
    {
        let args = [5_i32.to_raya(), 3_i32.to_raya()];
        let result = unsafe { add_ffi(args.as_ptr(), args.len()) };
        let value = unsafe { result.as_value().as_i32().unwrap() };
        println!("add(5, 3) = {}", value);
        assert_eq!(value, 8);
    }

    // Test multiply
    {
        let args = [4_i32.to_raya(), 7_i32.to_raya()];
        let result = unsafe { multiply_ffi(args.as_ptr(), args.len()) };
        let value = unsafe { result.as_value().as_i32().unwrap() };
        println!("multiply(4, 7) = {}", value);
        assert_eq!(value, 28);
    }

    // Test is_even
    {
        let args = [42_i32.to_raya()];
        let result = unsafe { is_even_ffi(args.as_ptr(), args.len()) };
        let value = unsafe { result.as_value().as_bool().unwrap() };
        println!("is_even(42) = {}", value);
        assert_eq!(value, true);
    }

    // Test divide (success)
    {
        let args = [10_i32.to_raya(), 2_i32.to_raya()];
        let result = unsafe { divide_ffi(args.as_ptr(), args.len()) };
        let value = unsafe { result.as_value().as_i32().unwrap() };
        println!("divide(10, 2) = {}", value);
        assert_eq!(value, 5);
    }

    // Test divide (error)
    {
        let args = [10_i32.to_raya(), 0_i32.to_raya()];
        let result = unsafe { divide_ffi(args.as_ptr(), args.len()) };
        // Should return null (error placeholder for now)
        println!("divide(10, 0) = null (error)");
        assert!(unsafe { result.as_value().is_null() });
    }

    // Test module initialization
    {
        let module_ptr = raya_module_init();
        let module = unsafe { &*module_ptr };

        println!("\nModule: '{}' v{}", module.name(), module.version());
        println!("Functions: {:?}", module.function_names());

        assert_eq!(module.name(), "math");
        assert_eq!(module.version(), "1.0.0");
        assert_eq!(module.function_names().len(), 4);

        // Cleanup
        raya_module_cleanup(module_ptr);
    }

    println!("\n✓ All tests passed!");
}
