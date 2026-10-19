//! Differential test for the ten `java.sound.Sound` rows (M1-12).
//!
//! Replays `re/out/sound/sound.jsonl`, produced by
//! `re/oracle/sound_vectors.py gen`: the REAL exe natives executed under
//! Unicorn on a hand-built `CallInfo` (no JVM boot, no game data), with the
//! music set, the three volume words and the two DirectSound capability words
//! planted. Override the directory with `SLR_SOUND_VECTORS`. Skips with a note
//! when the file is absent so `cargo test --workspace` stays green without the
//! exe/venv.
//!
//! Oracle boundary (see the harness docstring): `0x550f30` (release),
//! `0x550e50` (install) and `0x550fe0` (push channel 1's volume) are hooked and
//! logged; everything between them is the real exe code, including `0x45d910`
//! marshalling.
//!
//! Normalisations:
//!
//! - the four music-set "objects" are the image's static descriptor addresses
//!   (`0x63c878`/`0x640908`/`0x6408a8`/`0x640890`); the port's set words are
//!   the same addresses, so they compare directly;
//! - a row's `events` are `[name, word]` pairs; the port's [`SoundEvent`]s map
//!   onto them one-to-one (`release`, `install`, `push_volume`, `reload`).

use serde_json::Value as Json;
use slr_sound as s;
use slr_vm::host::{CallInfo, VmHost};
use slr_vm::values::Value;

const RECEIVER: u32 = 0x0050_0000;

fn vectors_dir() -> std::path::PathBuf {
    std::env::var_os("SLR_SOUND_VECTORS").map_or_else(
        || {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/sound")
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

fn call(vm: &mut VmHost, row: &s::SoundBinding, params: &[(u8, u32)]) -> Option<u32> {
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

/// The port's events as the probe's `[name, word]` pairs.
///
/// A [`SoundEvent::Reload`] carries the same `install` call the probe records,
/// but its track word is compared through the row's globals instead of the
/// event, so only the install it ends with is mapped here.
fn port_events() -> Vec<(String, u32)> {
    s::with_sound(|h| {
        h.events()
            .iter()
            .filter_map(|e| match e {
                s::SoundEvent::Release { object } => Some(("release".to_owned(), *object)),
                s::SoundEvent::Install { object } => Some(("install".to_owned(), *object)),
                s::SoundEvent::MusicVolume { bits } => Some(("push_volume".to_owned(), *bits)),
                s::SoundEvent::Reload { .. } => None,
            })
            .collect()
    })
}

#[test]
fn the_sound_rows_replay() {
    let Some(rows) = load("sound.jsonl") else {
        eprintln!("SKIP: no re/out/sound/sound.jsonl (run re/oracle/sound_vectors.py gen)");
        return;
    };
    let mut checked = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id");
        let name = row["name"].as_str().expect("name");
        let sig = row["sig"].as_str().expect("sig");
        let want = &row["result"];

        let binding = s::SOUND_NATIVE_BINDINGS
            .iter()
            .find(|b| b.name == name && b.sig == sig)
            .unwrap_or_else(|| panic!("{id}: no binding for {name}{sig}"));

        s::reset_sound();
        // plant the row's world into the port's host
        let world = &row["world"];
        let current = world["current"].as_u64().unwrap_or(0) as u32;
        if current != 0 {
            let track = world["tracks"][current.to_string()].as_u64().unwrap_or(0) as u32 as i32;
            let mode = world["modes"][current.to_string()].as_u64().unwrap_or(0) as u32 as i32;
            s::with_sound(|h| {
                h.set_current(Some(current));
                h.set_set_words(current, track, mode);
            });
        }
        if let Some(volumes) = world["volumes"].as_array() {
            s::with_sound(|host| {
                for (channel, bits) in volumes.iter().enumerate() {
                    host.set_volume_bits(channel, bits.as_u64().expect("volume bits") as u32);
                }
            });
        }
        if let Some(ds_3d) = world["ds_3d"].as_u64() {
            s::with_sound(|h| {
                let (_, mix) = h.capabilities();
                h.set_capabilities(ds_3d as u32, mix);
            });
        }
        if let Some(ds_mix) = world["ds_mix"].as_u64() {
            s::with_sound(|h| {
                let (three_d, _) = h.capabilities();
                h.set_capabilities(three_d, ds_mix as u32);
            });
        }

        let mut vm = VmHost::new();
        s::install_headless(&mut vm);
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
        {
            let expected_result = match s::return_type(sig) {
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

        if let Some(err) = want["error"].as_str() {
            panic!("{id}: the probe faulted ({err}); a replay row must not");
        }

        // the answer word: a void row answers nothing, a float row on ST0,
        // and the marshaller's leftover EAX is not a result
        match s::return_type(sig) {
            b'V' => assert_eq!(got, None, "{id}: void"),
            b'F' => {
                let want_bits = want["st0"].as_f64().map(|v| (v as f32).to_bits());
                assert_eq!(got, want_bits, "{id}: float result");
            }
            _ => {
                let want_word = want["eax"].as_u64().map(|w| w as u32);
                assert_eq!(got, want_word, "{id}: result");
            }
        }

        // the library calls the row made, in order
        let want_events: Vec<(String, u32)> = want["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|e| {
                let pair = e.as_array().expect("pair");
                (
                    pair[0].as_str().expect("name").to_owned(),
                    pair[1].as_u64().expect("word") as u32,
                )
            })
            .collect();
        assert_eq!(port_events(), want_events, "{id}: events");

        let globals = &want["globals"];
        s::with_sound(|host| {
            assert_eq!(
                host.current().unwrap_or(0),
                globals["current"].as_u64().unwrap() as u32,
                "{id}: current set"
            );
            let want_track = globals["track"].as_u64().unwrap() as u32 as i32;
            let want_mode = globals["mode"].as_u64().unwrap() as u32 as i32;
            assert_eq!(host.track().unwrap_or(0), want_track, "{id}: track");
            assert_eq!(host.mode().unwrap_or(0), want_mode, "{id}: mode");
            for channel in 0..3 {
                assert_eq!(
                    host.volume_bits(channel),
                    globals["volume"][channel].as_u64().map(|word| word as u32),
                    "{id}: channel {channel}"
                );
            }
        });
        checked += 1;
    }
    assert!(checked >= 30, "replayed only {checked} sound rows");
}
