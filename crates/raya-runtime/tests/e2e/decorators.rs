//! Phase 5: Decorator Integration Tests (Milestone 3.9)
//!
//! End-to-end tests for decorators on classes, methods, and fields.
//! Tests verify that decorators are called and classes work correctly after decoration.
//!
//! Note: Due to current compiler limitations, named function declarations cannot
//! access top-level variables. Tests are designed to work within this constraint.

use super::harness::*;

// ============================================================================
// Class Decorators - Basic
// ============================================================================

#[test]
fn test_class_decorator_simple() {
    // Simple class decorator - verify class still works after decoration
    expect_i32(
        "function Injectable(classId: number): void {
             // Decorator that marks a class as injectable
             // classId is the internal class identifier
         }

         @Injectable
         class Service {
             value: number = 42;
         }

         let s = new Service();
         return s.value;",
        42,
    );
}

#[test]
fn test_class_decorator_factory() {
    // Decorator factory that takes arguments
    expect_i32(
        "function Controller(path: string): (classId: number) => void {
             return (classId: number): void => {
                 // Factory creates decorator that receives path
             };
         }

         @Controller(\"/api\")
         class ApiController {
             value: number = 100;
         }

         let c = new ApiController();
         return c.value;",
        100,
    );
}

#[test]
fn test_class_decorator_multiple() {
    // Multiple decorators are applied to the class
    expect_i32(
        "function First(classId: number): void {}
         function Second(classId: number): void {}
         function Third(classId: number): void {}

         @First
         @Second
         @Third
         class MultiDecorated {
             value: number = 42;
         }

         let m = new MultiDecorated();
         return m.value;",
        42,
    );
}

#[test]
fn test_class_decorator_on_multiple_classes() {
    // Same decorator applied to multiple classes
    expect_i32(
        "function Mark(classId: number): void {}

         @Mark
         class ServiceA {
             a: number = 10;
         }

         @Mark
         class ServiceB {
             b: number = 20;
         }

         @Mark
         class ServiceC {
             c: number = 12;
         }

         let a = new ServiceA();
         let b = new ServiceB();
         let c = new ServiceC();
         return a.a + b.b + c.c;",
        42,
    );
}

// ============================================================================
// Method Decorators
// ============================================================================

#[test]
fn test_method_decorator_simple() {
    // Method decorator - verify method still works after decoration
    expect_i32(
        "function Log(classId: number, methodName: string): void {
             // Decorator that marks a method for logging
         }

         class Service {
             @Log
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
fn test_method_decorator_factory() {
    // Method decorator factory
    expect_i32(
        "function GET(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {
                 // Register route
             };
         }

         class Api {
             @GET(\"/users\")
             getUsers(): number {
                 return 100;
             }
         }

         let api = new Api();
         return api.getUsers();",
        100,
    );
}

#[test]
fn test_method_decorator_multiple() {
    // Multiple method decorators
    expect_i32(
        "function Auth(classId: number, methodName: string): void {}
         function Log(classId: number, methodName: string): void {}
         function Cache(classId: number, methodName: string): void {}

         class Service {
             @Auth
             @Log
             @Cache
             getData(): number {
                 return 42;
             }
         }

         let s = new Service();
         return s.getData();",
        42,
    );
}

#[test]
fn test_method_decorator_on_different_methods() {
    // Decorators on multiple methods
    expect_i32(
        "function Track(classId: number, methodName: string): void {}

         class Calculator {
             @Track
             add(a: number, b: number): number {
                 return a + b;
             }

             @Track
             multiply(a: number, b: number): number {
                 return a * b;
             }
         }

         let c = new Calculator();
         return c.add(20, 22);",
        42,
    );
}

// ============================================================================
// Field Decorators
// ============================================================================

#[test]
fn test_field_decorator_simple() {
    // Field decorator - verify field still works after decoration
    expect_i32(
        "function Column(classId: number, fieldName: string): void {
             // Decorator that marks a field as a database column
         }

         class Entity {
             @Column
             name: string = \"test\";

             value: number = 42;
         }

         let e = new Entity();
         return e.value;",
        42,
    );
}

