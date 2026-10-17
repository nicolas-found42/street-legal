//! `slr-calllog-diff`: report the first divergence between two call logs.
//!
//! Usage: `slr-calllog-diff <expected.slrlog> <got.slrlog>`
//!
//! Exit 0 when the logs are identical, 1 on any divergence, 2 on I/O or
//! decode errors (with the [`LogError`](slr_calllog::LogError) on stderr).

use slr_calllog::{DiffOutcome, Divergence, LogError, LogReader};
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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() != 3 {
        eprintln!("usage: slr-calllog-diff <expected.slrlog> <got.slrlog>");
        return ExitCode::from(2);
    }
    let expected_path = PathBuf::from(&argv[1]);
    let got_path = PathBuf::from(&argv[2]);
    let expected = match load(&expected_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error reading {}: {e}", expected_path.display());
            return ExitCode::from(2);
        }
    };
    let got = match load(&got_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error reading {}: {e}", got_path.display());
            return ExitCode::from(2);
        }
    };
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
