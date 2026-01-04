# Raya Specification Issues & Open Questions

This document lists inconsistencies, ambiguities, and areas requiring design decisions.

---

## 1. Cross-Reference Errors

### 1.1 Incorrect Section References

**Location:** README.md line 276, 309 and OPCODE.md line 388

**Issue:** References to "Section 17.6" should be "Section 17.7"
- Pattern matching is Section 17.6
- JSON serialization is Section 17.7

**Fix Required:**
- README.md line 276: ✅ Correct (references 17.6 for match)
- README.md line 309: ❌ Says "Section 17.6" but should say "Section 17.7" (JSON)
- OPCODE.md line 388: ❌ Says "LANG.md 17.6" but should say "LANG.md 17.7" (JSON)
- LANG.md line 2998: ❌ Says "Section 17.6" but should say "Section 17.7" (JSON)

---

## 2. Missing Documentation

### 2.1 Bare Primitive Unions in MAPPING.md

**Issue:** MAPPING.md has no section showing how bare primitive unions compile to bytecode.

**What's Missing:**
- How `string | number` is transformed to internal representation
- How assignment works (e.g., `id = 42` → wrapping logic)
- How operations unwrap values
- What opcodes are involved

**Example Needed:**
```ts
type ID = string | number;
let id: ID = 42;
id = "abc";
const x = id;  // How is unwrapping done?
```

**Decision Required:** How is this actually implemented at the bytecode level?

---

### 2.2 match() Utility in MAPPING.md

**Issue:** The `match()` utility is described in LANG.md Section 17.6 but has no bytecode mapping examples.

**What's Missing:**
1. How `match()` compiles for bare primitive unions
2. How `match()` compiles for discriminated unions
3. How discriminant field inference works (see 3.1 below)
4. What opcodes are generated

**Example Needed:**
```ts
// Bare union
match(id, {
  string: (s) => ...,
  number: (n) => ...
});

// Discriminated union
match(result, {
  ok: (r) => ...,
  err: (r) => ...
});
```

**Decision Required:** Is `match()` a language built-in or library function? How does it actually compile?

---

### 2.3 JsonValue Type Not Documented

**Issue:** `JsonValue` and `parseJson` are used in examples (LANG.md Section 17.8) but never formally defined.

**Location:** LANG.md lines 2690, 2701, 2723, etc.

**Example:**
```ts
import { JsonValue, parseJson } from "raya:json/internal";
```

**What's Missing:**
- Definition of `JsonValue` type structure
- Signature of `parseJson()` function
- Is this part of standard library or internal-only?
- Should this be in Section 17 (Standard Library)?

**Decision Required:** Formally specify the JSON API structure.

---

### 2.4 Map and Set Types

**Issue:** `Map<K, V>` is used in examples but never defined in standard library.

**Location:** LANG.md line 2812 uses `Map<string, JsonValue>`

**What's Missing:**
- Is `Map` a built-in type?
- What about `Set`?
- If yes, they should be documented in Section 17 (Standard Library)
- What are their methods and semantics?

**Decision Required:** Define standard collection types or remove from examples.

---

## 3. Ambiguities Requiring Clarification

### 3.1 Discriminant Field Inference

**Issue:** Documentation says "Compiler infers discriminant field automatically" but doesn't explain HOW.

**Location:** LANG.md line 2322

**Questions:**
1. How does the compiler choose between `kind`, `type`, `tag`, `status`, etc.?
2. What if a union has multiple fields that could be discriminants?
3. Is there a convention or does the user specify it?

**Example:**
```ts
type Result =
  | { status: "ok"; type: "success"; value: string }
  | { status: "error"; type: "failure"; error: string };

// Which field is the discriminant? status or type?
```

**Decision Required:** Specify discriminant field selection algorithm.

---

### 3.2 Bare Union Internal Representation

**Issue:** The internal representation `{ $type: "string"; $value: string }` is described but implementation details are unclear.

**Questions:**
1. Are `$type` and `$value` actual runtime field names?
2. Can user code access these fields?
3. How does assignment work? Is wrapping automatic everywhere?
4. What about function parameters and returns?
5. Are these fields in the object layout or compiler magic?

**Example:**
```ts
type ID = string | number;
let id: ID = 42;

// Can I do this?
console.log(id.$type);  // ???
console.log(id.$value); // ???

// Or is unwrapping always automatic?
const len = id.length;  // If id is string, this works?
```

**Decision Required:** Specify exact semantics of bare union transformation.

---

### 3.3 "Zero Runtime Overhead" Claim

**Issue:** README.md claims "Zero runtime overhead" but bare primitive unions DO have runtime overhead.

**Location:** README.md line 116

**Reality:**
- Bare unions require wrapping/unwrapping (overhead)
- The `$type` field must be checked by `match()` (overhead)
- Memory overhead for wrapper object

**More Accurate:** "Zero type tag overhead" or "Zero RTTI overhead"

**Decision Required:** Revise performance claims to be accurate.

---

### 3.4 Module Path Resolution

**Issue:** Module system section doesn't specify how imports are resolved.

**Location:** LANG.md Section 16, README.md lines 260-265

**What's Missing:**
1. How are module paths resolved? (relative vs absolute)
2. Is there a package manager?
3. What's the standard library naming convention?
   - Examples show: `raya:std`, `raya:json`, `raya:json/internal`, `raya:reflect`
   - But these aren't documented anywhere

**Decision Required:** Specify module resolution algorithm and standard library structure.

---

## 4. Potential Contradictions