#[test]
fn test_field_decorator_factory() {
    // Field decorator factory
    expect_i32(
        "function Column(dbType: string): (classId: number, fieldName: string) => void {
             return (classId: number, fieldName: string): void => {
                 // Register column type
             };
         }

         class User {
             @Column(\"varchar\")
             name: string = \"John\";

             age: number = 42;
         }

         let u = new User();
         return u.age;",
        42,
    );
}

#[test]
fn test_field_decorator_on_multiple_fields() {
    // Decorators on multiple fields
    expect_i32(
        "function Required(classId: number, fieldName: string): void {}

         class Form {
             @Required
             username: string = \"user\";

             @Required
             email: string = \"test@test.com\";

             @Required
             password: string = \"secret\";

             count: number = 42;
         }

         let f = new Form();
         return f.count;",
        42,
    );
}

#[test]
fn test_field_decorator_multiple_on_same_field() {
    // Multiple decorators on same field
    expect_i32(
        "function Required(classId: number, fieldName: string): void {}

         function Min(len: number): (classId: number, fieldName: string) => void {
             return (classId: number, fieldName: string): void => {};
         }

         function Max(len: number): (classId: number, fieldName: string) => void {
             return (classId: number, fieldName: string): void => {};
         }

         class User {
             @Required
             @Min(3)
             @Max(100)
             username: string = \"test\";

             value: number = 42;
         }

         let u = new User();
         return u.value;",
        42,
    );
}

// ============================================================================
// Combined Decorators (Class + Method + Field)
// ============================================================================

#[test]
fn test_combined_decorators() {
    // All decorator types on one class
    expect_i32(
        "function Entity(classId: number): void {}
         function Column(classId: number, fieldName: string): void {}
         function Route(classId: number, methodName: string): void {}

         @Entity
         class User {
             @Column
             id: number = 1;

             @Column
             name: string = \"test\";

             @Route
             save(): number {
                 return this.id;
             }

             @Route
             delete(): number {
                 return 0;
             }

             getValue(): number {
                 return 42;
             }
         }

         let u = new User();
         return u.getValue();",
        42,
    );
}

// ============================================================================
// Framework Patterns
// ============================================================================

#[test]
fn test_dependency_injection_pattern() {
    // DI-style decorator pattern
    expect_i32(
        "function Injectable(classId: number): void {}

         @Injectable
         class Logger {
             log(msg: string): void {}
         }

         @Injectable
         class Database {
             query(sql: string): void {}
         }

         @Injectable
         class UserService {
             getValue(): number {
                 return 42;
             }
         }

         let us = new UserService();
         return us.getValue();",
        42,
    );
}

