//! `slr-calllog-diff`: report the first divergence between two call logs.
//!
//! Usage: `slr-calllog-diff [--mask-refs <native_api.tsv>] <expected.slrlog> <got.slrlog>`
//!
//! `--mask-refs` zeroes native-call arguments and results whose declared type
//! (from the exe's native table, `class<TAB>name<TAB>sig<TAB>label`) is a
//! reference: object and String words are allocator-dependent addresses in the
//! original and heap words in the port, so they cannot be compared bit for bit.
//! Primitive values are never masked, except results of the wall-clock natives
//! (`System.currentTime`, `System.simTime`), which depend on when the run
//! happened and are masked together with the references.
//!
//! Exit 0 when the logs are identical, 1 on any divergence, 2 on I/O or
//! decode errors (with the [`LogError`](slr_calllog::LogError) on stderr).

use slr_calllog::{DiffOutcome, Divergence, LogError, LogReader, LogRecord, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::ExitCode;

fn load(path: &PathBuf) -> Result<Vec<slr_calllog::LogRecord>, LogError> {
    let file = File::open(path)?;
    let mut reader = LogReader::open(BufReader::new(file))?;
    let mut records = Vec::new();
    while let Some(r) = reader.next_record()? {
        records.push(r.clone());
    }
    Ok(records)
}

/// Split a descriptor's parameter list into kind characters (`I`, `F`, `L`, `[`).
fn param_kinds(sig: &str) -> Option<(Vec<char>, char)> {
    let inner = sig.strip_prefix('(')?;
    let (params, ret) = inner.split_once(')')?;
    let mut kinds = Vec::new();
    let mut chars = params.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'L' => {
                for d in chars.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
                kinds.push('L');
            }
            '[' => {
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.next() == Some('L') {
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                kinds.push('[');
            }
            other => kinds.push(other),
        }
    }
    Some((kinds, ret.chars().next().unwrap_or('V')))
}

type Signatures = HashMap<(String, String, usize), Vec<(Vec<char>, char)>>;

fn load_signatures(path: &str) -> std::io::Result<Signatures> {
    let mut map: Signatures = HashMap::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let mut cols = line.split('\t');
        let (Some(class), Some(name), Some(sig)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        if let Some((kinds, ret)) = param_kinds(sig) {
            map.entry((class.to_owned(), name.to_owned(), kinds.len()))
                .or_default()
                .push((kinds, ret));
        }
    }
    Ok(map)
}

fn mask(records: &mut [LogRecord], signatures: &Signatures) {
    for record in records {
        let LogRecord::NativeCall {
            class,
            name,
            args,
            result,
        } = record
        else {
            continue;
        };
        if class == "java.lang.System" && matches!(name.as_str(), "currentTime" | "simTime") {
            *result = result.as_ref().map(|_| Value::Int(0));
        }
        let Some(candidates) = signatures.get(&(class.clone(), name.clone(), args.len())) else {
            continue;
        };
        for (i, arg) in args.iter_mut().enumerate() {
            if candidates
                .iter()
                .any(|(kinds, _)| matches!(kinds.get(i), Some('L' | '[')))
            {
                *arg = Value::Int(0);
            }
        }
        if let Some(value) = result {
            if candidates.iter().any(|(_, ret)| matches!(ret, 'L' | '[')) {
                *value = Value::Int(0);
            }
        }
    }
}

fn main() -> ExitCode {
    let mut argv: Vec<String> = std::env::args().collect();
    let mut signatures = None;
    if argv.len() > 2 && argv[1] == "--mask-refs" {
        match load_signatures(&argv[2]) {
            Ok(s) => signatures = Some(s),
            Err(e) => {
                eprintln!("error reading {}: {e}", argv[2]);
                return ExitCode::from(2);
            }
        }
        argv.drain(1..3);
    }
    if argv.len() != 3 {
        eprintln!(
            "usage: slr-calllog-diff [--mask-refs <native_api.tsv>] <expected.slrlog> <got.slrlog>"
        );
        return ExitCode::from(2);
    }
    let expected_path = PathBuf::from(&argv[1]);
    let got_path = PathBuf::from(&argv[2]);
    let mut expected = match load(&expected_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error reading {}: {e}", expected_path.display());
            return ExitCode::from(2);
        }
    };
    let mut got = match load(&got_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error reading {}: {e}", got_path.display());
            return ExitCode::from(2);
        }
    };
    if let Some(signatures) = &signatures {
        mask(&mut expected, signatures);
        mask(&mut got, signatures);
    }
    match slr_calllog::first_divergence(&expected, &got) {
        DiffOutcome::Ok { records } => {
            println!("OK: {records} records identical");
            ExitCode::from(0)
        }
        DiffOutcome::Diverged(Divergence::LengthMismatch {
            common,
            expected_len,
            got_len,
        }) => {
            let exp = expected_len.map_or("?".to_owned(), |n| n.to_string());
            let got_n = got_len.map_or("?".to_owned(), |n| n.to_string());
            println!(
                "DIVERGENCE after record #{common}: length mismatch: expected {exp} records ({}), got {got_n} records ({})",
                expected_path.display(),
                got_path.display(),
            );
            ExitCode::from(1)
        }
        DiffOutcome::Diverged(d) => {
            println!("{d}");
            ExitCode::from(1)
        }
    }
}
