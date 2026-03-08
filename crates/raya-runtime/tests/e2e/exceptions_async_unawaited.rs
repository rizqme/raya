use super::expect_i32;

#[test]
fn test_async_exception_not_caught_without_await() {
    expect_i32(
        "async function fail(): number {
             throw 'async error';
             return 0;
         }
         let result = 42;
         try {
             fail();
             result = 42;
         } catch (e) {
             result = 0;
         }
         return result;",
        42,
    );
}
