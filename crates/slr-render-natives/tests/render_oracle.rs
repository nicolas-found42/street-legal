//! Differential test for the `java.render.*` rows (M1-12).
//!
//! Replays `re/out/render/render.jsonl`, produced by
//! `re/oracle/render_vectors.py gen`: the REAL exe natives executed under
//! Unicorn on a hand-built `CallInfo` (no JVM boot, no game data). Override the
//! directory with `SLR_RENDER_VECTORS`. Skips with a note when the file is
//! absent so `cargo test --workspace` stays green without the exe/venv.
//!
//! The rows whose whole body is global-word traffic are compared directly; the
//! device effects are compared at recording-only library boundaries. All
//! recorded rows must return normally; faulting fixtures cannot pass.

use serde_json::Value as Json;
use slr_render_natives as r;
use slr_vm::host::{CallInfo, VmHost};
use slr_vm::values::Value;

const RECEIVER: u32 = 0x0050_0000;

fn vectors_dir() -> std::path::PathBuf {
    std::env::var_os("SLR_RENDER_VECTORS").map_or_else(
        || {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/render")
                .to_path_buf()
        },
        std::path::PathBuf::from,
    )
}

fn load(name: &str) -> Option<Vec<Json>> {
    let text = std::fs::read_to_string(vectors_dir().join(name)).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("row {i}: {e}")));
    }
    Some(rows)
}

fn row_named(name: &str) -> &'static r::RenderBinding {
    r::RENDER_NATIVE_BINDINGS
        .iter()
        .find(|row| row.name == name)
        .expect("row")
}

/// The container the ABI walk reads: `params` reversed, then the receiver.
fn call(vm: &mut VmHost, row: &r::RenderBinding, params: &[(u8, u32)]) -> Option<u32> {
    let mut args: Vec<Value> = params
        .iter()
        .rev()
        .map(|(ty, word)| match ty {
            b'F' => Value::float(f32::from_bits(*word)),
            b'L' | b'[' => Value::reference(*word),
            _ => Value::int(*word as i32),
        })
        .collect();
    args.push(Value::reference(RECEIVER));
    vm.native_impls[&row.impl_addr](
        vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: row.class,
            name: row.name,
            sig: row.sig,
        },
    )
}

fn params(row: &Json) -> Vec<(u8, u32)> {
    row["args"]["params"]
        .as_array()
        .expect("params")
        .iter()
        .map(|p| {
            let pair = p.as_array().expect("pair");
            let ty = pair[0].as_str().expect("type");
            (ty.as_bytes()[0], pair[1].as_u64().expect("word") as u32)
        })
        .collect()
}

/// The world the row was probed with, planted into the port's host.
fn plant(host: &mut r::RenderHost, world: &Json) {
    if let Some(modes) = world["modes"].as_array() {
        host.set_display_modes(
            modes
                .iter()
                .map(|entry| {
                    let words = entry.as_array().expect("mode");
                    [
                        words[0].as_i64().expect("w") as i32,
                        words[1].as_i64().expect("h") as i32,
                        words[2].as_i64().expect("bpp") as i32,
                    ]
                })
                .collect(),
        );
    }
    if let Some(current) = world["current"].as_array() {
        host.set_current_desc([
            current[0].as_i64().expect("w") as i32,
            current[1].as_i64().expect("h") as i32,
            current[2].as_i64().expect("bpp") as i32,
        ]);
    }
    if let Some(color) = world["text_color"].as_i64() {
        host.set_text_color(color as i32);
    }
    if let Some(video) = world["video_playing"].as_u64() {
        host.set_video_playing(video as u32);
    }
    let dirty = world["dirty"].as_u64().unwrap_or(0) as u32;
    let frame = world["frame"].as_u64().unwrap_or(0) as u32;
    host.set_flush_state(dirty, frame);
}

