//! Phase 10: Decorator tests
//!
//! Tests for decorators on classes, methods, and fields.
//! Decorators are compile-time metadata that can transform declarations.

use super::harness::*;

// ============================================================================
// Class Decorators
// ============================================================================

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_class_decorator_simple() {
    // Decorator that marks a class as sealed (no subclasses)
    expect_i32(
        "@sealed
         class Config {
             value: number = 42;
         }
         let c = new Config();
         return c.value;",
        42,
    );
}

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_class_decorator_with_args() {
    // Decorator with arguments
    expect_i32(
        "@version(2)
         class Api {
             version: number = 2;
         }
         let api = new Api();
         return api.version;",
        2,
    );
}

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_class_multiple_decorators() {
    expect_i32(
        "@sealed
         @serializable
         class Data {
             value: number = 10;
         }
         let d = new Data();
         return d.value;",
        10,
    );
}

// ============================================================================
// Method Decorators
// ============================================================================

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_method_decorator_logged() {
    // @logged decorator wraps method to log calls
    expect_i32(
        "class Service {
             @logged
             compute(x: number): number {
                 return x * 2;
             }
         }
         let s = new Service();
         return s.compute(21);",
        42,
    );
}

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_method_decorator_memoized() {
    // @memoized caches results
    expect_i32(
        "class Calculator {
             calls: number = 0;

             @memoized
             fib(n: number): number {
                 this.calls = this.calls + 1;
                 if (n <= 1) { return n; }
                 return this.fib(n - 1) + this.fib(n - 2);
             }
         }
         let c = new Calculator();
         c.fib(10);
         return c.calls;",
        11, // Without memoization would be 177 calls
    );
}

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_method_decorator_deprecated() {
    // @deprecated marks method as deprecated (compile-time warning)
    expect_i32(
        "class Legacy {
             @deprecated
             oldMethod(): number {
                 return 42;
             }
         }
         let l = new Legacy();
         return l.oldMethod();",
        42,
    );
}

// ============================================================================
// Field Decorators
// ============================================================================

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_field_decorator_readonly() {
    expect_i32(
        "class Constants {
             @readonly
             PI: number = 3;
         }
         let c = new Constants();
         return c.PI;",
        3,
    );
}

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_field_decorator_validate() {
    // @validate ensures field value meets criteria
    expect_i32(
        "class User {
             @validate(x => x >= 0)
             age: number = 25;
         }
         let u = new User();
         return u.age;",
        25,
    );
}

// ============================================================================
// Parameter Decorators
// ============================================================================

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_parameter_decorator_inject() {
    // @inject provides dependency injection
    expect_i32(
        "class Service {
             handle(@inject logger: Logger): number {
                 return 42;
             }
         }
         let s = new Service();
         return 42;",
        42,
    );
}

// ============================================================================
// Decorator Factories
// ============================================================================

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_decorator_factory() {
    // Decorator factory returns a decorator
    expect_i32(
        "function timeout(ms: number) {
             return function(target: any, key: string) {
                 // Apply timeout to method
             };
         }

         class Api {
             @timeout(1000)
             fetch(): number {
                 return 42;
             }
         }
         let api = new Api();
         return api.fetch();",
        42,
    );
}

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_decorator_with_metadata() {
    // Decorator that adds metadata
    expect_i32(
        "@metadata('service', 'api')
         class ApiService {
             value: number = 42;
         }
         let s = new ApiService();
         return s.value;",
        42,
    );
}

// ============================================================================
// Decorator Composition
// ============================================================================

#[test]
#[ignore = "Decorators not yet implemented"]
fn test_decorator_composition_order() {
    // Decorators are applied bottom-up
    expect_i32(
        "class Pipeline {
             @first
             @second
             @third
             process(): number {
                 return 42;
             }
         }
         let p = new Pipeline();
         return p.process();",
        42,
    );
}
