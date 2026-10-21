//! Differential test: the `slr-resource` handle model against the exe's own
//! `java.util.resource.*` handle natives, executed in Unicorn
//! (`re/oracle/res_vectors.py gen`, M1-11 / issue #38).
//!
//! Oracle: **Unicorn function oracle** — the natives run as real exe code with
//! the JVM class tables, allocator and log sinks hooked, so the recorded facts
//! are the exe's own queue words, particle-object words, gate outcomes and
//! engine-call arguments. Vectors live in git-ignored `re/out/res/`
//! (`res_<group>_<native>.jsonl`); override the directory with
//! `SLR_RES_VECTORS`. The test SKIPs with a printed note when the files are
//! absent so `cargo test --workspace` stays green on a fresh worktree — same
//! pattern as `crates/slr-fmt-texture/tests/ptx_oracle.rs` and
//! `crates/slr-vm/tests/vmstr_oracle.rs`.
//!
//! Three suites, one per recorded group (171 rows total):
//!
//! - `anim_*` (57 rows) — the six `Animation` queue setters. Every row replays
//!   `handles::anim_push` on a `HandleTable` `Animation` entry pre-loaded with
//!   the row's `pre` queue and compares the port's queue and count with the
//!   exe's. The payload is the row's float bits for `(F)V` and 0 for the
//!   no-argument commands, so the comparison pins the tag/payload mapping.
//!   Two recorded facts stay out of scope: the queue `capacity` word (the port
//!   models the queue as a `Vec`, the exe grows by `0x20` entries through
//!   `FUN_0054f580`), and the `[A+8]` handle-exists word. The rows with
//!   `exists = 0` record the exe's early return (no append); the port has no
//!   such word, so its analogue — handle liveness — is driven instead: the
//!   handle is released and `anim_push` must refuse while the row's recorded
//!   queue and count are checked to be unchanged.
//! - `psys_*` (59 rows) — the five `ParticleSystem` natives. Each row rebuilds
//!   the port's `flags`/`freq_bits`/action list from `env`/`pre`, calls the
//!   single port function, and compares the port's object words and the whole
//!   action list (type, label, counter, rate, domain words) with the exe's
//!   live chain. `setCounter` feeds `handles::build_domain(rate, 0.0f)` — the
//!   pair the exe itself builds — so the domain rows also pin
//!   `FUN_0047f270`/`FUN_0047f660`. Not modelled (and not compared): the env
//!   `mode` word that only selects an extra materialise call before the gate,
//!   the `events` list of `Vector3` component reads, and the `Vector3` words
//!   the exe also stores in the domain object.
//! - `gate_*` (55 rows) — the gate/constant natives. `sfx_nplay` drives
//!   `sfx_target_in_range` (the forwarded `Vector3` word is nonzero exactly
//!   when the gate passes) and `sfx_scaled_volume` (against the exe's eight
//!   pushed engine words, so the operand order is pinned); `sfx_stop` drives
//!   the `EngineSink::sfx_stop` passthrough through `Recorder`; `getTypeID`
//!   drives `render_type_gate` against the recorded `fetched` count and result
//!   word; `getDetail` drives `render_detail`; `setMatrix` drives
//!   `phys_slot_addr` against the recorded body addresses. The one
//!   `env_null` `getDetail` row is counted and skipped: the exe misses at the
//!   env-pointer test *before* the helper's gate, and the port helper takes no
//!   env word (that gate belongs to the caller).

use serde_json::{json, Value};
use slr_resource::handles::{
    anim_push, build_domain, phys_slot_addr, psys_del_action, psys_get_counter,
    psys_mode_permanent, psys_set_counter, psys_set_freq, render_detail, render_type_gate,
    sfx_scaled_volume, sfx_target_in_range, Action, AnimCommand, Command, Domain, EngineSink,
    HandleKind, HandleTable, Recorder,
};
use std::path::PathBuf;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_RES_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/res"))
}

/// One suite's rows, or `None` when the file is absent (skip signal).
fn rows(suite: &str) -> Option<Vec<Value>> {
    let path = vectors_dir().join(format!("res_{suite}.jsonl"));
    let text = std::fs::read_to_string(&path).ok()?;
    Some(
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
            .collect(),
    )
}

