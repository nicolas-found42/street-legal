//! `slr-audio-diff <expected.jsonl> <got.jsonl>`: first divergence of two
//! canonical DirectSound streams. Exit 0 identical, 1 diverged, 2 unreadable.

use std::path::Path;
use std::process::ExitCode;

use slr_audio::diff::first_divergence;
use slr_audio::stream::Stream;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: slr-audio-diff <expected.jsonl> <got.jsonl>");
        return ExitCode::from(2);
    }
    let read = |path: &str| Stream::read(Path::new(path));
    let (expected, got) = match (read(&args[0]), read(&args[1])) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            for error in [a.err(), b.err()].into_iter().flatten() {
                eprintln!("error: {error}");
            }
            return ExitCode::from(2);
        }
    };
    match first_divergence(&expected, &got) {
        None => {
            println!("OK: {} events identical", expected.events.len());
            ExitCode::SUCCESS
        }
        Some(divergence) => {
            println!("{divergence}");
            ExitCode::from(1)
        }
    }
}
