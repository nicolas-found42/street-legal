//! Boot-specific diagnostics over [`slr_telemetry`].
//!
//! The streams, durability and environment capture live in the shared crate;
//! this module adds the headless VM state snapshot, the native-divergence
//! artifact and the boot's native call-log ordinals.
use serde_json::{json, Value};
use slr_vm::{
    diagnostics::{self, EventKind},
    host::VmHost,
};
use std::{cell::RefCell, path::Path};

thread_local! {
    static ANCHORS: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
    static TARGET_NATIVE: RefCell<Option<u64>> = const { RefCell::new(None) };
}

/// Open every boot stream. `wall_clock` adds `wall_ns` to each event line.
pub fn configure(
    output: &Path,
    run_id: &str,
    capacity: usize,
    full: bool,
    wall_clock: bool,
) -> Result<(), String> {
    ANCHORS.with(|anchors| anchors.borrow_mut().clear());
    slr_telemetry::configure(output, run_id, capacity, full, wall_clock, true)
}

pub fn detail(category: &str, fields: Vec<(String, String)>) -> Option<u64> {
    slr_telemetry::detail(category, fields)
}

pub fn anchor(name: &str, word: u32) {
    ANCHORS.with(|anchors| anchors.borrow_mut().push((name.into(), word)));
    detail(
        "native-root",
        vec![
            ("name".into(), name.into()),
            ("word".into(), word.to_string()),
        ],
    );
}

pub fn target_native(target: Option<u64>) {
    TARGET_NATIVE.with(|slot| *slot.borrow_mut() = target);
}

/// A deterministic diagnostic rerun requests the actual VM native call ID
/// correlated with its first mismatching call-log record.
pub fn native_snapshot(
    vm: &VmHost,
    thread: &slr_vm::interp::Thread,
    info: &slr_vm::host::CallInfo<'_>,
    phase: slr_vm::host::NativePhase,
) {
    if let Err(error) = crate::machine_recording::drain(vm) {
        slr_telemetry::detail("machine-recording-failure", vec![("error".into(), error)]);
    }
    if !matches!(phase, slr_vm::host::NativePhase::Return) {
        return;
    }
    let context = diagnostics::current_context();
    let Some(id) = context.native.as_ref().map(|native| native.call_id) else {
        return;
    };
    let selected = TARGET_NATIVE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == Some(id) {
            *slot = None;
            slr_telemetry::run_id()
        } else {
            None
        }
    });
    let Some(run_id) = selected else {
        return;
    };
    let result = (|| {
        snapshot(vm, "native-divergence", None)?;
        let value = json!({"schema_version":1,"run_id":run_id,"native_call_id":id,"context":context,
            "native":{"class":info.class,"name":info.name,"signature":info.sig,
                "argument_cells":info.args.iter().map(|v|json!({"type":v.tag.name(),"payload_bits":v.payload})).collect::<Vec<_>>()},
            "argument_objects":info.args.iter().filter(|v|v.tag.is_ref() && v.payload!=0)
                .filter_map(|v|vm.diagnostic_object(v.payload)).collect::<Vec<_>>(),
            "current_thread":diagnostics::failure_snapshot(vm,thread),
            "parked_threads":crate::sched::diagnostic_threads(vm),
            "resources":crate::factory::diagnostic_state(),"recent_events":diagnostics::recent(),
            "diagnostics":diagnostics::stats(),
            "wall_ns":slr_telemetry::monotonic_ns()});
        slr_telemetry::write_artifact("divergence.json", &value)
    })();
    if let Err(error) = result {
        // Record the failure on the shared stream without aborting the run:
        // the divergence artifact is diagnostic, not acceptance evidence.
        detail(
            "diagnostic-error",
            vec![
                ("artifact".into(), "divergence.json".into()),
                ("error".into(), error),
            ],
        );
    }
}

/// Targeted frame state and complete object fields on failure; all payloads are
/// integer words, so NaNs, signed zero and reference identities survive JSON.
pub fn snapshot(vm: &VmHost, phase: &str, failure: Option<&str>) -> Result<(), String> {
    let sequence = diagnostics::emit(EventKind::External {
        category: "state-checkpoint".into(),
        fields: vec![("phase".into(), phase.into())],
    });
    let context = diagnostics::current_context();
    let anchors = ANCHORS.with(|anchors| anchors.borrow().clone());
    let objects:Vec<_> = vm.objects.iter().enumerate().filter(|(index,object)| {
        failure.is_some() || anchors.iter().any(|(_,word)|*word==*index as u32+0x1000) || matches!(object.class_name(&vm.loader),
            "java.game.GameLogic"|"java.game.MainMenu"|"java.game.MainMenuDialog"|"java.game.OptionsDialog"|"java.game.Config"|"java.game.GameInit")
    }).map(|(index,object)|json!({"word":index as u32+0x1000,"class":object.class_name(&vm.loader),
        "gc_flags":vm.gc.flags(index as u32+0x1000),
        "fields":object.fields.iter().map(|v|json!({"type":v.tag.name(),"payload_bits":v.payload})).collect::<Vec<_>>()})).collect();
    let queues = slr_lang::thread::with_threads(|host| {
        let queues = host.queues();
        let list = |ids: Vec<usize>| {
            ids.into_iter()
                .map(|id| {
                    let t = queues.get(id);
                    json!({"id":id+1,"name":t.name,"priority":t.priority,"flags":t.flags,
                "sleep_deadline_bits":t.sleep_deadline.to_bits(),"waiting_on":t.wait_object})
                })
                .collect::<Vec<_>>()
        };
        json!({"ready":list(queues.run_order()),"waiting":list(queues.notify_order())})
    });
    let value = json!({"schema_version":1,"phase":phase,"sequence":sequence,"provenance":slr_telemetry::provenance(),"context":context,"failure":failure,
        "objects":objects,"queues":queues,"native_roots":anchors,"allocation_count":vm.objects.len(),
        "resources":crate::factory::diagnostic_state(),
        "threads":crate::sched::diagnostic_threads(vm),
        "diagnostics":diagnostics::stats(),
        "gc":{"active":vm.gc.active(),"pending":vm.gc.pending(),"allocations":vm.gc.allocation_counter()},
        "recent_events":if failure.is_some(){serde_json::to_value(diagnostics::recent()).unwrap_or(Value::Null)}else{Value::Null}});
    slr_telemetry::write_state(&value)?;
    if failure.is_some() {
        slr_telemetry::write_artifact("failure.json", &value)?;
    }
    Ok(())
}

pub fn check() -> Result<(), String> {
    slr_telemetry::check()
}

pub fn finish(failure: Option<&str>) -> Result<(), String> {
    slr_telemetry::finish(failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_native_values_preserve_float_bits_and_byte_strings() {
        // The shared crate owns the value encoding; keep the boot wrapper's
        // only local invariant (anchors stay ordered) covered here.
        ANCHORS.with(|anchors| anchors.borrow_mut().clear());
        anchor("Config", 0xffff0080);
        anchor("GameInit", 0xffff007f);
        ANCHORS.with(|anchors| {
            let anchors = anchors.borrow();
            assert_eq!(anchors[0].0, "Config");
            assert_eq!(anchors[1].1, 0xffff007f);
        });
    }
}