/// Print the skip note once per suite when no vector file was found, so a run
/// without the exe/venv is visibly a skip and not a silent pass.
fn skip_note(test: &str, group: &str, suites: &[&str]) -> bool {
    let dir = vectors_dir();
    let mut found = false;
    for suite in suites {
        if dir.join(format!("res_{suite}.jsonl")).is_file() {
            found = true;
        }
    }
    if !found {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        eprintln!(
            "SKIP res_oracle {group}: no vector files in {} (run re/oracle/res_vectors.py gen --exe <stock exe>)",
            dir.display()
        );
    }
    found
}

fn u32_of(v: &Value) -> u32 {
    v.as_u64().expect("u32") as u32
}

fn words(v: &Value) -> Vec<u32> {
    v.as_array().expect("array").iter().map(u32_of).collect()
}

// ---------------------------------------------------------------- Animation

/// `(suite, command)` for the six queue setters.
const ANIM_SUITES: [(&str, AnimCommand); 6] = [
    ("anim_setSpeed", AnimCommand::SetSpeed),
    ("anim_setFade", AnimCommand::SetFade),
    ("anim_play", AnimCommand::Play),
    ("anim_loopPlay", AnimCommand::Loop),
    ("anim_pause", AnimCommand::Pause),
    ("anim_seek", AnimCommand::Seek),
];

/// The recorded `[tag, payload]` pairs as port [`Command`]s.
fn commands(v: &Value) -> Vec<Command> {
    v.as_array()
        .expect("queue")
        .iter()
        .map(|e| {
            let w = words(e);
            Command {
                tag: w[0],
                payload: w[1],
            }
        })
        .collect()
}

#[test]
fn anim_queue_matches_oracle_vectors() {
    if !skip_note(
        "anim_queue_matches_oracle_vectors",
        "anim",
        &ANIM_SUITES.map(|(s, _)| s),
    ) {
        return;
    }
    let (mut total, mut suppressed, mut suites) = (0usize, 0usize, 0usize);
    for (suite, cmd) in ANIM_SUITES {
        let Some(rows) = rows(suite) else {
            eprintln!("note res_oracle anim: res_{suite}.jsonl absent, suite skipped");
            continue;
        };
        suites += 1;
        for row in &rows {
            let id = row["id"].as_u64().expect("id");
            let ctx = format!("{suite} row {id}");
            let args = &row["args"];
            let result = &row["result"];
            let pre = commands(&args["pre"]);
            let want = commands(&result["queue"]);
            let want_count = u32_of(&result["count"]) as usize;
            let exists = args["exists"].as_u64().expect("exists") != 0;
            // The setters take the float as their argument; the no-argument
            // commands push a zero payload.
            let takes_float = row["sig"].as_str().expect("sig") == "(F)V";
            assert_eq!(
                takes_float,
                cmd.has_float_payload(),
                "{ctx}: command/signature mismatch"
            );
            let payload = if takes_float {
                u32_of(&args["f32_bits"])
            } else {
                0
            };
            assert_eq!(want_count, want.len(), "{ctx}: vector count/queue disagree");

            let mut t = HandleTable::new();
            let h = t.alloc(HandleKind::Animation, vec![]);
            t.get_mut(h).unwrap().queue = pre.clone();

            if !exists {
                // `[A+8]` is clear: the exe returns without pushing. The port
                // models that state as a dead handle.
                assert!(t.free(h), "{ctx}: handle was live");
                assert!(
                    !anim_push(&mut t, h, cmd, payload),
                    "{ctx}: cleared handle must not append"
                );
                assert_eq!(want, pre, "{ctx}: the vector records no append");
                assert_eq!(want_count, pre.len(), "{ctx}: vector count unchanged");
                suppressed += 1;
                total += 1;
                continue;
            }

            let mut expect = pre.clone();
            expect.push(Command {
                tag: cmd.tag(),
                payload,
            });
            assert_eq!(want, expect, "{ctx}: vector queue is pre + [tag, payload]");
            assert!(
                anim_push(&mut t, h, cmd, payload),
                "{ctx}: a live handle must append"
            );
            let entry = t.get(h).expect("live handle");
            assert_eq!(entry.queue, want, "{ctx}: port queue");
            assert_eq!(entry.queue.len(), want_count, "{ctx}: port count");
            total += 1;
        }
    }
    eprintln!(
        "res_oracle anim: {total} rows replayed over {suites} suites ({suppressed} with the \
         handle-exists word clear)"
    );
}

