//! VM creation and configuration.

use raya_engine::vm::scheduler::SchedulerLimits;
use raya_engine::vm::Vm;
use raya_stdlib::StdNativeHandler;
use std::sync::Arc;

use crate::RuntimeOptions;

/// Create a fully-configured VM with stdlib support.
pub fn create_vm(options: &RuntimeOptions) -> Vm {
    let threads = if options.threads == 0 {
        num_cpus::get()
    } else {
        options.threads
    };

    let limits = SchedulerLimits {
        max_preemptions: options
            .max_preemptions
            .unwrap_or(raya_engine::vm::defaults::DEFAULT_MAX_PREEMPTIONS),
        preempt_threshold_ms: options
            .preempt_threshold_ms
            .unwrap_or(raya_engine::vm::defaults::DEFAULT_PREEMPT_THRESHOLD_MS),
        max_heap_size: if options.heap_limit > 0 {
            Some(options.heap_limit)
        } else {
            None
        },
        ..Default::default()
    };
    let use_custom_limits = options.heap_limit > 0
        || options.max_preemptions.is_some()
        || options.preempt_threshold_ms.is_some();

    let mut vm = if use_custom_limits {
        create_vm_with_limits(threads, limits)
    } else {
        Vm::with_native_handler(threads, Arc::new(StdNativeHandler))
    };

    // Register symbolic native functions for registered kernel-call dispatch.
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    vm
}

fn create_vm_with_limits(threads: usize, limits: SchedulerLimits) -> Vm {
    Vm::with_scheduler_limits_and_native_handler(threads, limits, Arc::new(StdNativeHandler))
}
