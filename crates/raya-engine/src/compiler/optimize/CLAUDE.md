# optimize module

IR optimization passes for the Raya compiler.

## Module Structure

```
optimize/
├── mod.rs           # Optimizer struct, pass orchestration
├── constant_fold.rs # Constant folding
├── dce.rs           # Dead code elimination
├── inline.rs        # Function inlining
└── phi_elim.rs      # PHI node elimination
```

## Optimizer

```rust
pub struct Optimizer {
    passes: Vec<Box<dyn Pass>>,
}

impl Optimizer {
    // Standard optimization level
    pub fn basic() -> Self {
        Self {
            passes: vec![
                Box::new(ConstantFold),
                Box::new(DeadCodeElimination),
            ],
        }
    }

    // Aggressive optimizations
    pub fn aggressive() -> Self {
        Self {
            passes: vec![
                Box::new(ConstantFold),
                Box::new(Inline::new(threshold: 50)),
                Box::new(DeadCodeElimination),
                Box::new(ConstantFold), // Re-run after inlining
            ],
        }
    }

    pub fn optimize(&self, module: &mut IrModule) {
        for pass in &self.passes {
            pass.run(module);
        }
    }
}
```

## Optimization Passes

### Constant Folding (`constant_fold.rs`)

Evaluates constant expressions at compile time.

```
// Before
r0 = 2
r1 = 3
r2 = Add r0, r1

// After
r2 = 5
```

Handles:
- Arithmetic: `+`, `-`, `*`, `/`, `%`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical: `&&`, `||`, `!`
- String concatenation (limited)

### Dead Code Elimination (`dce.rs`)

Removes unreachable code and unused assignments.

```
// Before
r0 = 42        // unused
r1 = 10
Return r1

// After
r1 = 10
Return r1
```

Handles:
- Unreachable blocks (after unconditional jump/return)
- Unused variable assignments
- Branches with constant conditions

### Function Inlining (`inline.rs`)

Replaces function calls with the function body.

```
// Before
function add(a, b) { return a + b; }
r0 = Call add, 1, 2

// After (inlined)
r0 = Add 1, 2
```

Inlining heuristics:
- Small functions (< threshold instructions)
- Non-recursive functions
- Called once (always inline)

### PHI Elimination (`phi_elim.rs`)

Converts PHI nodes to explicit copies for non-SSA targets.

```
// SSA form
entry:
    r0 = 1
    Jump merge
other:
    r1 = 2
    Jump merge
merge:
    r2 = Phi(entry: r0, other: r1)

// After phi elimination
entry:
    r0 = 1
    r2 = r0  // Copy
    Jump merge
other:
    r1 = 2
    r2 = r1  // Copy
    Jump merge
merge:
    // r2 now has the right value
```

## Pass Trait

```rust
pub trait Pass {
    fn name(&self) -> &str;
    fn run(&self, module: &mut IrModule);
}
```

## For AI Assistants

- Passes are run in order, some benefit from multiple runs
- Constant folding is the most impactful optimization
- Inlining can expose more constant folding opportunities
- DCE should run after other passes clean up dead code
- PHI elimination is needed because bytecode isn't SSA
- All passes must preserve program semantics
