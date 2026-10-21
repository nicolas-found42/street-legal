//! Differential test: the cfg keyword handlers against the exe's own handlers, executed in
//! Unicorn (`re/oracle/cfg_handler_vectors.py gen handlers`).
//!
//! Oracle: **Unicorn function oracle**, control word 0x007F (x87 PC=24, the game's setting).
//! For every line of every stock+modded cfg whose keyword is ported: load the `init.json`
//! fixture into a [`Space`](slr_physics::space::Space), run the port's table handler with the
//! recorded raw args buffer, replay the recorded host calls through
//! [`ReplayHost`](slr_physics::cfg::test_support::ReplayHost), and compare the changed words
//! with the recorded `diff` bit for bit. Skipped rows are re-applied from their recorded
//! `diff` and fast-forwarded to their recorded heap pointer, so later lines replay on the
//! exact oracle state. Lines flagged `nondet` read uninitialised stack and are counted, not
//! compared. The test SKIPs when vectors are absent.

use slr_physics::cfg::test_support::{load_init, parse_call, ReplayHost};
use slr_physics::cfg::{table, Ctx, Handler};
use slr_physics::space::Space;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_CFG_HANDLER_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/cfg_handlers")
        })
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Keywords with no port yet (in exe table order). The test fails loudly on any keyword
/// outside this list that has no handler, so newly ported handlers shrink it.
/// Currently empty: every table keyword is ported.
const KNOWN_UNPORTED: &[&str] = &[];

/// Replay one recorded handler line (oracle `args`/`calls`/`diff` row) against the port
/// handler `h` on `m` at car-class object `k`, and compare the changed words bit for bit.
/// Returns the mismatch description on failure; the caller owns skip/reapply policy.
fn replay_row(m: &mut Space, k: u32, row: &serde_json::Value, h: Handler) -> Result<(), String> {
    let calls = row["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(parse_call)
        .collect::<Vec<_>>();
    let mut host = ReplayHost::new(calls);
    let buf = unhex(row["args"].as_str().unwrap());
    m.begin_journal();
    let replay = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut c = Ctx::new(m, &mut host, k);
        h(&mut c, &buf);
    }));
    if let Err(e) = replay {
        return Err(e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "host replay panic".to_string()));
    }
    if !host.exhausted() {
        return Err("handler stopped after fewer host calls than recorded".to_string());
    }
    let mut got: Vec<(u32, u32)> = m.end_journal().into_iter().collect();
    got.sort_unstable();
    let mut want: Vec<(u32, u32)> = row["diff"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32))
        .collect();
    want.sort_unstable();
    if got != want {
        // Symmetric difference of the journal: only the words that disagree, so a
        // systematic offset error reads as "one word at the wrong address".
        let gs: HashSet<(u32, u32)> = got.iter().copied().collect();
        let ws: HashSet<(u32, u32)> = want.iter().copied().collect();
        let mut diff: Vec<(u32, u32)> = gs.symmetric_difference(&ws).copied().collect();
        diff.sort_unstable();
        return Err(format!(
            "diff mismatch (got {} words, want {}): differing={diff:08x?}",
            got.len(),
            want.len()
        ));
    }
    Ok(())
}

