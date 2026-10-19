//! Differential test: PTX header parse + type-1 loader model vs the exe.
//!
//! Replays the Unicorn oracle vectors at `re/out/ptx/ptx_vectors.jsonl`
//! (git-ignored; produced locally by `re/oracle/ptx_vectors.py gen --exe
//! reference/raw/StreetLegal_Redline.bak.exe`). Override the file with
//! `SLR_PTX_VECTORS` (a directory, or the `.jsonl` itself). Skips with a
//! note when absent so `cargo test --workspace` stays green without the
//! exe/venv (CI, fresh worktrees) — same pattern as
//! `crates/slr-vm/tests/value_oracle.rs`.
//!
//! Per row: decode `input_hex`, run [`slr_fmt_texture::parse`], check the
//! header words against the recorded bytes, then replay the loader's init
//! and walk state model using [`slr_fmt_texture::mip_plan`]:
//!
//! - init: `r68_kept`/`r98` == `plan.kept`, `r6c_index` == `min(1, kept)`,
//!   `r70_cur_off` == `16 + first_entry_total + 0x14` (0 when none kept),
//!   `r80_entry_off` == 16, `r20_flags` == 4 when nothing is kept;
//! - the device-allocation event (`w, h, kept, [res+0x54]`) and the two
//!   checksum (`"sum"`) hooks;
//! - walks: `r80_entry_off` of walk N == the previous `r70_cur_off`,
//!   `r70_cur_off` advances by `total + 0x14`, `r6c_index` increments,
//!   `r34_snap` == 1, `r20_flags` == 0x10 (0x14 on the finishing walk,
//!   which also zeroes `r70_cur_off`).
//!
//! **Honest scope:** the init rows are real exe code apart from the faked
//! device slot; the walk rows hook `FUN_00506ad0`/`FUN_00506c70` as stubs, so
//! this covers the advance/finish rule, not JPEG decoding (blocked on a
//! booted/device context).

use std::path::PathBuf;

use slr_fmt_texture::{mip_plan, parse};

/// `res + 0x54` default: `DAT_006188b8` is 3 in the stock image.
const DEFAULT_RES54: u32 = 3;

fn vectors_path() -> PathBuf {
    let p = std::env::var_os("SLR_PTX_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/ptx"));
    if p.is_dir() {
        p.join("ptx_vectors.jsonl")
    } else {
        p
    }
}