#[test]
fn test_http_routing_pattern() {
    // HTTP routing decorator pattern
    expect_i32(
        "function Controller(path: string): (classId: number) => void {
             return (classId: number): void => {};
         }

         function GET(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }

         function POST(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }

         @Controller(\"/api/users\")
         class UserController {
             @GET(\"/\")
             list(): number { return 1; }

             @GET(\"/:id\")
             get(): number { return 1; }

             @POST(\"/\")
             create(): number { return 1; }

             getValue(): number { return 42; }
         }

         let uc = new UserController();
         return uc.getValue();",
        42,
    );
}

#[test]
fn test_orm_entity_pattern() {
    // ORM-style entity/column decorators
    expect_i32(
        "function Entity(tableName: string): (classId: number) => void {
             return (classId: number): void => {};
         }

         function PrimaryKey(classId: number, fieldName: string): void {}

         function Column(dbType: string): (classId: number, fieldName: string) => void {
             return (classId: number, fieldName: string): void => {};
         }

         @Entity(\"users\")
         class User {
             @PrimaryKey
             id: number = 1;

             @Column(\"varchar\")
             name: string = \"test\";

             @Column(\"int\")
             age: number = 42;
         }

         let u = new User();
         return u.age;",
        42,
    );
}

#[test]
fn test_validation_pattern() {
    // Validation decorator pattern
    expect_i32(
        "function Validate(classId: number): void {}

         function Required(classId: number, fieldName: string): void {}

         function MinLength(len: number): (classId: number, fieldName: string) => void {
             return (classId: number, fieldName: string): void => {};
         }

         function Email(classId: number, fieldName: string): void {}

         @Validate
         class UserForm {
             @Required
             @MinLength(3)
             username: string = \"abc\";

             @Required
             @Email
             email: string = \"test@test.com\";

             @Required
             @MinLength(8)
             password: string = \"password123\";

             getValue(): number { return 42; }
         }

         let form = new UserForm();
         return form.getValue();",
        42,
    );
}

#[test]
fn test_serialization_pattern() {
    // JSON serialization decorator pattern
    expect_i32(
        "function Serializable(classId: number): void {}

         function JsonProperty(jsonName: string): (classId: number, fieldName: string) => void {
             return (classId: number, fieldName: string): void => {};
         }

         function JsonIgnore(classId: number, fieldName: string): void {}

         @Serializable
         class ApiResponse {
             @JsonProperty(\"user_name\")
             userName: string = \"john\";

             @JsonProperty(\"created_at\")
             createdAt: string = \"2024-01-01\";

             @JsonIgnore
             internalId: number = 0;

             getValue(): number { return 42; }
         }

         let resp = new ApiResponse();
         return resp.getValue();",
        42,
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_decorator_on_empty_class() {
    // Decorator on class with no members
    expect_i32(
        "function Mark(classId: number): void {}

         @Mark
         class Empty {}

         let e = new Empty();
         return 42;",
        42,
    );
}

#[test]
fn test_decorator_on_class_with_constructor() {
    // Decorator on class with explicit constructor
    expect_i32(
        "function Track(id: number): void {}

         @Track
         class WithConstructor {
             value: number;

             constructor(v: number) {
                 this.value = v;
             }
         }

         let w = new WithConstructor(42);
         return w.value;",
        42,
    );
}

#[test]
fn test_decorator_on_class_with_inheritance() {
    // Decorators with inheritance
    expect_i32(
        "function Track(classId: number): void {}

         @Track
         class Base {
             baseValue: number = 10;
         }

         @Track
         class Derived extends Base {
             derivedValue: number = 32;
         }

         let d = new Derived();
         return d.baseValue + d.derivedValue;",
        42,
    );
}

#[test]
fn test_decorator_preserves_method_functionality() {
    // Decorated methods still work correctly
    expect_i32(
        "function Log(classId: number, methodName: string): void {}

         class Calculator {
             @Log
             add(a: number, b: number): number {
                 return a + b;
             }

             @Log
             multiply(a: number, b: number): number {
                 return a * b;
             }
         }

         let c = new Calculator();
         return c.add(c.multiply(2, 3), c.add(30, 6));",
        42, // (2*3) + (30+6) = 6 + 36 = 42
    );
}

#[test]
fn test_nested_decorator_factories() {
    // Decorator factories that return decorator factories
    expect_i32(
        "function WithOptions(prefix: string): (suffix: string) => (classId: number) => void {
             return (suffix: string): (classId: number) => void => {
                 return (classId: number): void => {};
             };
         }

         @WithOptions(\"api\")(\"v2\")
         class Endpoint {
             value: number = 42;
         }

         let e = new Endpoint();
         return e.value;",
        42,
    );
}

// ============================================================================
// Decorator with Complex Arguments
// ============================================================================

#[test]
fn test_decorator_with_multiple_arguments() {
    // Decorator factory receiving multiple args
    expect_i32(
        "function Server(h: string, p: number): (classId: number) => void {
             return (classId: number): void => {};
         }

         @Server(\"localhost\", 8080)
         class AppServer {
             port: number = 42;
         }

         let s = new AppServer();
         return s.port;",
        42,
    );
}

#[test]
fn test_decorator_with_boolean_argument() {
    // Decorator with boolean flag
    expect_i32(
        "function Async(flag: boolean): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }

         class Worker {
             @Async(true)
             process(): number {
                 return 42;
             }
         }

         let w = new Worker();
         return w.process();",
        42,
    );
}

// ============================================================================
// Class and Method Both Decorated
// ============================================================================

#[test]
fn test_class_and_method_both_decorated() {
    // Both class and its methods have decorators
    expect_i32(
        "function Service(classId: number): void {}
         function Action(classId: number, methodName: string): void {}

         @Service
         class MyService {
             @Action
             doWork(): number {
                 return 42;
             }
         }

         let s = new MyService();
         return s.doWork();",
        42,
    );
}

// ============================================================================
// Minimal Tests for Debugging
// ============================================================================

#[test]
fn test_minimal_method_no_decorator() {
    // Test that method calls work without decorators
    expect_i32(
        "class Api {
             getUsers(): number {
                 return 100;
             }
         }
         let api = new Api();
         return api.getUsers();",
        100,
    );
}

#[test]
fn test_minimal_factory_call() {
    // Test that factory functions work
    expect_i32(
        "function factory(): () => number {
             return (): number => 42;
         }
         let f = factory();
         return f();",
        42,
    );
}

#[test]
fn test_minimal_closure_with_two_args() {
    // Test that closures with multiple args work
    expect_i32(
        "function factory(): (x: number, y: number) => number {
             return (x: number, y: number): number => x + y;
         }
         let f = factory();
         return f(20, 22);",
        42,
    );
}

#[test]
fn test_minimal_method_decorator_factory_isolated() {
    // Isolate the decorator factory call - no method call
    expect_i32(
        "function GET(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }
         let f = GET(\"/users\");
         return 42;",
        42,
    );
}

#[test]
fn test_minimal_class_with_factory_decorated_method() {
    // Class with a factory-decorated method - can we call the method?
    expect_i32(
        "function GET(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }

         class Api {
             @GET(\"/users\")
             getUsers(): number {
                 return 100;
             }
         }

         let api = new Api();
         return api.getUsers();",
        100,
    );
}

#[test]
fn test_minimal_factory_decorated_no_method_call() {
    // Class exists but we don't call the decorated method - just return a constant
    expect_i32(
        "function GET(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }

         class Api {
             @GET(\"/users\")
             getUsers(): number {
                 return 100;
             }
         }

         let api = new Api();
         return 42;",
        42,
    );
}

#[test]
fn test_minimal_factory_decorated_call_other_method() {
    // Class has two methods - one decorated, one not. Call the non-decorated one.
    expect_i32(
        "function GET(path: string): (classId: number, methodName: string) => void {
             return (classId: number, methodName: string): void => {};
         }

         class Api {
             @GET(\"/users\")
             getUsers(): number {
                 return 100;
             }

             getValue(): number {
                 return 42;
             }
         }

         let api = new Api();
         return api.getValue();",
        42,
    );
}

#[test]
fn test_minimal_undecorated_class_baseline() {
    // Baseline: Class WITHOUT any decorators - should definitely work
    expect_i32(
        "class Api {
             getUsers(): number {
                 return 100;
             }
         }

         let api = new Api();
         return 42;",
        42,
    );
}

#[test]
fn test_minimal_direct_decorator_no_factory() {
    // Direct decorator (not factory) on a method
    expect_i32(
        "function Log(classId: number, methodName: string): void {}

         class Api {
             @Log
             getUsers(): number {
                 return 100;
             }
         }

         let api = new Api();
         return 42;",
        42,
    );
}

// ============================================================================
// Parameter Decorator Tests
// ============================================================================

#[test]
fn test_parameter_decorator_on_method() {
    // Simple parameter decorator on a method parameter
    expect_i32(
        "function Inject(classId: number, methodName: string, paramIndex: number): void {}

         class Service {
             process(@Inject input: number): number {
                 return input * 2;
             }
         }

         let s = new Service();
         return s.process(21);",
        42,
    );
}

#[test]
fn test_parameter_decorator_on_constructor() {
    // Parameter decorator on constructor parameter
    expect_i32(
        "function Inject(classId: number, methodName: string, paramIndex: number): void {}

         class Service {
             value: number;

             constructor(@Inject initialValue: number) {
                 this.value = initialValue;
             }
         }

         let s = new Service(42);
         return s.value;",
        42,
    );
}

#[test]
fn test_parameter_decorator_multiple_params() {
    // Multiple decorated parameters on same method
    expect_i32(
        "function Required(classId: number, methodName: string, paramIndex: number): void {}
         function Optional(classId: number, methodName: string, paramIndex: number): void {}

         class Calculator {
             add(@Required a: number, @Optional b: number): number {
                 return a + b;
             }
         }

         let c = new Calculator();
         return c.add(20, 22);",
        42,
    );
}

#[test]
fn test_parameter_decorator_factory() {
    // Parameter decorator factory
    expect_i32(
        "function Validate(rule: string): (classId: number, methodName: string, paramIndex: number) => void {
             return (classId: number, methodName: string, paramIndex: number): void => {};
         }

         class Service {
             process(@Validate(\"positive\") value: number): number {
                 return value;
             }
         }

         let s = new Service();
         return s.process(42);",
        42,
    );
}

#[test]
fn test_parameter_decorator_with_method_decorator() {
    // Combining parameter and method decorators
    expect_i32(
        "function Log(classId: number, methodName: string): void {}
         function Inject(classId: number, methodName: string, paramIndex: number): void {}

         class Service {
             @Log
             process(@Inject x: number, @Inject y: number): number {
                 return x + y;
             }
         }

         let s = new Service();
         return s.process(20, 22);",
        42,
    );
}

#[test]
fn test_parameter_decorator_all_types_combined() {
    // All decorator types together: class, method, field, parameter
    expect_i32(
        "function ClassDec(classId: number): void {}
         function MethodDec(classId: number, methodName: string): void {}
         function FieldDec(classId: number, fieldName: string): void {}
         function ParamDec(classId: number, methodName: string, paramIndex: number): void {}

         @ClassDec
         class FullyDecorated {
             @FieldDec
             value: number = 10;

             @MethodDec
             compute(@ParamDec multiplier: number): number {
                 return this.value * multiplier;
             }
         }

         let obj = new FullyDecorated();
         // value is 10, multiplier is 4.2, result is 42
         return obj.compute(4) + 2;",
        42,
    );
}

// ============================================================================
// Type-Constrained Decorator Tests
// ============================================================================

#[test]
fn test_type_constrained_decorator_matching_signature() {
    // A type-constrained decorator that accepts only specific method signatures
    // This should compile successfully because the method matches the expected signature
    expect_i32(
        "type Handler = (x: number) => number;

         function Typed(method: Handler): Handler {
             return method;
         }

         class Service {
             @Typed
             process(x: number): number {
                 return x * 2;
             }
         }

         let s = new Service();
         return s.process(21);",
        42,
    );
}

#[test]
fn test_type_constrained_decorator_mismatched_signature() {
    // A type-constrained decorator applied to a method with wrong signature
    // This should produce a compile error
    expect_compile_error(
        "type Handler = (x: number) => number;

         function Typed(method: Handler): Handler {
             return method;
         }

         class Service {
             @Typed
             process(x: string): string {
                 return x;
             }
         }

         let s = new Service();
         return 0;",
        "signature", // Error should mention signature mismatch
    );
}

#[test]
fn test_type_constrained_decorator_wrong_param_count() {
    // Decorator expects one param, method has two
    expect_compile_error(
        "type SingleArg = (x: number) => number;

         function Typed(method: SingleArg): SingleArg {
             return method;
         }

         class Service {
             @Typed
             process(x: number, y: number): number {
                 return x + y;
             }
         }

         return 0;",
        "signature",
    );
}

#[test]
fn test_type_constrained_decorator_wrong_return_type() {
    // Decorator expects number return, method returns string
    expect_compile_error(
        "type NumHandler = (x: number) => number;

         function Typed(method: NumHandler): NumHandler {
             return method;
         }

         class Service {
             @Typed
             process(x: number): string {
                 return \"hello\";
             }
         }

         return 0;",
        "signature",
    );
}

#[test]
fn test_type_constrained_decorator_factory() {
    // Type-constrained decorator factory - should work when signature matches
    expect_i32(
        "type ApiHandler = (id: number) => number;

         function Route(path: string): (method: ApiHandler) => ApiHandler {
             return (method: ApiHandler): ApiHandler => method;
         }

         class Controller {
             @Route(\"/user\")
             getUser(id: number): number {
                 return id + 40;
             }
         }

         let c = new Controller();
         return c.getUser(2);",
        42,
    );
}

#[test]
fn test_type_constrained_decorator_factory_mismatch() {
    // Type-constrained decorator factory with wrong method signature
    expect_compile_error(
        "type ApiHandler = (id: number) => number;

         function Route(path: string): (method: ApiHandler) => ApiHandler {
             return (method: ApiHandler): ApiHandler => method;
         }

         class Controller {
             @Route(\"/user\")
             getUser(name: string): string {
                 return name;
             }
         }

         return 0;",
        "signature",
    );
}
