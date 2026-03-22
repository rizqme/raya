# Phase 1 Parser Workplan

Goal:
- move the parser from “Raya-flavored JS” toward context-aware ECMAScript parsing
- specifically reduce the `asi`, generator, spread/rest, and class-element syntax clusters

This document is the concrete follow-on to:
- `test_analysis/test262/2026_03_22/engine_rework_plan.md`

## Work Items By File

### `crates/raya-engine/src/parser/parser.rs`

Add parser-context helpers and shared token-boundary queries:
- explicit helpers for:
  - line terminator before current token
  - semicolon-insertion boundary
  - class-element parsing context
  - allow/disallow `yield`
  - allow/disallow `await`
- longer term:
  - replace the current ad hoc booleans/counters with a compact parser context struct

Status now:
- lexer-backed token trivia landed through `LexedToken`:
  - `has_line_terminator_before_current`
  - `has_line_terminator_before_peek`
  - `can_insert_semicolon_before_current`
- newline-sensitive parser decisions no longer depend on span-line approximation

### `crates/raya-engine/src/parser/parser/stmt.rs`

Rework statement parsing around ECMAScript “no LineTerminator here” rules:
- `return`
- `throw`
- `break`
- `continue`
- `yield`

Then tackle statement forms that currently misparse under ASI:
- labeled statements vs expression statements
- `if/else` newline-sensitive fallthrough cases
- block-scope declarations in statement positions

Status now:
- first ASI-sensitive consumers landed for:
  - `return`
  - `throw`
  - `break`
  - `continue`
  - `yield`
- class member heads now treat `static` and `async` contextually instead of as
  unconditional modifiers
- runtime parameter parsing now enforces the same basic rest rules as function
  types: one rest parameter maximum, and it must be last
- `yield*` now parses as delegated yield only when `*` stays on the same line as
  `yield`; newline-separated `yield` and `*` correctly stop at ASI instead

### `crates/raya-engine/src/parser/parser/expr.rs`

Rework postfix and call/member parsing:
- postfix `++/--` must respect line terminator boundaries
- generic-call speculative parsing must not interfere with ordinary relational parsing
- call/member/index/optional-chain parsing should share one suffix loop with explicit
  no-line-terminator checks where the spec requires them

Generator/class-element syntax work to land here:
- generator method heads
- async generator method heads
- class-element combinations with `static`, private names, decorators, and `*`
- `yield*`

Spread/rest syntax work to land here:
- spread in call argument lists
- spread in `new` argument lists
- tighter distinction between array spread and pattern rest contexts

Status now:
- postfix `++/--` no longer binds across a line terminator
- the precedence climber no longer loops forever when newline-separated postfix
  `++/--` remains in the token stream for the next parse step
- object methods now share one parsing path for ordinary, async, and generator
  heads, including TS-style type parameters and return annotations
- arrow parameter parsing now rejects duplicate rest parameters and non-final
  rest parameters instead of accepting them and drifting into later phases
- parenthesized destructuring vs grouped-expression parsing now goes through an
  explicit speculative arrow helper instead of the older eager `{` / `[` heuristic
- parenthesized object/array expressions now backtrack out of the arrow-parameter
  path instead of being eagerly misclassified as destructuring params
- call/new/async-call arguments now use a first-class `CallArgument` AST with
  explicit spread nodes instead of pretending every argument is a plain expression
- spread in call and `new` argument lists now parses through the shared argument
  path instead of remaining a parser gap

### `crates/raya-engine/src/parser/parser/pattern.rs`

Normalize rest/destructuring grammar so it is not partially duplicated:
- array rest elements
- object rest properties
- binding patterns vs assignment patterns
- destructuring in params, `for-of`, `catch`, and declarations

Reason:
- current test262 failures around `...` are spread across expression, statement, and
  class-method cases because the grammar is duplicated in too many places

Status now:
- object destructuring accepts string, numeric, and computed property keys
- pattern keys now share the object-literal `PropertyKey` model instead of a
  destructuring-only identifier shortcut
- object-pattern shorthand stays restricted to identifier keys, so `{ [k] }`
  still correctly requires an explicit `: value` binding form

### `crates/raya-engine/src/parser/parser/types.rs`

Audit type-argument speculation and TS-specific syntax boundaries:
- make sure `<...>` speculation backs off cleanly in expression contexts
- keep TS syntax from swallowing ECMAScript relational/operator forms

Reason:
- several parse failures are not “missing feature” bugs, but precedence/suffix confusion

### `crates/raya-engine/src/parser/lexer.rs`

Still needed:
- longer-term lexer/parser context cleanup after the first explicit trivia pass

Why:
- the explicit trivia bit is now in place, but parser context itself is still
  spread across several helpers/counters rather than one compact state object

## Recommended Implementation Order

Phase 1 completion order as landed:
1. finish ASI-sensitive statement rules
2. finish postfix/call/member no-line-terminator handling
3. unify class-element head parsing
4. add generator method and `yield*` grammar
5. unify spread/rest grammar across expr/pattern/params
6. replace span-based newline approximation with lexer-carried token trivia

## Acceptance Slices

Run after each subphase:
- `test/language/asi`
- generator-heavy subsets under `test/language/arguments-object`
- class-element syntax subsets under `test/language/expressions/class` and `test/language/statements/class`

Use:
- `scripts/test262_language_state.py`

## Immediate Next Code Changes

Phase 1 parser work is now complete enough to stop doing grammar-only cleanup and
move to the next structural layer:
- early-error legality and binder pass improvements
- arguments object / activation-record semantics
- descriptor kernel fidelity
- iterator/runtime protocol cleanup

## Phase 1 Notes

What Phase 1 now covers:
- ASI-sensitive statement parsing
- postfix/no-line-terminator handling
- generator and async-generator method heads
- delegated `yield*`
- rest-parameter legality
- grouped-vs-arrow ambiguity cleanup
- first-class spread arguments in call/new syntax
- object-pattern computed keys
- lexer-carried inter-token newline trivia

What is intentionally deferred beyond Phase 1:
- compact parser-context rearchitecture
- parser/checker diagnostic normalization for early syntax failures
- runtime semantics work needed for the broader Test262 language failures
