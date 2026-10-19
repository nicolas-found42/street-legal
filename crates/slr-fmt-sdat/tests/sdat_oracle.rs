//! Differential tests for the SDAT reader/writer functions of the exe
//! (ticket M2-05, issue #20).
//!
//! Reads the Unicorn oracle vectors at `re/out/sdat/sdat_<name>.jsonl`
//! (git-ignored; produced locally by `re/oracle/sdat_vectors.py`, which
//! calls the real exe code on synthetic objects built in emulator memory
//! — no game data needed, only the exe + venv). Override the directory
//! with `SLR_SDAT_VECTORS`. Skips with a note when the files are absent so
//! `cargo test --workspace` stays green without the exe/venv.
//!
//! Oracle boundary (see the harness docstring): the file layer
//! `0x54cf00` / `0x54cec0` / `0x54cca0` and the JPEG-companion savers are
//! hooked and logged as events; everything above them is the real exe code.
//! The test replays each scenario against the port in `slr_fmt_sdat::store`
//! and compares the result and the ordered event log exactly.

use serde_json::Value;
use slr_fmt_sdat::store::{
    close_file, file_read_float, file_read_int, file_write_float, file_write_int, store_slot_run,
    Event, FileIo, FileState, Handle, ResourceNode, SlotNode, SlotObject,
};
use std::path::PathBuf;

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_SDAT_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/sdat")
}

fn load_rows(name: &str) -> Option<Vec<Value>> {
    let text = std::fs::read_to_string(vectors_dir().join(format!("sdat_{name}.jsonl"))).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("{name} row {i}: {e}")));
    }
    Some(rows)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn event_of(v: &Value) -> Event {
    let a = v.as_array().expect("event array");
    match a[0].as_str().unwrap() {
        "w" => Event::Write {
            slot: a[1].as_u64().unwrap() as u32,
            data: hex(a[2].as_str().unwrap()),
        },
        "r" => Event::Read {
            slot: a[1].as_u64().unwrap() as u32,
            len: a[2].as_u64().unwrap() as u32,
        },
        "close" => Event::Close {
            slot: a[1].as_u64().unwrap() as u32,
        },
        "free" => Event::Free,
        "prepare" => {
            // the exe passes the constant args (0x80000000, 1.0f, 0, 0)
            assert_eq!(
                (a[1].as_u64(), a[2].as_u64(), a[3].as_u64(), a[4].as_u64()),
                (Some(0x8000_0000), Some(0x3F80_0000), Some(0), Some(0))
            );
            Event::Prepare
        }
        "companion" => Event::Companion {
            kind: a[1].as_u64().unwrap() as u32,
            name: a[2].as_str().unwrap().to_string(),
        },
        "listfree" => Event::ListFree {
            offset: a[1].as_u64().unwrap() as u32,
        },
        "setfield" => {
            assert_eq!(a[2].as_u64(), Some(0));
            Event::ClearHandleField
        }
        other => panic!("unknown event {other}"),
    }
}

fn events_of(row: &Value) -> Vec<Event> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(event_of)
        .collect()
}

/// File layer double: logs writes/reads like the harness hooks.
struct Io {
    events: Vec<Event>,
    write_rv: u32,
    input: Vec<u8>,
}

impl FileIo for Io {
    fn write(&mut self, slot: u32, data: &[u8]) -> u32 {
        self.events.push(Event::Write {
            slot,
            data: data.to_vec(),
        });
        self.write_rv
    }
    fn read(&mut self, slot: u32, buf: &mut [u8]) -> u32 {
        self.events.push(Event::Read {
            slot,
            len: buf.len() as u32,
        });
        let n = buf.len().min(self.input.len());
        buf[..n].copy_from_slice(&self.input[..n]);
        self.input.drain(..n);
        n as u32
    }
}

fn handle_of(args: &Value) -> Handle {
    match args["state"].as_str().unwrap() {
        "open" => Handle::Open(args["slot"].as_u64().unwrap() as u32),
        "closed" => Handle::Closed,
        "missing" | "wrongtype" => Handle::Missing,
        s => panic!("state {s}"),
    }
}

fn io_of(args: &Value) -> Io {
    Io {
        events: Vec::new(),
        write_rv: args["rv"].as_u64().unwrap_or(4) as u32,
        input: hex(args["input"].as_str().unwrap_or("")),
    }
}

fn f64_bits(v: &Value) -> u64 {
    u64::from_le_bytes(hex(v["st0_f64"].as_str().unwrap()).try_into().unwrap())
}

fn nodes_of(v: &Value) -> Vec<SlotNode> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| SlotNode {
            id: n["id"].as_u64().unwrap() as u32,
            kind: n["kind"].as_u64().unwrap() as u32,
            object: n.get("obj").map(|o| SlotObject {
                blob: o["blob"].as_str().map(hex),
            }),
        })
        .collect()
}