fn hex(bytes: &str) -> Vec<u8> {
    assert_eq!(bytes.len() % 2, 0, "odd-length hex");
    (0..bytes.len() / 2)
        .map(|i| u8::from_str_radix(&bytes[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

fn u32_at(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(data[off..off + 4].try_into().expect("4 bytes"))
}

/// Snapshot of the loader's resource object (`res`) fields the oracle records.
struct Snapshot {
    eax: u32,
    kept: u32,
    index: u32,
    cur_off: u32,
    entry_off: u32,
    r98: u32,
    flags: u32,
    snap: u32,
}

fn snapshot(v: &serde_json::Value) -> Snapshot {
    let g = |k: &str| v[k].as_u64().expect(k) as u32;
    Snapshot {
        eax: g("eax"),
        kept: g("r68_kept"),
        index: g("r6c_index"),
        cur_off: g("r70_cur_off"),
        entry_off: g("r80_entry_off"),
        r98: g("r98"),
        flags: g("r20_flags"),
        snap: g("r34_snap"),
    }
}

#[test]
fn ptx_loader_matches_oracle_vectors() {
    let path = vectors_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SLR_ORACLE_SKIPPED: ptx_loader_matches_oracle_vectors");
            eprintln!(
                "SKIP ptx_oracle: {} absent (run re/oracle/ptx_vectors.py gen --exe <stock exe>)",
                path.display()
            );
            return;
        }
    };

    let mut rows = 0usize;
    let mut walks = 0usize;
    let mut clamped = 0usize;
    let mut zero_kept = 0usize;
    let mut stock_rows = 0usize;
    let mut reference_rows = 0usize;
    let mut synthetic_rows = 0usize;

    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("row {line_no}: {e}"));
        let id = row["id"].as_u64().expect("id");
        let source = row["source"].as_str().expect("source");
        let ctx = format!("row {id} ({source})");
        if source.starts_with("stock/") {
            stock_rows += 1;
        } else if source.starts_with("reference/") {
            reference_rows += 1;
        } else {
            synthetic_rows += 1;
        }

        let data = hex(row["input_hex"].as_str().expect("input_hex"));
        let clamp_b4 = row["clamp_b4"].as_u64().expect("clamp_b4") as u32;
        let res54 = row["res54"].as_u64().map_or(DEFAULT_RES54, |v| v as u32);

        // Header words: bytes 4..16 are (mip_count, width, height).
        let mip_count = u32_at(&data, 4);
        let width = u32_at(&data, 8);
        let height = u32_at(&data, 12);

        // The zero-count row is the one container the port rejects; the exe
        // still reaches the device call with kept == 0.
        let parsed = parse(&data);
        if mip_count == 0 {
            assert!(parsed.is_err(), "{ctx}: parse must reject mip_count 0");
        } else {
            let ptx = parsed.unwrap_or_else(|e| panic!("{ctx}: parse: {e}"));
            assert_eq!(ptx.mip_count, mip_count, "{ctx}: mip_count");
            assert_eq!(ptx.width, width, "{ctx}: width");
            assert_eq!(ptx.height, height, "{ctx}: height");

            // Per-entry totals as the oracle recorded them (parse has the
            // same values; this cross-checks the table walk, not just words).
            let result = &row["result"];
            let recorded = result["init"]["entry_totals"].as_array().expect("totals");
            assert_eq!(ptx.mips.len(), recorded.len(), "{ctx}: entry count");
            for (i, (mip, want)) in ptx.mips.iter().zip(recorded).enumerate() {
                assert_eq!(
                    mip.total as u64,
                    want.as_u64().unwrap(),
                    "{ctx}: mip {i} total"
                );
            }
        }

        let plan = mip_plan(mip_count, width, height, clamp_b4);
        let result = &row["result"];
        let init = snapshot(&result["init"]);
        if plan.kept != mip_count {
            clamped += 1;
        }
        if plan.kept == 0 {
            zero_kept += 1;
        }

        // --- init state ---
        assert_eq!(init.eax, 0, "{ctx}: init eax");
        assert_eq!(init.kept, plan.kept, "{ctx}: init r68_kept");
        assert_eq!(init.r98, plan.kept, "{ctx}: init r98");
        assert_eq!(init.index, plan.kept.min(1), "{ctx}: init r6c_index");
        if plan.kept == 0 {
            assert_eq!(init.cur_off, 0, "{ctx}: init r70_cur_off (nothing kept)");
            assert_eq!(
                init.entry_off, 0,
                "{ctx}: init r80_entry_off (nothing kept)"
            );
            assert_eq!(init.flags, 4, "{ctx}: init r20_flags (nothing kept)");
        } else {
            let first = u32_at(&data, 16) as u64;
            assert_eq!(
                init.cur_off as u64,
                16 + first + 0x14,
                "{ctx}: init r70_cur_off"
            );
            assert_eq!(init.entry_off, 16, "{ctx}: init r80_entry_off");
            assert_eq!(init.flags, 0, "{ctx}: init r20_flags");
        }

        // --- init call events: one device allocation, two checksums ---
        let events = result["init_events"].as_array().expect("init_events");
        let tags: Vec<&str> = events
            .iter()
            .map(|e| e[0].as_str().expect("event tag"))
            .collect();
        assert_eq!(tags, ["device", "sum", "sum"], "{ctx}: event tags");
        assert_eq!(
            events[0],
            serde_json::json!(["device", plan.width, plan.height, plan.kept, res54]),
            "{ctx}: device allocation args"
        );

        // --- walk state model ---
        let recorded_walks = result["walks"].as_array().expect("walks");
        if plan.kept == 0 {
            assert!(recorded_walks.is_empty(), "{ctx}: walks with nothing kept");
            rows += 1;
            continue;
        }
        assert_eq!(
            recorded_walks.len() as u32,
            plan.kept,
            "{ctx}: one walk per kept mip"
        );

        let mut cur_off = init.cur_off;
        let mut entry_off = 16u32;
        let mut index = 1u32;
        for (n, wv) in recorded_walks.iter().enumerate() {
            let w = snapshot(wv);
            let finishing = index >= plan.kept;
            if !finishing {
                // A non-final walk re-points the entry at the cursor (the
                // next table entry) before advancing.
                entry_off = cur_off;
            }
            assert_eq!(w.eax, 0, "{ctx}: walk {n} eax");
            assert_eq!(w.kept, plan.kept, "{ctx}: walk {n} r68_kept");
            assert_eq!(w.r98, plan.kept, "{ctx}: walk {n} r98");
            assert_eq!(w.snap, 1, "{ctx}: walk {n} r34_snap");
            assert_eq!(w.entry_off, entry_off, "{ctx}: walk {n} r80_entry_off");

            if !finishing {
                assert_eq!(w.index, index + 1, "{ctx}: walk {n} r6c_index");
                assert_eq!(
                    w.cur_off,
                    cur_off + u32_at(&data, cur_off as usize) + 0x14,
                    "{ctx}: walk {n} r70_cur_off advance"
                );
                assert_eq!(w.flags, 0x10, "{ctx}: walk {n} r20_flags");
                cur_off = w.cur_off;
                index += 1;
            } else {
                // Finishing walk: same index, the entry stays on the last
                // one walked, cursor zeroed, finish flag set.
                assert_eq!(w.index, plan.kept, "{ctx}: walk {n} r6c_index (finish)");
                assert_eq!(w.flags, 0x14, "{ctx}: walk {n} r20_flags (finish)");
                assert_eq!(w.cur_off, 0, "{ctx}: walk {n} r70_cur_off (finish)");
                assert_eq!(n as u32 + 1, plan.kept, "{ctx}: finish is the last walk");
            }
            walks += 1;
        }
        rows += 1;
    }

    assert!(rows > 0, "no rows in {}", path.display());
    // The committed harness produces exactly this many rows; a mismatch means
    // the generator changed and the evidence line must be refreshed.
    assert_eq!(
        rows,
        225,
        "vector row count changed ({}); expected the 225-row harness set",
        path.display()
    );
    assert_eq!(
        (stock_rows, reference_rows, synthetic_rows),
        (185, 9, 31),
        "per-suite row counts changed in {}",
        path.display()
    );
    eprintln!(
        "ptx_oracle: {rows} rows replayed ({stock_rows} stock, {reference_rows} reference, \
         {synthetic_rows} synthetic; {walks} walks, {clamped} clamped plans, {zero_kept} zero-kept) \
         from {}",
        path.display()
    );
}
