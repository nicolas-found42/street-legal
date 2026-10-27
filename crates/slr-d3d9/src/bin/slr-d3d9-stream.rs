//! `slr-d3d9-stream`: canonicalize, compare and inventory D3D9 call streams.
//!
//! ```text
//! slr-d3d9-stream canon     <log.slrlog> [--out canon.jsonl]
//! slr-d3d9-stream compare   <expected.slrlog> <got.slrlog> [--skip-expected N]
//!                           [--skip-got N] [--max-frames N] [--out report.json]
//! slr-d3d9-stream inventory <log.slrlog> [--out inventory.json]
//! ```
//! Exit 0: success (compare: all frames equal); 1: compare found divergence;
//! 2: unreadable input, unknown/unsupported method or bad usage.
use serde_json::json;
use slr_d3d9::stream::{
    canon::Canonicalizer,
    compare::{compare, Options},
    inventory, CANON_SCHEMA,
};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    process::ExitCode,
};

fn open(path: &str) -> Result<BufReader<File>, String> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|e| format!("{path}: {e}"))
}

fn flag(args: &mut Vec<String>, name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => {
            if i + 1 >= args.len() {
                return Err(format!("{name} needs a value"));
            }
            let value = args.remove(i + 1);
            args.remove(i);
            Ok(Some(value))
        }
    }
}

fn number(args: &mut Vec<String>, name: &str) -> Result<Option<u64>, String> {
    flag(args, name)?
        .map(|v| v.parse().map_err(|e| format!("{name}: {e}")))
        .transpose()
}

fn emit(out: Option<String>, text: &str) -> Result<(), String> {
    match out {
        Some(path) => std::fs::write(PathBuf::from(path), text).map_err(|e| e.to_string()),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Err("usage: slr-d3d9-stream canon|compare|inventory ...".into());
    }
    let command = args.remove(0);
    let out = flag(&mut args, "--out")?;
    match command.as_str() {
        "canon" => {
            let [path] = args.as_slice() else {
                return Err("canon takes one log".into());
            };
            let mut c = Canonicalizer::new(open(path)?).map_err(|e| e.to_string())?;
            let mut sink: Box<dyn Write> = match &out {
                Some(p) => Box::new(BufWriter::new(File::create(p).map_err(|e| e.to_string())?)),
                None => Box::new(std::io::stdout().lock()),
            };
            writeln!(
                sink,
                "{}",
                json!({"schema": CANON_SCHEMA, "kind": "canonical-stream"})
            )
            .map_err(|e| e.to_string())?;
            while let Some(f) = c.next_frame().map_err(|e| e.to_string())? {
                writeln!(
                    sink,
                    "{}",
                    json!({"frame": f.index, "presented": f.presented, "sha256": f.sha256(), "events": f.events})
                )
                .map_err(|e| e.to_string())?;
            }
            writeln!(
                sink,
                "{}",
                json!({"end": true, "stream_sha256": c.stream_sha256(), "d3d9_records": c.tallies.d3d9_records, "other_records": c.tallies.other_records})
            )
            .map_err(|e| e.to_string())?;
            sink.flush().map_err(|e| e.to_string())?;
            Ok(ExitCode::SUCCESS)
        }
        "compare" => {
            let options = Options {
                skip_expected: number(&mut args, "--skip-expected")?.unwrap_or(0),
                skip_got: number(&mut args, "--skip-got")?.unwrap_or(0),
                max_frames: number(&mut args, "--max-frames")?,
            };
            let [expected, got] = args.as_slice() else {
                return Err("compare takes expected and got logs".into());
            };
            let mut e = Canonicalizer::new(open(expected)?).map_err(|e| e.to_string())?;
            let mut g = Canonicalizer::new(open(got)?).map_err(|e| e.to_string())?;
            let report = compare(&mut e, &mut g, &options).map_err(|e| e.to_string())?;
            let equal = report["equal"] == true;
            emit(out, &serde_json::to_string_pretty(&report).expect("json"))?;
            eprintln!(
                "{}: {} of {} compared frames equal; first diverged pair {}",
                if equal { "EQUAL" } else { "DIVERGED" },
                report["frames_equal"],
                report["frames_compared"],
                report["first_diverged_pair"]
            );
            Ok(if equal {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        "inventory" => {
            let [path] = args.as_slice() else {
                return Err("inventory takes one log".into());
            };
            let inv = inventory::scan(open(path)?).map_err(|e| e.to_string())?;
            emit(out, &serde_json::to_string_pretty(&inv).expect("json"))?;
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command {other:?}")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("slr-d3d9-stream: {message}");
            ExitCode::from(2)
        }
    }
}
