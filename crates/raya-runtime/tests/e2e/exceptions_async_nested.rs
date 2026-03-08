use super::expect_i32;

#[test]
fn test_async_exception_in_nested_await() {
    expect_i32(
        "async function inner(): number {
             throw 'inner error';
             return 0;
         }
         async function outer(): number {
             return await inner();
         }
         let result = 0;
         try {
             result = await outer();
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}