// ------------------------------------------------------------ ParticleSystem

const PSYS_SUITES: [&str; 5] = [
    "psys_modePermanent",
    "psys_setFreq",
    "psys_getCounter",
    "psys_delAction",
    "psys_setCounter",
];

/// The exe's counter-node type id (`+0x10`).
const COUNTER_TYPE: u32 = 0xd;

/// One recorded action node as a port [`Action`].
fn action(v: &Value) -> Action {
    let domain = if v["domain"].is_null() {
        None
    } else {
        let w = words(&v["domain"]);
        Some(Domain {
            ext: [w[0], w[1]],
            ext_sq: [w[2], w[3]],
        })
    };
    Action {
        type_id: u32_of(&v["type"]),
        label: v["label"].as_str().expect("label").to_owned(),
        counter: u32_of(&v["counter"]),
        rate: u32_of(&v["rate"]),
        domain,
        domain1: None,
        domain2: None,
        bone: 0,
    }
}

/// The port's node projection in the vector's own shape (`+0x10` type id,
/// `+0x14` label, `+0x54` rate, `+0x58` counter, `+0x60` domain words).
fn action_json(a: &Action) -> Value {
    json!({
        "type": a.type_id,
        "label": a.label,
        "counter": a.counter,
        "rate": a.rate,
        "domain": a.domain.map(|d| json!([d.ext[0], d.ext[1], d.ext_sq[0], d.ext_sq[1]])),
    })
}

#[test]
fn particle_actions_match_oracle_vectors() {
    if !skip_note(
        "particle_actions_match_oracle_vectors",
        "psys",
        &PSYS_SUITES,
    ) {
        return;
    }
    let mut total = 0usize;
    let mut suites = 0usize;
    for suite in PSYS_SUITES {
        let Some(rows) = rows(suite) else {
            eprintln!("note res_oracle psys: res_{suite}.jsonl absent, suite skipped");
            continue;
        };
        suites += 1;
        for row in &rows {
            let id = row["id"].as_u64().expect("id");
            let ctx = format!("{suite} row {id}");
            let args = &row["args"];
            let result = &row["result"];
            let env = &args["env"];
            let pre: Vec<Action> = args["pre"]
                .as_array()
                .expect("pre")
                .iter()
                .map(action)
                .collect();
            let call = &args["call"];

            let mut t = HandleTable::new();
            let h = t.alloc(HandleKind::ParticleSystem, vec![]);
            {
                let e = t.get_mut(h).unwrap();
                e.flags = u32_of(&env["flags"]);
                e.freq_bits = u32_of(&env["freq_bits"]);
                e.actions = pre.clone();
            }

            match suite {
                "psys_modePermanent" => {
                    let mode = u32_of(&call["mode"]);
                    assert!(psys_mode_permanent(&mut t, h, mode), "{ctx}");
                }
                "psys_setFreq" => {
                    let bits = u32_of(&call["freq_bits"]);
                    assert!(psys_set_freq(&mut t, h, bits), "{ctx}");
                }
                "psys_getCounter" => {
                    let label = call["label"].as_str().expect("label");
                    let got = psys_get_counter(&mut t, h, label);
                    assert_eq!(got, u32_of(&result["ret"]), "{ctx}: returned word");
                }
                "psys_delAction" => {
                    let label = call["label"].as_str().expect("label");
                    let freed = result["freed"].as_bool().expect("freed");
                    assert_eq!(psys_del_action(&mut t, h, label), freed, "{ctx}: freed");
                }
                "psys_setCounter" => {
                    let label = call["label"].as_str().expect("label").to_owned();
                    // The exe builds the domain from the rate and 0.0f.
                    let domain = build_domain(u32_of(&call["freq_bits"]), 0.0f32.to_bits());
                    // A first label match whose node is not type 0xd aborts the
                    // call; no match appends a fresh node.
                    let matched = pre
                        .iter()
                        .find(|a| a.label.eq_ignore_ascii_case(&label))
                        .map(|a| a.type_id);
                    let expected = matched.is_none_or(|ty| ty == COUNTER_TYPE);
                    let got = psys_set_counter(&mut t, h, label, domain);
                    assert_eq!(got, expected, "{ctx}: reuse/append/abort");
                }
                other => panic!("unhandled psys suite {other}"),
            }

            let entry = t.get(h).expect("live handle");
            assert_eq!(
                entry.flags,
                u32_of(&result["flags"]),
                "{ctx}: modePermanent flags word"
            );
            assert_eq!(
                entry.freq_bits,
                u32_of(&result["freq"]),
                "{ctx}: setFreq rate word"
            );
            let want: Vec<Value> = result["actions"].as_array().expect("actions").clone();
            let got: Vec<Value> = entry.actions.iter().map(action_json).collect();
            assert_eq!(got, want, "{ctx}: action list");
            total += 1;
        }
    }
    eprintln!("res_oracle psys: {total} rows replayed over {suites} suites");
}