#[test]
fn the_render_rows_replay() {
    let Some(rows) = load("render.jsonl") else {
        eprintln!("SKIP: no re/out/render/render.jsonl (run re/oracle/render_vectors.py gen)");
        return;
    };
    let mut checked = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id");
        let name = row["name"].as_str().expect("name");
        let sig = row["sig"].as_str().expect("sig");
        let want = &row["result"];
        assert!(
            want["error"].is_null(),
            "{id}: oracle fault: {}",
            want["error"]
        );
        let want_eax = want["eax"].as_u64().map(|w| w as u32);
        let world = &row["args"]["world"];

        let binding = r::RENDER_NATIVE_BINDINGS
            .iter()
            .find(|b| b.name == name && b.sig == sig)
            .unwrap_or_else(|| panic!("{id}: no binding for {name}{sig}"));

        r::reset_render();
        r::with_render(|h| plant(h, world));
        let mut vm = VmHost::new();
        r::install_headless(&mut vm);
        slr_calllog::headless::reset();
        let got = call(&mut vm, binding, &params(row));

        // Expected native log fields come from the executable vector, not
        // from a second invocation of the port.
        let records = slr_calllog::headless::take_records();
        assert_eq!(records.len(), 1, "{id}: one native record");
        let slr_calllog::LogRecord::NativeCall {
            class,
            name: logged_name,
            args: logged_args,
            result: logged_result,
        } = &records[0]
        else {
            panic!("{id}: native record")
        };
        assert_eq!(
            class,
            row["class"].as_str().unwrap(),
            "{id}: declaring class"
        );
        assert_eq!(logged_name, name, "{id}: method");
        let expected_args: Vec<slr_calllog::Value> = params(row)
            .into_iter()
            .map(|(ty, word)| {
                if ty == b'F' {
                    slr_calllog::Value::FloatBits(word)
                } else {
                    slr_calllog::Value::Int(word as i32)
                }
            })
            .collect();
        assert_eq!(logged_args, &expected_args, "{id}: logged arguments");
        let mut wire = slr_calllog::LogWriter::create(Vec::new()).unwrap();
        wire.record(&records[0]).unwrap();
        let bytes = wire.into_inner();
        let mut reader = slr_calllog::LogReader::open(bytes.as_slice()).unwrap();
        assert_eq!(
            reader.next_record().unwrap(),
            Some(&records[0]),
            "{id}: schema roundtrip"
        );
        assert!(reader.next_record().unwrap().is_none());
        if sig.ends_with("Ljava.lang.String;") {
            let Some(slr_calllog::Value::Int(word)) = logged_result else {
                panic!("{id}: string handle")
            };
            let actual_text = r::with_render(|host| host.string(*word as u32).map(str::to_owned));
            assert_eq!(
                actual_text.as_deref(),
                want["interns"][0].as_str(),
                "{id}: logged string result"
            );
        } else {
            let expected_result = match r::return_type(sig) {
                b'V' => None,
                b'F' => Some(slr_calllog::Value::FloatBits(
                    (want["st0"].as_f64().expect("float result") as f32).to_bits(),
                )),
                _ => Some(slr_calllog::Value::Int(
                    want["eax"].as_u64().expect("integer result") as u32 as i32,
                )),
            };
            assert_eq!(logged_result, &expected_result, "{id}: logged result");
        }

        // The interned word is the port's arena, not the exe's, so a
        // string-returning row compares the interned text instead.
        if sig.ends_with("Ljava.lang.String;") {
            let interned =
                r::with_render(|h| got.and_then(|word| h.string(word).map(str::to_owned)));
            let want_text = want["interns"]
                .as_array()
                .and_then(|list| list.first())
                .and_then(Json::as_str)
                .unwrap_or("");
            assert_eq!(interned.as_deref(), Some(want_text), "{id}: interned text");
        } else if r::return_type(sig) == b'F' {
            // A float row answers on ST0.
            let want_bits = want["st0"].as_f64().map(|v| (v as f32).to_bits());
            assert_eq!(got, want_bits, "{id}: float result");
        } else if r::return_type(sig) == b'V' {
            // A `void` row leaves whatever the marshaller left in `eax`; the
            // native result is nothing, so only the effect is compared.
            assert_eq!(got, None, "{id}: void");
        } else {
            assert_eq!(got, want_eax, "{id}: result");
        }

        // The globals the probe read back after the call.
        let globals = &want["globals"];
        if name == "setDefaultColor" {
            let want_color = globals["text_color"].as_u64().expect("colour") as u32;
            assert_eq!(
                call(&mut vm, row_named("getDefaultColor"), &[]),
                Some(want_color),
                "{id}: the colour word"
            );
        }
        if name == "forceRendering" || name == "flush" {
            let want_frame = globals["frame"].as_u64().expect("frame") as u32;
            r::with_render(|h| {
                assert_eq!(h.frame(), want_frame, "{id}: frame counter");
                assert_eq!(h.dirty(), 0, "{id}: the dirty word is cleared");
                // Seven passes, three events each.
                let selects = h
                    .events()
                    .iter()
                    .filter(|e| **e == r::RenderEvent::ManagerSelect)
                    .count();
                assert_eq!(selects, r::FLUSH_PASSES as usize, "{id}: passes");
            });
        }
        let events: Vec<Json> = r::with_render(|host| {
            host.events()
                .iter()
                .map(|event| match event {
                    r::RenderEvent::ManagerSelect => serde_json::json!(["manager_select"]),
                    r::RenderEvent::ManagerFlush => serde_json::json!(["manager_flush"]),
                    r::RenderEvent::ManagerFinish => serde_json::json!(["manager_finish", 1]),
                    r::RenderEvent::DeviceSync => serde_json::json!(["device_sync"]),
                    r::RenderEvent::DeviceFlush => serde_json::json!(["device_flush"]),
                    r::RenderEvent::ChangeVideoMode {
                        first,
                        second,
                        third,
                    } => serde_json::json!(["change_video_mode", first, second, third]),
                })
                .collect()
        });
        assert_eq!(
            serde_json::json!(events),
            want["events"],
            "{id}: ordered boundary effects"
        );
        checked += 1;
    }
    assert!(checked >= 22, "replayed only {checked} render rows");
}
