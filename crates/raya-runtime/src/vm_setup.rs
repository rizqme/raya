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
        max_heap_size: if options.heap_limit > 0 {
            Some(options.heap_limit)
        } else {
            None
        },
        ..Default::default()
    };

    let mut vm = Vm::with_native_handler(threads, Arc::new(StdNativeHandler));

    // Apply scheduler limits if any were set
    if options.heap_limit > 0 {
        // Recreate with limits
        vm = create_vm_with_limits(threads, limits);
    }

    // Register symbolic native functions for ModuleNativeCall dispatch
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    vm
}

fn create_vm_with_limits(threads: usize, limits: SchedulerLimits) -> Vm {
    // When we have limits, we need to use the scheduler_limits constructor
    // and also set the native handler. Since with_scheduler_limits doesn't
    // take a native handler, we use with_native_handler then apply limits
    // through the scheduler. For now, use with_native_handler which is the
    // primary path.
    let vm = Vm::with_native_handler(threads, Arc::new(StdNativeHandler));
    // TODO: Apply SchedulerLimits when Vm API supports both limits + handler
    let _ = limits;
    vm
}
