# Issue: `Task<T>` with Non-Number Types Returns Results Instead of Task IDs

## Status
**Discovered:** 2026-02-22  
**Severity:** High  
**Affects:** Compiler (lowering/codegen)

## Summary
Async functions that return non-number types (e.g., ``Task<string>``, ``Task<boolean>``) have their execution inlined when called inside other async functions. The array receives the **result values** (e.g., strings) instead of Task IDs (u64), causing `await [...]` to fail.

## Reproduction
```typescript
async function fetchUser(id: number): `Task<string>` {
    return `User ${id}`;
}

async function main(): Task<void> {
    // BUG: Array contains ["User 1", "User 2", "User 3"] 
    // instead of TaskId u64 values
    let users = await [fetchUser(1), fetchUser(2), fetchUser(3)];
}
```

**Error:** `Type error: Expected TaskId in array`

## Investigation Findings

### What Works
- ✅ ``Task<number>`` at any call site (top-level or nested)
- ✅ ``Task<string>`` at top-level (not inside async functions)  
- ✅ All existing tests (all use ``Task<number>``)

### What Fails
- ❌ ``Task<string>`` called inside async functions
- ❌ ``Task<boolean>`` and other non-number types (untested but likely)

### Root Cause Analysis
1. **Spawn opcode is NOT being executed** for non-number Task types
2. The compiler is emitting `Call` instead of `Spawn` for these functions
3. The async function executes synchronously and returns its result
4. Result values (strings) end up in the array instead of Task IDs
5. When `WaitAll` tries to extract u64 Task IDs, it reads string bytes as f64

**Evidence:** Raw bits `0x0000312072657355` decode to "User 1" (ASCII bytes, little-endian)

### Hypothesis
The compiler's async function detection (`async_functions` HashSet) is:
- Working correctly for ``Task<number>`` return types
- Failing for non-number Task types (`Task<string>`, `Task<boolean>`, etc.)

Possible causes:
1. Type checking during lowering treats `Task<string>` differently than `Task<number>`
2. Monomorphization or generic instantiation loses async marker for non-number types
3. Function lookup at call sites fails for specialized generic instances

## Workaround
Use ``Task<number>`` return types for now. If you need non-number results:
```typescript
// Workaround: Return index and map to strings later
async function fetchUserId(id: number): `Task<number>` {
    return id;
}
const ids = await [fetchUserId(1), fetchUserId(2), fetchUserId(3)];
const users = ids.map(id => `User ${id}`);
```

## Fix Strategy
1. **Add logging** to compiler lowering to track:
   - Which functions are registered as async
   - What function IDs are looked up at call sites
   - Why async_functions.contains() returns false

2. **Investigate type handling**:
   - Check if `Task<T>` generic instantiation loses async marker
   - Verify function_map lookups for generic functions
   - Test if monomorphization creates new function IDs

3. **Verify IR/codegen**:
   - Dump IR for failing test to see Call vs Spawn
   - Check if type coercion is happening in array lowering
   - Ensure Task registers preserve u64 encoding

## Tests
- `test_await_array_inline_with_strings` - Currently ignored
- `test_task_value_type` - Currently ignored

Both tests are in `crates/raya-runtime/tests/e2e/async_await.rs`

## Related Files
- `crates/raya-engine/src/compiler/lower/expr.rs` - Call site lowering (line 674)
- `crates/raya-engine/src/compiler/lower/mod.rs` - Async function registration (line 1030)
- `crates/raya-engine/src/vm/interpreter/opcodes/concurrency.rs` - Spawn/Await/WaitAll opcodes
- `crates/raya-engine/src/compiler/lower/expr.rs` - await lowering (line 2449)

## Impact
- Users cannot use `await [...]` with async functions returning strings, booleans, or custom types
- Significantly limits usefulness of concurrent task arrays
- Workaround is verbose and defeats the purpose of `Task<T>` generics
