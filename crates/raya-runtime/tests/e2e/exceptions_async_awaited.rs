use super::expect_i32;

#[test]
fn test_async_exception_caught_when_awaited() {
    expect_i32(
        "async function fail(): number {
             throw 'async error';
             return 0;
         }
         let result = 0;
         try {
             result = await fail();
         } catch (e) {
             result = 42;
         }
         return result;",
        42,
    );
}