#[test]
fn file_natives_match_oracle_vectors() {
    let names = ["write_int", "write_float", "read_int", "read_float"];
    let mut counts = Vec::new();
    for name in names {
        let Some(rows) = load_rows(name) else {
            eprintln!("SLR_ORACLE_SKIPPED: file_natives_match_oracle_vectors");
            eprintln!("SKIP: no oracle vectors (run re/oracle/sdat_vectors.py gen)");
            return;
        };
        assert!(!rows.is_empty(), "{name}: no rows");
        for row in &rows {
            let args = &row["args"];
            let h = handle_of(args);
            let mut io = io_of(args);
            let res = &row["result"];
            let ctx = format!("{name} row {}", row["id"]);
            match name {
                "write_int" => {
                    let rv = file_write_int(&mut io, h, args["value"].as_u64().unwrap() as u32);
                    assert_eq!(rv as u64, res["eax"].as_u64().unwrap(), "{ctx}");
                }
                "write_float" => {
                    let f = f32::from_bits(args["value"].as_u64().unwrap() as u32);
                    let rv = file_write_float(&mut io, h, f);
                    assert_eq!(rv as u64, res["eax"].as_u64().unwrap(), "{ctx}");
                }
                "read_int" => {
                    let got = file_read_int(&mut io, h);
                    if res.get("unspecified").is_some() {
                        assert_eq!(got, None, "{ctx}: exe result is undefined");
                    } else {
                        assert_eq!(got.map(u64::from), res["eax"].as_u64(), "{ctx}");
                    }
                }
                _ => {
                    let got = file_read_float(&mut io, h);
                    if res.get("unspecified").is_some() {
                        assert_eq!(got, None, "{ctx}: exe result is undefined");
                    } else {
                        // `fld dword` widens to ST0; compare the f64 view
                        let g = (got.expect(&ctx) as f64).to_bits();
                        let want = f64_bits(res);
                        if f64::from_bits(want).is_nan() {
                            assert!(f64::from_bits(g).is_nan(), "{ctx}");
                        } else {
                            assert_eq!(g, want, "{ctx}");
                        }
                    }
                }
            }
            assert_eq!(io.events, events_of(row), "{ctx}: file-layer events");
        }
        counts.push(format!("{name}={}", rows.len()));
    }
    eprintln!("sdat_oracle natives: {}", counts.join(" "));
}

#[test]
fn slot_run_store_matches_oracle_vectors() {
    let Some(rows) = load_rows("store_slots") else {
        eprintln!("SLR_ORACLE_SKIPPED: slot_run_store_matches_oracle_vectors");
        eprintln!("SKIP: no oracle vectors (run re/oracle/sdat_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());
    for row in &rows {
        let a = &row["args"];
        let run = store_slot_run(
            a["slot"].as_u64().unwrap() as u32,
            a["name"].as_str().unwrap(),
            &nodes_of(&a["nodes"]),
        )
        .unwrap_or_else(|e| panic!("row {}: {e:?}", row["id"]));
        assert_eq!(
            run.bytes as u64,
            row["result"]["eax"].as_u64().unwrap(),
            "row {}: return value",
            row["id"]
        );
        assert_eq!(run.events, events_of(row), "row {}: events", row["id"]);
    }
    eprintln!("sdat_oracle store_slots: rows={}", rows.len());
}

#[test]
fn file_close_matches_oracle_vectors() {
    let Some(rows) = load_rows("close") else {
        eprintln!("SLR_ORACLE_SKIPPED: file_close_matches_oracle_vectors");
        eprintln!("SKIP: no oracle vectors (run re/oracle/sdat_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());
    for row in &rows {
        let a = &row["args"];
        let state = FileState {
            slot: a["slot"].as_u64().unwrap() as u32,
            mode: a["mode"].as_u64().unwrap() as u32,
            version: a["version"].as_u64().unwrap() as u8,
            name: a["name"].as_str().unwrap().to_string(),
            slot_count_word: a["slot_count"].as_u64().unwrap() as u32,
            resource_count_word: a["res_count"].as_u64().unwrap() as u32,
            slots: nodes_of(&a["nodes"]),
            resources: a["res"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| ResourceNode {
                    id: r["id"].as_u64().unwrap() as u32,
                    path: r["path"].as_str().unwrap().to_string(),
                })
                .collect(),
        };
        let events = close_file(&state).unwrap_or_else(|e| panic!("row {}: {e:?}", row["id"]));
        assert_eq!(events, events_of(row), "row {}: events", row["id"]);
    }
    eprintln!("sdat_oracle close: rows={}", rows.len());
}