#[test]
fn cfg_handlers_match_unicorn_vectors() {
    let dir = vectors_dir();
    let init_text = match std::fs::read_to_string(dir.join("init.json")) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SLR_ORACLE_SKIPPED: cfg_handlers_match_unicorn_vectors");
            eprintln!(
                "SKIP: no oracle vectors (run re/oracle/cfg_handler_vectors.py gen handlers)"
            );
            return;
        }
    };
    let init: serde_json::Value = serde_json::from_str(&init_text).unwrap();
    let rows: Vec<(String, String, Vec<serde_json::Value>)> =
        ["handlers_stock.jsonl", "handlers_modded.jsonl"]
            .iter()
            .filter_map(|name| std::fs::read_to_string(dir.join(name)).ok())
            .flat_map(|text| {
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| serde_json::from_str(l).unwrap())
                    .collect::<Vec<serde_json::Value>>()
            })
            .map(|r| {
                (
                    r["src"].as_str().unwrap().to_string(),
                    r["file"].as_str().unwrap().to_string(),
                    r["lines"].as_array().unwrap().clone(),
                )
            })
            .collect();
    if rows.is_empty() {
        eprintln!("SLR_ORACLE_SKIPPED: cfg_handlers_match_unicorn_vectors");
        eprintln!("SKIP: no oracle vectors (run re/oracle/cfg_handler_vectors.py gen handlers)");
        return;
    }
    let handlers: HashMap<&str, _> = table().into_iter().collect();
    let unported: HashSet<&str> = KNOWN_UNPORTED.iter().copied().collect();

    let mut files = 0usize;
    let mut exercised = 0usize;
    let mut matched = 0usize;
    let mut skipped_nondet = 0usize;
    let mut skipped_unported = 0usize;
    let mut surprise_unported: HashSet<String> = HashSet::new();
    let mut distinct: HashSet<String> = HashSet::new();
    let mut bad: Vec<String> = Vec::new();

    for (src, file, lines) in &rows {
        files += 1;
        let mut m = Space::new();
        let (k, _p) = load_init(&mut m, &init);
        for row in lines {
            let kw = row["kw"].as_str().unwrap();
            if kw == "eof" {
                continue;
            }
            if row.get("nondet").is_some() {
                skipped_nondet += 1;
                apply_diff(&mut m, row);
                continue;
            }
            let Some(h) = handlers.get(kw) else {
                skipped_unported += 1;
                if !unported.contains(kw) {
                    surprise_unported.insert(kw.to_string());
                }
                apply_diff(&mut m, row);
                continue;
            };
            exercised += 1;
            distinct.insert(kw.to_string());
            match replay_row(&mut m, k, row, *h) {
                Ok(()) => matched += 1,
                Err(msg) => {
                    bad.push(format!("{src}:{file}:{} {kw}: {msg}", row["line"],));
                    apply_diff(&mut m, row);
                }
            }
            m.set_heap_ptr(row["hp"].as_u64().unwrap() as u32);
        }
    }

    eprintln!(
        "cfg_handlers_oracle: files={files} exercised={exercised} matched={matched} \
         distinct_kw={} nondet_skipped={skipped_nondet} unported_skipped={skipped_unported} \
         uncovered_unexpected={}",
        distinct.len(),
        surprise_unported.len(),
    );
    assert!(
        surprise_unported.is_empty(),
        "keywords with no handler and not on KNOWN_UNPORTED: {:?} (port them or list them)",
        {
            let mut v: Vec<_> = surprise_unported.into_iter().collect();
            v.sort();
            v
        }
    );
    assert!(exercised > 0, "no ported keyword rows exercised");
    if let Ok(path) = std::env::var("SLR_DUMP_MISMATCHES") {
        std::fs::write(path, bad.join("\n")).unwrap();
    }
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

/// Bring `m` onto the oracle state after a row the harness does not replay: apply the
/// recorded nonzero-word diff and fast-forward the heap pointer.
fn apply_diff(m: &mut Space, row: &serde_json::Value) {
    for e in row["diff"].as_array().unwrap() {
        m.w32(e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32);
    }
    let _ = m.end_journal();
    m.set_heap_ptr(row["hp"].as_u64().unwrap() as u32);
}

/// Handler address -> port function, for the leaf vectors (`cfg_handler_vectors.py gen
/// leaves`).  The car table (0x60e0f8) and the part table (0x611368) share addresses for
/// their common keywords, so one address map serves both; the part-only names are the
/// port's `*2` tails (`slotturn` -> `slotmove2`, `intdamage` -> `texture2`,
/// `material`@0x46cff0 -> `material2`, `wing`@0x46d470 -> `mesh2`, `rollbar`@0x46d7a0 ->
/// `lods2`, `sfx_loop` -> `stockpart2`).
fn leaf_handler(addr: u32) -> Option<Handler> {
    use slr_physics::cfg::*;
    let f: Handler = match addr {
        0x442f90 => type_,
        0x4432b0 => friction,
        0x443390 => skid,
        0x4435b0 => arm,
        0x443690 => hub,
        0x4437a0 => instant_center,
        0x4439b0 => wheeldeform,
        0x443ef0 => sw_radius,
        0x443f50 => mass,
        0x4440e0 => torque,
        0x444200 => clutch,
        0x4442b0 => rpm_idle,
        0x444310 => drag,
        0x444570 => center,
        0x444710 => nodriver,
        0x444a60 => key,
        0x444ae0 => handbrake,
        0x445f00 => turn,
        0x445fc0 => velfactor,
        0x446000 => roll,
        0x446050 => pitch,
        0x446100 => cheat_control,
        0x444e00 => osd_gauge,
        0x444e80 => osd_rpmpin,
        0x444f20 => osd_spdpin,
        0x444fc0 => osd_nospin,
        0x445060 => osd_damage,
        0x4450e0 => osd_damage_slot,
        0x445120 => osd_gear,
        0x4451b0 => osd_speed,
        0x445260 => osd_rpm,
        0x445310 => osd_nos,
        0x4453c0 => osd_turbo,
        0x445470 => osd_mileage,
        0x445520 => osd_abs,
        0x4455b0 => osd_asr,
        0x445640 => osd_hbrake,
        0x4456d0 => osd_emergency,
        0x445760 => sfx_engine_up,
        0x445890 => sfx_engine_down,
        0x4459c0 => sfx_exhaust,
        0x445af0 => sfx_gearbox_fwd,
        0x445b80 => sfx_gearbox_rev,
        0x445c10 => sfx_wreck,
        0x445d50 => sfx_gearchange,
        0x46bb80 => body,
        0x46c1a0 => shadow,
        0x46c3a0 => slot,
        0x46c4e0 => bolt,
        0x46c8f0 => flap,
        0x46ca70 => flapsfx,
        0x46cb90 => slotmove2,
        0x46ccd0 => slotmove,
        0x46cf10 => deform,
        0x46cfb0 => texture2,
        0x46cff0 => material2,
        0x46d130 => trigger,
        0x46d400 => logo,
        0x46d470 => mesh2,
        0x46d7a0 => lods2,
        0x46da30 => stockpart2,
        _ => return None,
    };
    Some(f)
}