### 4.1 Type Erasure vs Bare Unions

**Issue:** Documentation claims "all types erased" but bare unions add runtime type information.

**Locations:**
- README.md line 175: "No type tags on values"
- LANG.md Section 4.3: Bare unions use `$type` field

**Question:** Is `$type` a "type tag" or a "discriminant value"?

**Clarification Needed:** The distinction is:
- **Type tags** (banned): RTTI, instanceof checks
- **Discriminant values** (allowed): String/number fields checked at runtime

But this should be made explicit to avoid confusion.

---

### 4.2 "No Runtime Type Checks" vs Value Checks

**Issue:** Claims "zero runtime type checks" but discriminant checks ARE runtime checks.

**More Accurate Phrasing:**
- "No runtime type introspection (typeof/instanceof)"
- "Only value-based discriminant checks"
- "No type tags or RTTI"

**Decision Required:** Clarify terminology throughout documents.

---

## 5. Missing Standard Library Specification

### 5.1 Incomplete Standard Library

**Issue:** Section 17 lists standard library modules but doesn't specify their APIs.

**What's Listed (README.md):**
- Console: `console.log`, `console.error`
- Math: "Basic math operations" (which ones?)
- String: "Standard string methods" (which ones?)
- Array: "Standard array methods" (which ones?)

**What's Missing:**
- Full API signatures
- Method specifications
- Collection types (Map, Set, etc.)
- Error types
- File I/O (if supported)
- Network I/O (if supported)

**Decision Required:** Either specify full standard library or mark as "implementation-defined."

---

### 5.2 Error Type Not Defined

**Issue:** Examples use `Error` type but it's never defined.

**Locations:**
- LANG.md Section 17.7: `Result<T, Error>`
- LANG.md Section 17.8: `Result<UserId, Error>`

**What's Missing:**
```ts
class Error {
  message: string;
  // ... other fields?
}
```

**Decision Required:** Define standard Error type or use string for errors.

---

## 6. Interoperability & FFI

### 6.1 No FFI Specification

**Issue:** No mention of how to call external code (C, Rust, OS APIs, etc.).

**Questions:**
1. Can Raya call native libraries?
2. How does file I/O work?
3. How does network I/O work?
4. Is there an FFI mechanism?

**Decision Required:** Specify FFI or mark as future work.

---

## 7. Implementation Questions

### 7.1 Monomorphization Limits

**Issue:** Monomorphization can lead to code bloat.

**Questions:**
1. Is there a limit on generic instantiations?
2. What happens with recursive generics?
3. How are errors reported?

**Example:**
```ts
function recurse<T>(x: T): recurse<T> { ... }  // ???
```

**Decision Required:** Specify limits and error handling.

---

### 7.2 Vtable Dispatch Details

**Issue:** Documents mention "vtable dispatch" but don't specify how it works.

**Questions:**
1. How are vtables laid out?
2. What about interface method calls?
3. How is inheritance handled?

**Decision Required:** Specify vtable structure in ARCHITECTURE.md or mark as implementation detail.

---

## 8. Concurrency Questions

### 8.1 Task Stack Size

**Issue:** No mention of Task stack limits.

**Questions:**
1. What's the stack size for each Task?
2. Is it configurable?
3. What happens on stack overflow?

**Decision Required:** Specify Task stack behavior.

---

### 8.2 Task Cancellation

**Issue:** No mechanism for cancelling Tasks.

**Example:**
```ts
const task = longRunningOperation();
// How do I cancel it?
```

**Decision Required:** Add task cancellation or explicitly mark as not supported.

---

## 9. Type System Edge Cases

### 9.1 Circular References in Types

**Issue:** No mention of how circular type references are handled.

**Example:**
```ts
interface Node {
  value: number;
  next: Node | null;  // Circular reference
}
```

**Question:** Is this allowed? How is it compiled?

**Decision Required:** Specify behavior for circular types.

---

### 9.2 Generic Constraints

**Issue:** No syntax or semantics for generic constraints.

**Example:**
```ts
function sort<T extends Comparable>(items: T[]): T[] { ... }
```

**Questions:**
1. Are generic constraints supported?
2. If not, should they be in "Future Extensions"?

**Decision Required:** Clarify generic constraint support.

---

## 10. Recommendations

### Priority 1 (Blocking Issues)
1. Fix cross-reference errors (Section 17.6 vs 17.7)
2. Add bare union bytecode mapping to MAPPING.md
3. Add match() bytecode mapping to MAPPING.md
4. Clarify discriminant field inference algorithm
5. Define JsonValue and JSON API formally

### Priority 2 (Important Clarifications)
1. Revise "zero runtime overhead" claims to be accurate
2. Specify bare union internal representation details
3. Document module resolution and standard library structure
4. Define Error type
5. Clarify type tag vs discriminant value terminology

### Priority 3 (Future Work)
1. Specify full standard library APIs
2. Add FFI/interop specification
3. Specify vtable layout (or mark as impl detail)
4. Add task cancellation or mark as not supported
5. Specify generic constraints support

---

## Summary

The specification is **comprehensive and well-designed**, but has:
- **4 cross-reference errors** (easy fixes)
- **2 major missing sections** (bare unions and match() in MAPPING.md)
- **Several ambiguities** (discriminant inference, bare union internals)
- **Terminology that needs clarification** (zero overhead, type tags vs discriminants)
- **Missing standard library details** (APIs, Error type, collections)

Most issues are documentation gaps rather than design flaws. The core design is sound.
