# Milestone 3.3: Code Generation

**Status:** Planned
**Dependencies:** Milestone 3.1 (IR) ✅, Milestone 3.2 (Monomorphization) ✅
**Reference:** `design/MAPPING.md`

---

## Overview

Code generation transforms the optimized IR into bytecode. This milestone includes the bytecode emitter and key optimizations like string literal comparison optimization.

---

## Implementation Phases

### Phase 1: Bytecode Emitter Foundation

**Goal:** Basic bytecode emission infrastructure.

**Tasks:**
- [ ] Implement `CodeGenerator` structure
- [ ] Emit bytecode for basic expressions (literals, identifiers)
- [ ] Emit bytecode for binary/unary operations
- [ ] Handle local variable load/store
- [ ] Emit function calls

**Files:**
```
crates/raya-compiler/src/codegen.rs
crates/raya-compiler/src/module_builder.rs
```

---

### Phase 2: Control Flow

**Goal:** Generate bytecode for control flow constructs.

**Tasks:**
- [ ] Emit `if/else` statements with jump patching
- [ ] Emit `while` loops
- [ ] Emit `for` loops
- [ ] Emit `switch/match` statements
- [ ] Handle `break/continue`
- [ ] Handle `return` statements

---

### Phase 3: Classes and Objects

**Goal:** Generate bytecode for object-oriented features.

**Tasks:**
- [ ] Emit class definitions
- [ ] Generate vtables for method dispatch
- [ ] Emit `new` expressions
- [ ] Emit field access/store
- [ ] Emit method calls
- [ ] Handle constructors

---

### Phase 4: Closures

**Goal:** Generate bytecode for closures with captured variables.

**Tasks:**
- [ ] Detect captured variables
- [ ] Emit closure object creation
- [ ] Store captured variables in closure
- [ ] Load captured variables in closure body

---

### Phase 5: String Comparison Optimization

**Goal:** Optimize string comparisons when operands are constant pool references.

**Motivation:**
```typescript
let x = "hello";  // Stored in constant pool[5]
let y = "hello";  // Same constant pool entry (deduplicated)

x == y  // Can use pointer/index comparison instead of SEQ
```

**Tasks:**
- [ ] Track value origins (Constant vs Computed)
- [ ] Implement `ValueOrigin` enum for tracking
- [ ] Optimize constant-constant string comparison to index comparison
- [ ] Optimize string literal union type comparisons (always use IEQ)
- [ ] Fall back to SEQ only for general `string` type

**Key Insight: Type-Based Optimization**

If a value has type `"a" | "b"` (string literal union), it MUST be one of those literals,
even if computed at runtime (e.g., function return). We can always use index comparison.

```typescript
function getStatus(): "active" | "inactive" {
    return Math.random() > 0.5 ? "active" : "inactive";
}

let status = getStatus();  // Type is "active" | "inactive"
status == "active"         // Can use IEQ! Type guarantees it's a known constant
```

**Key Implementation:**

```rust
/// Tracks where a value came from
#[derive(Debug, Clone, Copy)]
pub enum ValueOrigin {
    /// Value is from constant pool (string literal, number literal, etc.)
    Constant(u16),  // Constant pool index
    /// Value was computed at runtime (concat, function return, etc.)
    Computed,
}

/// Check if type is a string literal union
fn is_string_literal_union(ty: &Type) -> bool {
    match ty {
        Type::Union(variants) => variants.iter().all(|v| matches!(v, Type::StringLiteral(_))),
        Type::StringLiteral(_) => true,
        _ => false,
    }
}

impl CodeGenerator {
    /// Emit comparison with optimization for constant strings
    fn emit_string_comparison(
        &mut self,
        left_ty: &Type,
        right_ty: &Type,
        left_origin: ValueOrigin,
        right_origin: ValueOrigin,
    ) {
        // Priority 1: If EITHER type is a string literal union, use IEQ
        // (the type guarantees the value is a known constant)
        if is_string_literal_union(left_ty) || is_string_literal_union(right_ty) {
            // Both operands must resolve to constant pool indices
            // For string literal unions, values are stored as indices
            self.emit(Opcode::Ieq);
            return;
        }

        // Priority 2: Both are known constants at compile time
        match (left_origin, right_origin) {
            (ValueOrigin::Constant(_), ValueOrigin::Constant(_)) => {
                // Pointers will be same if same constant pool entry
                // SEQ with pointer check will handle this at runtime
                self.emit(Opcode::Seq);
            }
            // General string type: full comparison
            _ => {
                self.emit(Opcode::Seq);
            }
        }
    }
}
```

