//! Process-crash-surviving diagnostics. Writers never buffer userspace history.
//! A completed marker is written only after all streams have been synchronized.
use serde_json::{json, Value};
use slr_calllog::{LogRecord, LogWriter};
use slr_vm::{
    diagnostics::{self, Event, EventKind},
    host::VmHost,
};
use std::{
    cell::RefCell,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

struct State {
    run_id: String,
    output: PathBuf,
    events: File,
    states: File,
    filtered: LogWriter<File>,
    all: LogWriter<File>,
    all_ordinal: u64,
    filtered_ordinal: u64,
    full: bool,
    skipped_steps: u64,
    error: Option<String>,
    anchors: Vec<(String, u32)>,
    target_native: Option<u64>,
    events_written: u64,
    states_written: u64,
    last_sequence: Option<u64>,
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

fn create(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("DiagnosticBoundary: {}: {error}", path.display()))
}
fn line(file: &mut File, value: &impl serde::Serialize) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    file.write_all(&bytes).map_err(|error| error.to_string())
}

pub fn configure(output: &Path, run_id: &str, capacity: usize, full: bool) -> Result<(), String> {
    let state = State {
        run_id: run_id.into(),
        output: output.to_owned(),
        events: create(&output.join("events.jsonl"))?,
        states: create(&output.join("states.jsonl"))?,
        filtered: LogWriter::create(create(&output.join("runtime.calllog"))?)
            .map_err(|e| e.to_string())?,
        all: LogWriter::create(create(&output.join("runtime-all.calllog"))?)
            .map_err(|e| e.to_string())?,
        all_ordinal: 0,
        filtered_ordinal: 0,
        full,
        skipped_steps: 0,
        error: None,
        anchors: vec![],
        target_native: None,
        events_written: 0,
        states_written: 0,
        last_sequence: None,
    };
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    diagnostics::configure(run_id.into(), capacity, Some(event));
    slr_calllog::headless::configure_stream(Some(native), false);
    let mut marker = create(&output.join("capture.json"))?;
    line(
        &mut marker,
        &json!({"schema_version":1,"run_id":run_id,"status":"running",
        "trace_level":if full {"full"} else {"metadata"},"event_ring_capacity":capacity}),
    )?;
    marker.sync_all().map_err(|e| e.to_string())?;
    detail("capture-start", vec![("run_id".into(), run_id.into())]);
    Ok(())
}

pub fn detail(category: &str, fields: Vec<(String, String)>) {
    diagnostics::emit(EventKind::External {
        category: category.into(),
        fields,
    });
}

pub fn anchor(name: &str, word: u32) {
    STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.anchors.push((name.into(), word));
        }
    });
    detail(
        "native-root",
        vec![
            ("name".into(), name.into()),
            ("word".into(), word.to_string()),
        ],
    );
}

pub fn target_native(target: Option<u64>) {
    STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.target_native = target;
        }
    });
}

/// A deterministic diagnostic rerun requests the actual VM native call ID
/// correlated with its first mismatching call-log record.
pub fn native_snapshot(
    vm: &VmHost,
    thread: &slr_vm::interp::Thread,
    info: &slr_vm::host::CallInfo<'_>,
    phase: slr_vm::host::NativePhase,
) {
    if !matches!(phase, slr_vm::host::NativePhase::Return) {
        return;
    }
    let context = diagnostics::current_context();
    let Some(id) = context.native.as_ref().map(|native| native.call_id) else {
        return;
    };
    let selected = STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut()?;
        if state.target_native == Some(id) {
            state.target_native = None;
            Some(state.run_id.clone())
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
            "diagnostics":diagnostics::stats()});
        STATE.with(|slot| {
            let slot = slot.borrow();
            let state = slot.as_ref().ok_or("diagnostics not configured")?;
            let mut file = create(&state.output.join("divergence.json"))?;
            line(&mut file, &value)?;
            file.sync_all().map_err(|e| e.to_string())
        })
    })();
    if let Err(error) = result {
        STATE.with(|slot| {
            if let Some(state) = slot.borrow_mut().as_mut() {
                state.error.get_or_insert(error);
            }
        });
    }
}

fn event(event: &Event) {
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if !state.full && matches!(event.kind, EventKind::Step { .. }) {
            state.skipped_steps += 1;
            return;
        }
        let result = line(&mut state.events, event);
        if let Err(error) = result {
            state.error.get_or_insert(error);
        } else {
            state.events_written += 1;
            state.last_sequence = Some(event.sequence);
        }
    });
}

fn filtered(record: &LogRecord) -> bool {
    !matches!(record,LogRecord::NativeCall{class,..} if class.starts_with("java.render.")
        || class.starts_with("java.sound.") || ["java.io.Input","java.io.MouseCursor","java.io.Controller"].contains(&class.as_str()))
}

fn recorded_value(value: &slr_calllog::Value) -> Value {
    use slr_calllog::Value as V;
    match value {
        V::Int(value) => json!({"type":"int","value":value}),
        V::Long(value) => json!({"type":"long","value":value}),
        V::FloatBits(bits) => json!({"type":"float","bits":bits}),
        V::DoubleBits(bits) => json!({"type":"double","bits":bits}),
        V::Str(value) => json!({"type":"string","value":value}),
        V::Bytes(value) => json!({"type":"bytes","value":value}),
        V::Bool(value) => json!({"type":"bool","value":value}),
    }
}

