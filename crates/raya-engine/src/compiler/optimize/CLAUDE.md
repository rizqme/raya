# Compiler Optimize

This folder runs cleanup and transformation passes over IR before codegen. It is about making valid IR smaller, simpler, or more emit-friendly without changing program meaning.

## What This Folder Owns

- Constant folding.
- Dead code elimination.
- Optional inlining.
- PHI elimination required before bytecode emission.
- Pass ordering and optimization-level policy.

## File Guide

- `constant_fold.rs`: replaces computable expressions with constants.
- `dce.rs`: removes unused instructions and unreachable paths.
- `inline.rs`: inlines selected call sites in more aggressive modes.
- `phi_elim.rs`: removes PHI nodes so codegen can emit linear bytecode.
- `mod.rs`: optimization levels and pass sequencing.

## Start Here When

- IR is semantically correct but too noisy or inefficient.
- A pass introduces or fixes semantic regressions.
- Codegen cannot handle an IR construct that should have been normalized away.

## Read Next

- IR source and structure: [`../ir/CLAUDE.md`](../ir/CLAUDE.md)
- Bytecode emission assumptions: [`../codegen/CLAUDE.md`](../codegen/CLAUDE.md)

## Things To Watch

- Pass ordering matters.
- Conservative and correct beats aggressive and fragile.
- PHI elimination is not an optional optimization; it is part of making IR codegen-ready.
