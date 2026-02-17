---
layout: home

hero:
  name: Raya
  text: A Strict, Concurrent TypeScript Subset
  tagline: Statically-typed language compiled to custom bytecode. Multi-threaded VM with goroutine-style concurrency. Zero runtime type checks.
  actions:
    - theme: brand
      text: Get Started
      link: /overview
    - theme: alt
      text: Language Spec
      link: /language/lang
    - theme: alt
      text: GitHub
      link: https://github.com/rizqme/raya

features:
  - title: Fully Static Type System
    details: All types verified at compile time. No any, no runtime type checks, no escape hatches.
  - title: Goroutine-Style Concurrency
    details: Lightweight green threads (Tasks) with automatic work-stealing across CPU cores.
  - title: TypeScript Syntax
    details: Familiar syntax for millions of developers. Every valid Raya program is valid TypeScript.
  - title: Monomorphization
    details: Generic code specialized per concrete type at compile time, like Rust and C++.
  - title: Typed Bytecode
    details: Type-aware instructions (IADD, FADD, NADD) enable unboxed operations with zero overhead.
  - title: Sound Type System
    details: Discriminated unions, exhaustiveness checking, no type assertions. Safety over convenience.
---