/// Differential test: the cfg keywords the stock+modded corpus never exercises, against the
/// exe's own handlers over random and edge vectors (`re/oracle/cfg_handler_vectors.py gen
/// leaves`).  Every leaf scenario is a synthetic cfg file run through the real dispatch (the
/// car table for `leaves_car.jsonl`, the part table 0x611368 for `leaves_part.jsonl`), so
/// the rows carry the same `args`/`calls`/`diff` shape as the corpus rows and replay
/// identically.  Rows flagged `nondet` read uninitialised stack in the exe itself (the
/// exe's stale-slot id operands in `arm`/`hub`) and are re-applied, not compared.  The test
/// SKIPs when vectors are absent.
#[test]
fn cfg_leaves_match_unicorn_vectors() {
    let dir = vectors_dir();
    let init_text = match std::fs::read_to_string(dir.join("init.json")) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SLR_ORACLE_SKIPPED: cfg_leaves_match_unicorn_vectors");
            eprintln!("SKIP: no oracle vectors (run re/oracle/cfg_handler_vectors.py gen leaves)");
            return;
        }
    };
    let init: serde_json::Value = serde_json::from_str(&init_text).unwrap();
    // The oracle runs `run_file` once per leaf group, restoring the captured
    // fixture before each. Every group therefore starts from the same heap
    // image and heap pointer; the replay must reset the `Space` per group or
    // the second group onwards sees the previous group's allocations.
    let groups: Vec<(String, serde_json::Value)> = ["leaves_car.jsonl", "leaves_part.jsonl"]
        .iter()
        .filter_map(|name| {
            std::fs::read_to_string(dir.join(name))
                .ok()
                .map(|text| (name.to_string(), text))
        })
        .flat_map(|(name, text)| {
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .map(move |l| {
                    (
                        name.clone(),
                        serde_json::from_str::<serde_json::Value>(l).unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    if groups.is_empty() {
        eprintln!("SLR_ORACLE_SKIPPED: cfg_leaves_match_unicorn_vectors");
        eprintln!("SKIP: no oracle vectors (run re/oracle/cfg_handler_vectors.py gen leaves)");
        return;
    }

    let mut exercised = 0usize;
    let mut matched = 0usize;
    let mut skipped_nondet = 0usize;
    let mut distinct: HashSet<String> = HashSet::new();
    let mut unknown: HashSet<String> = HashSet::new();
    let mut bad: Vec<String> = Vec::new();

    for (name, grp) in &groups {
        let mut m = Space::new();
        let (k, _p) = load_init(&mut m, &init);
        for row in grp["lines"].as_array().unwrap() {
            let kw = row["kw"].as_str().unwrap();
            if kw == "eof" {
                continue;
            }
            if row.get("nondet").is_some() {
                skipped_nondet += 1;
                apply_diff(&mut m, row);
                continue;
            }
            let addr = u32::from_str_radix(
                row["exe_addr"].as_str().unwrap().trim_start_matches("0x"),
                16,
            )
            .unwrap();
            let Some(h) = leaf_handler(addr) else {
                unknown.insert(format!("{name}:{} {kw} @0x{addr:x}", row["line"]));
                apply_diff(&mut m, row);
                continue;
            };
            exercised += 1;
            distinct.insert(kw.to_string());
            match replay_row(&mut m, k, row, h) {
                Ok(()) => matched += 1,
                Err(msg) => {
                    bad.push(format!("{name}:{} {kw}: {msg}", row["line"]));
                    apply_diff(&mut m, row);
                }
            }
            m.set_heap_ptr(row["hp"].as_u64().unwrap() as u32);
        }
    }

    eprintln!(
        "cfg_leaves_oracle: exercised={exercised} matched={matched} distinct_kw={} \
         nondet_skipped={skipped_nondet} unknown={}",
        distinct.len(),
        unknown.len(),
    );
    assert!(
        unknown.is_empty(),
        "leaf rows with no port handler: {unknown:?}"
    );
    assert!(exercised > 0, "no leaf rows exercised");
    if let Ok(path) = std::env::var("SLR_DUMP_MISMATCHES") {
        std::fs::write(path, bad.join("\n")).unwrap();
    }
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

/// Differential test for the six slr-physics cfg helpers that are not keyword handlers
/// (`cfg_handler_vectors.py gen direct`): the exe functions called directly over edge
/// vectors, every written word compared bit for bit.  `stricmp_eq` compares the equality
/// bit (the port's contract; the exe's ±1 ordering is not port-visible).  The test SKIPs
/// when vectors are absent.
#[test]
fn cfg_leaves_direct_match_unicorn_vectors() {
    use slr_physics::cfg::{
        list_head_ctor, list_node_ctor, ref_release, stricmp_eq, vec3_set, vec3_zero,
    };
    let text = match std::fs::read_to_string(vectors_dir().join("leaves_direct.jsonl")) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SLR_ORACLE_SKIPPED: cfg_leaves_direct_match_unicorn_vectors");
            eprintln!("SKIP: no oracle vectors (run re/oracle/cfg_handler_vectors.py gen direct)");
            return;
        }
    };
    let mut matched = 0usize;
    let mut bad: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let id = row["id"].as_str().unwrap().to_string();
        let name = row["name"].as_str().unwrap().to_string();
        let fail = |msg: String| format!("{id} {name}: {msg}");
        if name == "stricmp_eq" {
            let a = unhex(row["a"].as_str().unwrap());
            let b = unhex(row["b"].as_str().unwrap());
            let want = row["eax"].as_u64().unwrap() == 0;
            let got = stricmp_eq(&a, &b);
            if got == want {
                matched += 1;
            } else {
                bad.push(fail(format!(
                    "equality mismatch: a={a:?} b={b:?} exe_eq={want} port_eq={got}"
                )));
            }
            continue;
        }
        let addr = row["addr"].as_u64().unwrap() as u32;
        let n = row["n_words"].as_u64().unwrap() as u32;
        let in_words: Vec<u32> = row["in_words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        let want_words: Vec<u32> = row["out_words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        let args: Vec<u32> = row
            .get("args")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let this = row.get("this").and_then(|t| t.as_u64()).map(|t| t as u32);
        let mut m = Space::new();
        for (i, w) in in_words.iter().enumerate() {
            m.w32(addr + 4 * i as u32, *w);
        }
        m.begin_journal();
        match name.as_str() {
            "list_head_ctor" => list_head_ctor(&mut m, addr),
            "list_node_ctor" => list_node_ctor(&mut m, addr),
            "vec3_zero" => vec3_zero(&mut m, addr),
            "vec3_set" => vec3_set(&mut m, addr, args[0], args[1], args[2]),
            "ref_release" => ref_release(&mut m, this.expect("ref_release row records its slot")),
            other => {
                bad.push(fail(format!("unknown direct function {other}")));
                continue;
            }
        }
        // The helper must write only inside its own cell: every journalled word in range.
        let out_of_cell: Vec<(u32, u32)> = m
            .end_journal()
            .into_iter()
            .filter(|(a, _)| !(addr..addr + 4 * n).contains(a))
            .collect();
        if !out_of_cell.is_empty() {
            bad.push(fail(format!(
                "writes outside the {n}-word cell: {out_of_cell:08x?}"
            )));
            continue;
        }
        let got_words: Vec<u32> = (0..n).map(|i| m.r32(addr + 4 * i)).collect();
        if got_words == want_words {
            matched += 1;
        } else {
            let differing: Vec<usize> = got_words
                .iter()
                .zip(&want_words)
                .enumerate()
                .filter_map(|(i, (g, w))| (g != w).then_some(i))
                .collect();
            bad.push(fail(format!(
                "word mismatch at offsets {:08x?}: got {got_words:08x?}, want {want_words:08x?}",
                differing
                    .iter()
                    .map(|i| format!("+{:x}", 4 * i))
                    .collect::<Vec<_>>()
            )));
        }
    }

    eprintln!(
        "cfg_leaves_direct_oracle: matched={matched} bad={}",
        bad.len()
    );
    assert!(matched > 0, "no direct rows exercised");
    if let Ok(path) = std::env::var("SLR_DUMP_MISMATCHES") {
        std::fs::write(path, bad.join("\n")).unwrap();
    }
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}
