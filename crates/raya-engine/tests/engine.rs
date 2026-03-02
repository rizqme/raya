//! raya-engine integration tests — single binary entry point.
//!
//! Each submodule corresponds to a former top-level test file.
//! Build variants:
//!   cargo test -p raya-engine                      — base tests
//!   cargo test -p raya-engine --features jit       — + JIT tests
//!   cargo test -p raya-engine --features aot       — + AOT tests

#![allow(
    unused_variables,
    unused_imports,
    clippy::identity_op,
    clippy::unnecessary_cast
)]

// ── feature-gated suites ─────────────────────────────────────────────────────
#[cfg(feature = "aot")]
mod aot_integration;
#[cfg(feature = "jit")]
mod jit_integration;

// ── base suites ──────────────────────────────────────────────────────────────
mod ast_tests;
mod async_call_tests;
mod basic_codegen;
mod closure_tests;
mod codegen_tests;
mod concurrency_integration;
mod concurrent_task_tests;
mod context_isolation_tests;
mod decorator_test;
mod exception_handling_basic;
mod expression_tests;
mod gc_integration_tests;
mod gc_stress_tests;
mod hardening_test;
mod import_resolution;
mod inner_vm_integration;
mod interpreter_integration;
mod ir_comprehensive;
mod ir_demo;
mod json_integration;
mod jsx_lowering_tests;
mod jsx_tests;
mod milestone_2_9_test;
mod module_linking;
mod module_loading;
mod monomorphize_tests;
mod object_model_tests;
mod opcode_tests;
mod pattern_tests;
mod reflect_phase8_tests;
mod rest_pattern_test;
mod safepoint_integration;
mod scheduler_integration;
mod snapshot_integration;
mod snapshot_jit;
mod snapshot_restore_validation;
mod stack_integration;
mod statement_tests;
mod template_test;
mod tokens;
mod type_tests;
mod visibility_test;
mod visitor_tests;
mod vm_context_integration;