**Performance Comparison:**

| Scenario | Type | Before | After | Speedup |
|----------|------|--------|-------|---------|
| `"a" == "b"` | literal | O(n) SEQ | O(1) IEQ | 10-100x |
| `x == "hello"` (x from literal) | literal | O(n) SEQ | O(1) IEQ | 10-100x |
| `f() == "a"` where `f(): "a"\|"b"` | literal union | O(n) SEQ | O(1) IEQ | 10-100x |
| `x == y` (both `"a"\|"b"` type) | literal union | O(n) SEQ | O(1) IEQ | 10-100x |
| `x == y` (both from literals, type `string`) | string | O(n) SEQ | O(1) ptr | 10-100x |
| `x == userInput()` | string | O(n) SEQ | O(n) SEQ | 1x |

**Tests:**
- [ ] Constant-constant comparison uses IEQ
- [ ] Constant-variable (from literal) uses IEQ
- [ ] Computed string falls back to SEQ
- [ ] String literal union comparison uses IEQ

---

### Phase 5b: SEQ Pointer Check Optimization (Runtime)

**Goal:** Optimize SEQ opcode to check pointer equality first before string comparison.

**Motivation:**
Even when we can't use IEQ at compile time, we can still optimize SEQ at runtime:
```typescript
let x = "hello";
let y = x;        // y points to same string as x

x == y  // SEQ called, but pointers are same → return true immediately
```

**Implementation (in raya-core VM):**

```rust
// crates/raya-core/src/interpreter.rs

fn execute_seq(&mut self) -> Result<()> {
    let right = self.pop_string()?;
    let left = self.pop_string()?;

    // Optimization: Check pointer equality first (O(1))
    if std::ptr::eq(left.as_ptr(), right.as_ptr()) {
        self.push(Value::bool(true));
        return Ok(());
    }

    // Fall back to character comparison (O(n))
    let result = left == right;
    self.push(Value::bool(result));
    Ok(())
}
```

**Why this helps:**
1. **Same constant pool entry** → Same pointer → O(1)
2. **Variable assigned from another** → Same pointer → O(1)
3. **Different strings** → Full comparison → O(n) (no worse than before)

**Combined optimization flow:**

```
Compile time:
  Both constants? → Use IEQ (compare indices)
  Otherwise      → Use SEQ

Runtime (SEQ):
  Same pointer?  → Return true immediately (O(1))
  Otherwise      → Compare characters (O(n))
```

**Tests:**
- [ ] SEQ returns true immediately for same pointer
- [ ] SEQ correctly compares different strings
- [ ] No performance regression for different strings

---

### Phase 6: Integration

**Goal:** Integrate code generation into compiler pipeline.

**Tasks:**
- [ ] Wire up IR → Bytecode in `Compiler::compile()`
- [ ] Add debug output for generated bytecode
- [ ] Bytecode verification pass
- [ ] Performance benchmarks

---

## Files to Create/Modify

```
crates/raya-compiler/src/codegen.rs           # Main code generator
crates/raya-compiler/src/codegen/expr.rs      # Expression codegen
crates/raya-compiler/src/codegen/stmt.rs      # Statement codegen
crates/raya-compiler/src/codegen/control.rs   # Control flow
crates/raya-compiler/src/codegen/class.rs     # Class/object codegen
crates/raya-compiler/src/codegen/optimize.rs  # Codegen optimizations
```

---

## Success Criteria

1. **Functionality**
   - [ ] All IR constructs emit correct bytecode
   - [ ] Control flow works correctly
   - [ ] Classes and methods work
   - [ ] Closures capture variables correctly

2. **Optimization**
   - [ ] String literal comparisons use O(1) index comparison
   - [ ] Value origin tracking is accurate
   - [ ] No regression for computed string comparisons

3. **Testing**
   - [ ] Unit tests for each codegen phase
   - [ ] Integration tests with VM execution
   - [ ] Performance benchmarks for string comparison

---

## References

- `design/MAPPING.md` - Language to bytecode mappings
- `design/OPCODE.md` - Bytecode instruction set
- Milestone 3.1 (IR) - Input format
- Milestone 3.2 (Monomorphization) - Specialized code input

---

**Last Updated:** 2026-01-26
