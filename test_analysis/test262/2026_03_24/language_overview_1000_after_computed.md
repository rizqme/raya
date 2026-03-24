# Test262 Language State

- selector: `test/language`
- discovered: `1000`
- recorded: `1000`
- passed: `791`
- failed: `69`
- skipped: `140`
- timed out: `0`
- infrastructure errors: `0`
- missing: `0`
- elapsed seconds: `156.42`

## Early Overview

What looks healthy:
- The runner is resilient enough to keep marching through failing chunks instead of aborting on the first red case.
- A non-trivial slice of the selected language corpus already passes unchanged, so the failures are clustered rather than uniformly broken.
- Unsupported async/module/host-hook cases are being classified as skips instead of hanging the run.

What is failing first:
- `test/language/block-scope`: `29` failures
- `test/language/directive-prologue`: `22` failures
- `test/language/comments`: `9` failures
- `test/language/asi`: `5` failures
- `test/language/destructuring`: `4` failures

Dominant failure clusters:
- `expected compilation to fail`: `27` cases
- `runtime-failure`: `18` cases
- `parser-unsupported-syntax`: `12` cases
- `compile-failure`: `5` cases
- `unexpected compilation error`: `4` cases
- `throw-behavior`: `3` cases

## Sample Failed Cases

- `test/language/asi/S7.9.2_A1_T1.js`: expected compilation to fail
- `test/language/asi/S7.9.2_A1_T4.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: #1: Automatic semicolon insertion not work with return
- `test/language/asi/S7.9_A10_T8.js`: expected compilation to fail
- `test/language/asi/S7.9_A11_T4.js`: expected compilation to fail
- `test/language/asi/S7.9_A3.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: #2: Check return statement for automatic semicolon insertion
- `test/language/block-scope/leave/x-after-break-to-label.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected assertion to be truthy
- `test/language/block-scope/shadowing/catch-parameter-shadowing-catch-parameter.js`: compilation failed: Type error: <inline>.raya: ConstReassignment: cannot assign to const variable 'c'
- `test/language/block-scope/shadowing/catch-parameter-shadowing-function-parameter-name.js`: compilation failed: Type error: <inline>.raya: ConstReassignment: cannot assign to const variable 'a'
- `test/language/block-scope/shadowing/catch-parameter-shadowing-let-declaration.js`: compilation failed: Type error: <inline>.raya: ConstReassignment: cannot assign to const variable 'a'
- `test/language/block-scope/shadowing/catch-parameter-shadowing-var-variable.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected SameValue
- `test/language/block-scope/shadowing/const-declaration-shadowing-catch-parameter.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected SameValue
- `test/language/block-scope/shadowing/const-declarations-shadowing-parameter-name-let-const-and-var-variables.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected SameValue
- `test/language/block-scope/shadowing/let-declarations-shadowing-parameter-name-let-const-and-var.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected SameValue
- `test/language/block-scope/shadowing/parameter-name-shadowing-catch-parameter.js`: compilation failed: Type error: <inline>.raya: ConstReassignment: cannot assign to const variable 'c'
- `test/language/block-scope/shadowing/parameter-name-shadowing-parameter-name-let-const-and-var.js`: compilation failed: Type error: <inline>.raya: ConstReassignment: cannot assign to const variable 'd'
- `test/language/block-scope/syntax/function-declarations/in-statement-position-do-statement-while-expression.js`: expected compilation to fail
- `test/language/block-scope/syntax/function-declarations/in-statement-position-for-statement.js`: expected compilation to fail
- `test/language/block-scope/syntax/function-declarations/in-statement-position-if-expression-statement-else-statement.js`: expected compilation to fail
- `test/language/block-scope/syntax/function-declarations/in-statement-position-if-expression-statement.js`: expected compilation to fail
- `test/language/block-scope/syntax/function-declarations/in-statement-position-while-expression-statement.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-async-function.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-async-generator.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-class.js`: unexpected compilation error: Compile error: Internal compiler error: Internal compiler error: class declaration 'f' at span 574 missing NominalTypeId registration
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-const.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-function.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-generator.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/inner-block-var-name-redeclaration-attempt-with-let.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-async-function.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-async-generator.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-class.js`: unexpected compilation error: Compile error: Internal compiler error: Internal compiler error: class declaration 'f' at span 558 missing NominalTypeId registration
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-const.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-function.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-generator.js`: expected compilation to fail
- `test/language/block-scope/syntax/redeclaration/var-name-redeclaration-attempt-with-let.js`: expected compilation to fail
- `test/language/comments/S7.4_A2_T2.js`: expected compilation to fail
- `test/language/comments/S7.4_A5.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: #0000 
- `test/language/comments/S7.4_A6.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: #0000 throws
- `test/language/comments/hashbang/eval-indirect.js`: runtime failed: Runtime error: Runtime error: Main task failed: SyntaxError: Dynamic eval lexer error: [UnexpectedCharacter { char: '#', span: Span { start: 0, end: 1, line: 1, column: 1 } }]
- `test/language/comments/hashbang/eval.js`: runtime failed: Runtime error: Runtime error: Main task failed: SyntaxError: Dynamic eval lexer error: [UnexpectedCharacter { char: '#', span: Span { start: 0, end: 1, line: 1, column: 1 } }]
- `test/language/comments/hashbang/function-body.js`: unexpected compilation error: Lexer error: /private/var/folders/jr/hqhj_b8x7s13r08cg8wrhn_w0000gn/T/raya-es262-91031-test_language_comments_hashbang_function_body_js.js: [UnexpectedCharacter { char: '#', span: Span { start: 505, end: 506, line: 26, column: 16 } }]
- `test/language/comments/hashbang/function-constructor.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Function Call argument
- `test/language/comments/hashbang/no-line-separator.js`: runtime failed: Runtime error: Runtime error: Main task failed: SyntaxError: Dynamic eval lexer error: [UnexpectedCharacter { char: '#', span: Span { start: 0, end: 1, line: 1, column: 1 } }]
- `test/language/comments/hashbang/statement-block.js`: unexpected compilation error: Lexer error: /private/var/folders/jr/hqhj_b8x7s13r08cg8wrhn_w0000gn/T/raya-es262-91031-test_language_comments_hashbang_statement_block_js.js: [UnexpectedCharacter { char: '#', span: Span { start: 494, end: 495, line: 27, column: 3 } }]
- `test/language/destructuring/binding/initialization-requires-object-coercible-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected function to throw
- `test/language/destructuring/binding/initialization-requires-object-coercible-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected function to throw
- `test/language/destructuring/binding/keyed-destructuring-property-reference-target-evaluation-order-with-bindings.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Expected arrays to compare equal
- `test/language/destructuring/binding/typedarray-backed-by-resizable-buffer.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Array.concat expects 1 argument, got 2
- `test/language/directive-prologue/10.1.1-1-s.js`: compilation failed: Parse error: <inline>.raya: [ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), LeftBracket, LeftBrace, DotDotDot], found: Public }, span: Span { start: 1184, end: 1190, line: 58, column: 13 }, message: "Unexpected token Public, expected one of: [Identifier(Symbol(1)), LeftBracket, LeftBrace, DotDotDot]", suggestion: None }, ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), IntLiteral(0), FloatLiteral(0.0), StringLiteral(Symbol(1)), True, False, Null, LeftParen, LeftBracket, LeftBrace], found: Public }, span: Span { start: 1224, end: 1230, line: 60, column: 28 }, message: "Unexpected token Public", suggestion: None }, ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), IntLiteral(0), FloatLiteral(0.0), StringLiteral(Symbol(1)), True, False, Null, LeftParen, LeftBracket, LeftBrace], found: RightBrace }, span: Span { start: 1240, end: 1241, line: 61, column: 5 }, message: "Unexpected token RightBrace", suggestion: None }]
- `test/language/directive-prologue/10.1.1-10-s.js`: compilation failed: Parse error: <inline>.raya: [ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), LeftBracket, LeftBrace, DotDotDot], found: Public }, span: Span { start: 1183, end: 1189, line: 58, column: 13 }, message: "Unexpected token Public, expected one of: [Identifier(Symbol(1)), LeftBracket, LeftBrace, DotDotDot]", suggestion: None }, ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), IntLiteral(0), FloatLiteral(0.0), StringLiteral(Symbol(1)), True, False, Null, LeftParen, LeftBracket, LeftBrace], found: Public }, span: Span { start: 1223, end: 1229, line: 60, column: 28 }, message: "Unexpected token Public", suggestion: None }, ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), IntLiteral(0), FloatLiteral(0.0), StringLiteral(Symbol(1)), True, False, Null, LeftParen, LeftBracket, LeftBrace], found: RightBrace }, span: Span { start: 1239, end: 1240, line: 61, column: 5 }, message: "Unexpected token RightBrace", suggestion: None }]
- `test/language/directive-prologue/10.1.1-11-s.js`: compilation failed: Parse error: <inline>.raya: [ParseError { kind: UnexpectedToken { expected: [Identifier(Symbol(1)), IntLiteral(0), FloatLiteral(0.0), StringLiteral(Symbol(1)), True, False, Null, LeftParen, LeftBracket, LeftBrace], found: Public }, span: Span { start: 1608, end: 1614, line: 77, column: 27 }, message: "Unexpected token Public", suggestion: None }]