// ------------------------------------------------------------ gate constants

const GATE_SUITES: [&str; 5] = [
    "gate_sfx_nplay",
    "gate_sfx_stop",
    "gate_render_gettypeid",
    "gate_render_getdetail",
    "gate_phys_setmatrix",
];

/// `getDetail`'s default float: the exe loads `[0x5e73cc]` = 0.0f, and the
/// port takes that default as a parameter.
const DETAIL_DEFAULT: u32 = 0.0f32.to_bits();

/// `Ypr` orientation offset minus `Vector3` position offset inside a body slot.
const ORI_FROM_POS: u32 = 0xa4 - 0x64;

#[test]
fn handle_gates_match_oracle_vectors() {
    if !skip_note("handle_gates_match_oracle_vectors", "gate", &GATE_SUITES) {
        return;
    }
    let mut total = 0usize;
    let mut suites = 0usize;
    let mut env_null_skipped = 0usize;
    for suite in GATE_SUITES {
        let Some(rows) = rows(suite) else {
            eprintln!("note res_oracle gate: res_{suite}.jsonl absent, suite skipped");
            continue;
        };
        suites += 1;
        // The physics body base is not recorded; every row must reduce to the
        // same one through the port's stride.
        let mut body_base: Option<u32> = None;
        for row in &rows {
            let id = row["id"].as_u64().expect("id");
            let ctx = format!("{suite} row {id}");
            let args = &row["args"];
            let result = &row["result"];
            match suite {
                "gate_sfx_nplay" => {
                    let play = &result["play"];
                    assert!(!play.is_null(), "{ctx}: the engine tail ran");
                    let pushed = words(&play["args"]);
                    assert_eq!(pushed.len(), 8, "{ctx}: eight pushed engine words");
                    let vol = f32::from_bits(u32_of(&args["fc"]));
                    let unit = u32_of(&result["unit"]);
                    let vec_arg = args["vec"].as_bool().expect("vec");
                    let in_range = sfx_target_in_range(if vec_arg { 1 } else { 0 });
                    assert_eq!(in_range, vec_arg, "{ctx}: Vector3 gate");
                    assert_eq!(pushed[3] != 0, in_range, "{ctx}: forwarded Vector3 word");
                    // The eight pushed words: [arg3, unit*arg4, arg5, vector,
                    // arg6, arg2, 0, 0] with arg2..arg4 = fa/fb/fc.
                    assert_eq!(pushed[0], u32_of(&args["fb"]), "{ctx}: arg3");
                    assert_eq!(
                        pushed[1],
                        sfx_scaled_volume(unit, vol),
                        "{ctx}: DAT_00612c58 * vol"
                    );
                    assert_eq!(pushed[2], u32_of(&args["d"]), "{ctx}: arg5");
                    assert_eq!(pushed[4], u32_of(&args["e"]), "{ctx}: arg6");
                    assert_eq!(pushed[5], u32_of(&args["fa"]), "{ctx}: arg2");
                    assert_eq!((pushed[6], pushed[7]), (0, 0), "{ctx}: trailing words");
                }
                "gate_sfx_stop" => {
                    let stop = &result["stop"];
                    assert!(!stop.is_null(), "{ctx}: the engine tail ran");
                    let handle = u32_of(&stop["handle"]);
                    let channel = u32_of(&stop["channel"]);
                    assert_eq!(channel, u32_of(&args["channel"]), "{ctx}: channel arg");
                    let mut rec = Recorder::new();
                    rec.sfx_stop(handle, channel);
                    assert_eq!(
                        rec.calls,
                        vec![format!("sfx_stop({handle},{channel})")],
                        "{ctx}: sink passthrough"
                    );
                }
                "gate_render_gettypeid" => {
                    let mode = u32_of(&args["mode"]);
                    // `env_null` clears `[obj+0xc]`; the fetch returns the
                    // envelope object unless `fetch_null`.
                    let env_word = if args["env_null"].as_bool().unwrap_or(false) {
                        0
                    } else {
                        1
                    };
                    let envelope = if args["fetch_null"].as_bool().unwrap_or(false) {
                        0
                    } else {
                        1
                    };
                    let fetched = u32_of(&result["fetched"]) != 0;
                    let gated_in = env_word != 0 && mode == 3;
                    assert_eq!(fetched, gated_in, "{ctx}: 0x419860 fetch reached");
                    let gate = render_type_gate(env_word, mode, envelope);
                    assert_eq!(gate, gated_in && envelope != 0, "{ctx}: type gate");
                    let want = if gate { u32_of(&args["type_word"]) } else { 0 };
                    assert_eq!(u32_of(&result["ret"]), want, "{ctx}: returned word");
                }
                "gate_render_getdetail" => {
                    if args["env_null"].as_bool().unwrap_or(false) {
                        // The exe misses at the env pointer, which the native
                        // dereferences *before* it reaches the helper's gate
                        // (mode 1|3, nonzero object word, detail != default):
                        // feeding these inputs to `render_detail` would return
                        // `detail`, not the recorded default. The helper takes
                        // no env word, so the row is counted and skipped
                        // rather than driven — asserted here so the exclusion
                        // stays discriminating if the vector changes.
                        let mode = u32_of(&args["mode"]);
                        assert!(mode == 1 || mode == 3, "{ctx}: mode in the helper window");
                        assert_ne!(u32_of(&args["obj_word"]), 0, "{ctx}: object word");
                        assert_ne!(
                            u32_of(&args["detail"]),
                            DETAIL_DEFAULT,
                            "{ctx}: detail is not the default"
                        );
                        assert_eq!(u32_of(&result["ret"]), DETAIL_DEFAULT, "{ctx}: default");
                        env_null_skipped += 1;
                        continue;
                    }
                    let got = render_detail(
                        u32_of(&args["mode"]),
                        u32_of(&args["obj_word"]),
                        u32_of(&args["detail"]),
                        DETAIL_DEFAULT,
                    );
                    assert_eq!(got, u32_of(&result["ret"]), "{ctx}: detail word");
                }
                "gate_phys_setmatrix" => {
                    let slot = u32_of(&args["slot"]);
                    let pos_addr = u32_of(&result["pos_addr"]);
                    let ori_addr = u32_of(&result["ori_addr"]);
                    let base = pos_addr.wrapping_sub(phys_slot_addr(0, slot));
                    match body_base {
                        Some(b) => assert_eq!(base, b, "{ctx}: body base differs"),
                        None => body_base = Some(base),
                    }
                    assert_eq!(
                        phys_slot_addr(base, slot),
                        pos_addr,
                        "{ctx}: body + slot*0xd4"
                    );
                    assert_eq!(
                        ori_addr.wrapping_sub(pos_addr),
                        ORI_FROM_POS,
                        "{ctx}: orientation offset"
                    );
                    // The position words the exe wrote at the resolved slot.
                    let expect = words(&result["expect_pos"]);
                    assert_eq!(words(&result["pos"]), expect, "{ctx}: position fields");
                    let writes = result["writes"].as_array().expect("writes");
                    for (i, w) in expect.iter().enumerate() {
                        let at = pos_addr + 4 * i as u32;
                        assert!(
                            writes
                                .iter()
                                .any(|p| u32_of(&p[0]) == at && u32_of(&p[1]) == *w),
                            "{ctx}: write of {w:#010x} at {at:#010x}"
                        );
                    }
                }
                other => panic!("unhandled gate suite {other}"),
            }
            total += 1;
        }
    }
    eprintln!(
        "res_oracle gate: {total} rows replayed over {suites} suites \
         ({env_null_skipped} env-null getDetail rows skipped)"
    );
}
