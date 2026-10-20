//! Read-only, numbered text view of every decoded call-log record.
use slr_calllog::LogReader;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::process::ExitCode;

fn inspect(path: &std::ffi::OsStr) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = LogReader::open(BufReader::new(File::open(path)?))?;
    let mut output = BufWriter::new(std::io::stdout().lock());
    loop {
        let ordinal = reader.ordinal();
        let Some(record) = reader.next_record()? else {
            break;
        };
        writeln!(output, "#{ordinal} {record:?}")?;
    }
    output.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: slr-calllog-inspect <log.slrlog>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: slr-calllog-inspect <log.slrlog>");
        return ExitCode::from(2);
    }
    match inspect(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", std::path::Path::new(&path).display());
            ExitCode::from(2)
        }
    }
}
