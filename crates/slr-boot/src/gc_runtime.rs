//! Boot embedding for the original incremental collector and finalizer lists.
//! Native roots are explicit original anchors, never all native containers.

use slr_vm::{
    gc::Step,
    host::VmHost,
    sched::{CallbackRun, FinalizerQueues},
    vmclass::ClassRef,
};
use std::{cell::RefCell, path::Path};

#[derive(Default)]
struct State {
    config: Option<u32>,
    roots: Option<[u32; 2]>,
    events: Vec<serde_json::Value>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

/// Script object held by original native Config anchor63643c.
pub fn config_script() -> Option<u32> {
    STATE.with(|state| state.borrow().config)
}

///63643c is bound before GameInit's constructor. Native setting reads during
/// that constructor already observe Config, while complete collector startup
/// still requires GameInit's separate root. Starting a new boot clears roots
/// and receipts retained by the previous VM on this thread.
pub fn configure_config(config: u32) -> Result<(), String> {
    if config == 0 {
        return Err("GcNativeRootBoundary: Config script anchor must be bound".into());
    }
    STATE.with(|state| {
        *state.borrow_mut() = State {
            config: Some(config),
            roots: None,
            events: vec![],
        }
    });
    Ok(())
}

/// Configure the script words retained through Config (63643c) and GameInit
/// (63642c). The middle native anchor63646c is a separate unresolved input.
pub fn configure_roots(
    config: u32,
    game_init: u32,
    unknown_63646c: Option<u32>,
) -> Result<(), String> {
    if unknown_63646c.is_some_and(|word| word != 0) {
        return Err(
            "GcNativeRootBoundary: native anchor63646c requires an observed script binding".into(),
        );
    }
    if config == 0 || game_init == 0 {
        return Err("GcNativeRootBoundary: Config/GameInit script anchors must be bound".into());
    }
    STATE.with(|state| {
        *state.borrow_mut() = State {
            config: Some(config),
            roots: Some([config, game_init]),
            events: vec![],
        }
    });
    Ok(())
}

fn object(vm: &VmHost, word: u32) -> serde_json::Value {
    let class = word
        .checked_sub(0x1000)
        .and_then(|index| vm.objects.get(index as usize))
        .map(|object| match &object.class {
            ClassRef::Loaded(id) => vm.loader.class_name(*id).unwrap_or("?").to_owned(),
            ClassRef::Array(array) => array.descriptor().to_owned(),
        });
    serde_json::json!({"word":word, "allocation_ordinal":word.checked_sub(0xfff), "class":class})
}

fn record(event: serde_json::Value) {
    crate::telemetry::detail("gc", vec![("data".into(), event.to_string())]);
    STATE.with(|state| state.borrow_mut().events.push(event));
}

/// Remove strings revived by interning before they consume a drain slot.
pub fn retain_queued(vm: &VmHost, finals: &mut FinalizerQueues) {
    finals.retain_queued(|word| vm.gc.flags(*word).is_some_and(|flags| flags & 8 != 0));
}

/// One original callback opportunity. Start roots follow418110,418290 then
/// class-cache static cells. Active marks consume the Collector's250-object
/// budget; sweeping appends newly unreachable objects in allocation-list order.
pub fn step(vm: &mut VmHost, finals: &mut FinalizerQueues) -> Result<CallbackRun, String> {
    let mut roots = Vec::new();
    if !vm.gc.active() {
        let native = STATE
            .with(|state| state.borrow().roots)
            .ok_or("GcNativeRootBoundary: native roots were not configured")?;
        for word in native {
            if vm.gc.flags(word).is_none() {
                return Err(format!(
                    "GcNativeRootBoundary: unknown script anchor{word:#x}"
                ));
            }
            roots.push(word);
        }
        roots.extend(crate::sched::gc_thread_roots(vm)?);
        roots.extend(vm.gc_static_roots());
    }
    let phase = vm.gc_step(roots);
    let callback = match &phase {
        Step::Swept { queued } => {
            for word in queued {
                finals.push_queued(*word);
            }
            record(
                serde_json::json!({"operation":"sweep", "queued":queued.iter().map(|word| object(vm,*word)).collect::<Vec<_>>(),
                "allocation_counter":vm.gc.allocation_counter(), "mark_bytes":vm.gc.mark_bytes()}),
            );
            CallbackRun::Consumed
        }
        Step::Idle | Step::Started | Step::Marking => {
            record(
                serde_json::json!({"operation":format!("{phase:?}"), "pending":vm.gc.pending(), "allocation_counter":vm.gc.allocation_counter()}),
            );
            CallbackRun::Retained
        }
    };
    Ok(callback)
}

///408560 marks finalized before dispatch, runs the actual script finalize,
/// clears reference storage, then appends the record to the spare list.
pub fn finalize(vm: &mut VmHost, finals: &mut FinalizerQueues, word: u32) -> Result<(), String> {
    if !vm.gc.finalize(word) {
        return Ok(());
    }
    record(serde_json::json!({"operation":"finalize", "object":object(vm,word)}));
    crate::sched::run_finalizer(vm, word).map_err(|(thread, error)| {
        crate::script_stop(vm, &thread, &format!("GC finalize{word:#x}"), error)
    })?;
    vm.gc_clear_references(word);
    finals.push_spare(word);
    Ok(())
}

pub fn write_trace(output: &Path) -> Result<(), String> {
    STATE.with(|state| {
        std::fs::write(
            output.join("gc-runtime.json"),
            serde_json::to_vec_pretty(&state.borrow().events).unwrap(),
        )
        .map_err(|error| format!("GcTraceBoundary:{error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_lang::thread::with_threads;
    use slr_vm::host::{CallInfo, VmHost};
    use std::cell::RefCell;

    thread_local! { static CALLS: RefCell<Vec<(String,i32,u32)>> = const { RefCell::new(vec![]) }; }

    #[test]
    fn config_anchor_is_available_before_init_and_new_boot_clears_old_roots() {
        configure_roots(0x1001, 0x1002, None).unwrap();
        STATE.with(|state| {
            state
                .borrow_mut()
                .events
                .push(serde_json::json!({"previous":true}))
        });
        configure_config(0x2001).unwrap();
        assert_eq!(config_script(), Some(0x2001));
        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.roots, None);
            assert!(state.events.is_empty());
        });
        assert!(configure_config(0).is_err());
        assert_eq!(config_script(), Some(0x2001));
        configure_roots(0x2001, 0x2002, None).unwrap();
        STATE.with(|state| assert_eq!(state.borrow().roots, Some([0x2001, 0x2002])));
        assert_eq!(config_script(), Some(0x2001));
        STATE.with(|state| *state.borrow_mut() = State::default());
        assert_eq!(config_script(), None);
    }

    fn stock_file() -> (VmHost, u32) {
        let mut vm = VmHost::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock");
        for file in [
            "system/Scripts/lang/Object.class",
            "system/Scripts/lang/Native.class",
            "system/Scripts/io/File.class",
        ] {
            vm.loader
                .load_file_named(
                    &std::fs::read(root.join(file)).expect("unchanged stock class required"),
                )
                .unwrap();
        }
        slr_io::file::install(&mut vm);
        vm.native_thread_hook = Some(crate::sched::hook);
        vm.native_impls.insert(slr_io::file::FILE_CLOSE, close);
        let word = vm.alloc_object("java.io.File").unwrap();
        (vm, word)
    }

    fn close(_vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
        with_threads(|host| {
            let id = host
                .current()
                .expect("finalize native must have a calling thread");
            let state = host.queues().get(id);
            CALLS.with(|calls| {
                calls
                    .borrow_mut()
                    .push((state.name.clone(), state.priority, state.flags))
            });
            // A blocked finalizer still has its immediate original destructor.
            host.wait(0x1234).unwrap();
        });
        None
    }

    #[test]
    fn stock_finalize_runs_on_scoped_thread_and_cannot_leave_a_monitor_waiter() {
        slr_lang::thread::reset();
        CALLS.with(|calls| calls.borrow_mut().clear());
        let (mut vm, word) = stock_file();
        let before = with_threads(|host| host.queues().run_order());
        let mut finals = FinalizerQueues::new();
        finalize(&mut vm, &mut finals, word).unwrap();
        CALLS.with(|calls| {
            let calls = calls.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "THRD-FIN");
            assert_eq!(calls[0].1, 10);
            assert_ne!(calls[0].2 & 1, 0);
        });
        with_threads(|host| {
            assert_eq!(host.queues().run_order(), before);
            assert!(host.queues().notify_order().is_empty());
            assert!(host.waiters(0x1234).is_empty());
            assert_eq!(host.current(), None);
        });
        assert_eq!(finals.spare(), vec![word]);
        finalize(&mut vm, &mut finals, word).unwrap();
        CALLS.with(|calls| assert_eq!(calls.borrow().len(), 1));
    }

    #[test]
    fn explicit_native_roots_precede_sweep_and_revival_removes_queued_strings() {
        slr_lang::thread::reset();
        let mut vm = VmHost::new();
        let config = vm.alloc_array("I", 0).unwrap();
        let init = vm.alloc_array("I", 0).unwrap();
        let garbage = vm.alloc_array("I", 0).unwrap();
        for _ in 0..1000 {
            vm.alloc_array("I", 0).unwrap();
        }
        configure_roots(config, init, None).unwrap();
        let mut finals = FinalizerQueues::new();
        assert_eq!(step(&mut vm, &mut finals).unwrap(), CallbackRun::Retained);
        assert_eq!(step(&mut vm, &mut finals).unwrap(), CallbackRun::Consumed);
        assert!(!finals.queued().contains(&config));
        assert!(!finals.queued().contains(&init));
        assert_eq!(finals.queued()[0], garbage);
        vm.gc.revive(garbage);
        retain_queued(&vm, &mut finals);
        assert!(!finals.queued().contains(&garbage));
        assert!(configure_roots(config, init, Some(123)).is_err());
    }
}
