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
use slr_physics::cfg::{table, Ctx};
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
                let mut c = Ctx::new(&mut m, &mut host, k);
                h(&mut c, &buf);
            }));
            if let Err(e) = replay {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "host replay panic".to_string());
                bad.push(format!("{src}:{file}:{} {kw}: {msg}", row["line"],));
                apply_diff(&mut m, row);
                continue;
            }
            if !host.exhausted() {
                bad.push(format!(
                    "{src}:{file}:{} {kw}: handler stopped after fewer host calls than recorded",
                    row["line"]
                ));
                apply_diff(&mut m, row);
                continue;
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
            if got == want {
                matched += 1;
            } else {
                // Symmetric difference of the journal: only the words that disagree, so a
                // systematic offset error reads as "one word at the wrong address".
                let gs: HashSet<(u32, u32)> = got.iter().copied().collect();
                let ws: HashSet<(u32, u32)> = want.iter().copied().collect();
                let mut diff: Vec<(u32, u32)> = gs.symmetric_difference(&ws).copied().collect();
                diff.sort_unstable();
                let detail = format!(" differing={diff:08x?}");
                bad.push(format!(
                    "{src}:{file}:{} {kw}: diff mismatch (got {} words, want {}){detail}",
                    row["line"],
                    got.len(),
                    want.len(),
                ));
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