fn native(record: &LogRecord) {
    let ordinals = STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut()?;
        let all_ordinal = state.all_ordinal;
        let selected = filtered(record);
        let filtered_ordinal = selected.then_some(state.filtered_ordinal);
        let result = state
            .all
            .record(record)
            .and_then(|()| state.all.flush())
            .and_then(|()| {
                if selected {
                    state
                        .filtered
                        .record(record)
                        .and_then(|()| state.filtered.flush())
                } else {
                    Ok(())
                }
            });
        if let Err(error) = result {
            state.error.get_or_insert(error.to_string());
        }
        state.all_ordinal += 1;
        if selected {
            state.filtered_ordinal += 1;
        }
        Some((all_ordinal, filtered_ordinal))
    });
    if let Some((all, filtered)) = ordinals {
        let native_record = match record {
            LogRecord::NativeCall {
                class,
                name,
                args,
                result,
            } => json!({"class":class,"name":name,
                "arguments":args.iter().map(recorded_value).collect::<Vec<_>>(),
                "result":result.as_ref().map(recorded_value)}),
            _ => json!({"record_kind":record.kind_name()}),
        };
        detail(
            "native-call",
            vec![
                ("all_ordinal".into(), all.to_string()),
                (
                    "filtered_ordinal".into(),
                    filtered.map_or_else(String::new, |v| v.to_string()),
                ),
                ("native_record".into(), native_record.to_string()),
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
    let anchors = STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or_else(Vec::new, |state| state.anchors.clone())
    });
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
    let provenance=STATE.with(|slot|slot.borrow().as_ref().map(|s|json!({"run_id":s.run_id,"all_records":s.all_ordinal,"filtered_records":s.filtered_ordinal})));
    let value = json!({"schema_version":1,"phase":phase,"sequence":sequence,"provenance":provenance,"context":context,"failure":failure,
        "objects":objects,"queues":queues,"native_roots":anchors,"allocation_count":vm.objects.len(),
        "resources":crate::factory::diagnostic_state(),
        "threads":crate::sched::diagnostic_threads(vm),
        "diagnostics":diagnostics::stats(),
        "gc":{"active":vm.gc.active(),"pending":vm.gc.pending(),"allocations":vm.gc.allocation_counter()},
        "recent_events":if failure.is_some(){serde_json::to_value(diagnostics::recent()).unwrap_or(Value::Null)}else{Value::Null}});
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return Ok(());
        };
        line(&mut state.states, &value)?;
        state.states_written += 1;
        if failure.is_some() {
            let mut file = create(&state.output.join("failure.json"))?;
            line(&mut file, &value)?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        state.states.sync_data().map_err(|e| e.to_string())?;
        state.all.get_ref().sync_data().map_err(|e| e.to_string())?;
        state
            .filtered
            .get_ref()
            .sync_data()
            .map_err(|e| e.to_string())?;
        state.events.sync_data().map_err(|e| e.to_string())
    })
}

pub fn check() -> Result<(), String> {
    STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|s| s.error.clone())
            .map_or(Ok(()), |e| Err(format!("DiagnosticBoundary: {e}")))
    })
}

pub fn finish(failure: Option<&str>) -> Result<(), String> {
    check()?;
    detail(
        "capture-end",
        vec![(
            "status".into(),
            if failure.is_some() {
                "failed"
            } else {
                "completed"
            }
            .into(),
        )],
    );
    STATE.with(|slot| {
        let mut slot=slot.borrow_mut();
        let Some(state)=slot.as_mut() else{return Ok(())};
        if let Some(error)=&state.error {return Err(format!("DiagnosticBoundary: {error}"));}
        state.all.flush().map_err(|e|e.to_string())?;
        state.filtered.flush().map_err(|e|e.to_string())?;
        state.all.get_ref().sync_all().map_err(|e|e.to_string())?;
        state.filtered.get_ref().sync_all().map_err(|e|e.to_string())?;
        state.events.sync_all().map_err(|e|e.to_string())?;
        state.states.sync_all().map_err(|e|e.to_string())?;
        let mut marker=create(&state.output.join("capture-end.json"))?;
        line(&mut marker,&json!({"schema_version":1,"run_id":state.run_id,"status":if failure.is_some(){"failed"}else{"completed"},
            "error":failure,"all_records":state.all_ordinal,"filtered_records":state.filtered_ordinal,
            "events_written":state.events_written,"states_written":state.states_written,"last_sequence":state.last_sequence,
            "diagnostics":diagnostics::stats(),
            "trace_level":if state.full{"full"}else{"metadata"},"suppressed_steps":state.skipped_steps,
            "pending_records":0,"truncated":false}))?;
        marker.sync_all().map_err(|e|e.to_string())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn structured_native_values_preserve_float_bits_and_byte_strings() {
        let nan = super::recorded_value(&slr_calllog::Value::FloatBits(0xffc01234));
        assert_eq!(nan["bits"], 0xffc01234u32);
        let negative_zero =
            super::recorded_value(&slr_calllog::Value::DoubleBits(0x8000000000000000));
        assert_eq!(negative_zero["bits"], 0x8000000000000000u64);
        assert_eq!(
            super::recorded_value(&slr_calllog::Value::Bytes(vec![0, 255]))["value"],
            serde_json::json!([0, 255])
        );
    }
}
