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
- first ASI helper landed:
  - `has_line_terminator_before_current`
  - `can_insert_semicolon_before_current`

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

### `crates/raya-engine/src/parser/parser/pattern.rs`

Normalize rest/destructuring grammar so it is not partially duplicated:
- array rest elements
- object rest properties
- binding patterns vs assignment patterns
- destructuring in params, `for-of`, `catch`, and declarations

Reason:
- current test262 failures around `...` are spread across expression, statement, and
  class-method cases because the grammar is duplicated in too many places

### `crates/raya-engine/src/parser/parser/types.rs`

Audit type-argument speculation and TS-specific syntax boundaries:
- make sure `<...>` speculation backs off cleanly in expression contexts
- keep TS syntax from swallowing ECMAScript relational/operator forms

Reason:
- several parse failures are not “missing feature” bugs, but precedence/suffix confusion

### `crates/raya-engine/src/parser/lexer.rs`

Still needed:
- carry explicit trivia metadata for tokens instead of relying on span-line approximation
- line terminator tracking should eventually be lexed, not inferred from token start lines

Why:
- the current helper is a good first cut for ASI, but multi-line tokens/comments need
  real inter-token trivia bits for full correctness

## Recommended Implementation Order

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

After the initial ASI patch, the next parser changes should be:
- support `function*`, generator methods, and class generator elements without falling
  into ordinary method-name parsing
- support spread in call/new argument lists where `DotDotDot` is still rejected
- audit labeled statement parsing around ASI edge cases in `test/language/asi`

## Phase 1 Notes

The current ASI/postfix implementation is intentionally a bridge:
- newline boundaries are still inferred from token spans rather than explicit lexer
  trivia
- that is good enough to unblock the first `asi` and postfix hang cluster
- the final Phase 1 target is still to move line terminator knowledge into lexer output
