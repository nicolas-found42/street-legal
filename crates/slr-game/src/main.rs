// Legacy macroquad frontend, retired by ADR 0006; lint debt is not paid down here.
#![allow(
    clippy::type_complexity,
    clippy::needless_range_loop,
    clippy::too_many_arguments
)]
//! Street Legal Racing: Redline, recreated in Rust from the shipped game data.

mod app;
mod autotest;
mod render3d;
mod session;
mod telemetry;
mod ui;

use app::{App, Args};
use macroquad::prelude::*;
use slr_core::catalog::Catalog;
use std::path::PathBuf;

fn conf() -> Conf {
    Conf {
        window_title: "Street Legal Racing: Redline (Rust)".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

fn parse_args() -> Args {
    let mut reference = std::env::var_os("SLR_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("reference/raw"));
    let mut save_dir = std::env::var_os("SLR_SAVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("saves"));
    let mut autotest = false;
    let mut shots = PathBuf::from("target/autotest");
    let mut time_scale = 1.0;
    let mut diagnostics = None;
    let mut diagnostics_interval = 30;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--reference" => reference = it.next().map(PathBuf::from).unwrap_or(reference),
            "--save-dir" => save_dir = it.next().map(PathBuf::from).unwrap_or(save_dir),
            "--autotest" => autotest = true,
            "--shots" => shots = it.next().map(PathBuf::from).unwrap_or(shots),
            "--time-scale" => time_scale = it.next().and_then(|s| s.parse().ok()).unwrap_or(1.0),
            "--diagnostics" => {
                diagnostics = it.next().map(PathBuf::from).or(diagnostics);
            }
            "--diagnostics-interval" => {
                diagnostics_interval = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(diagnostics_interval);
            }
            "--help" | "-h" => {
                println!("street-legal [--reference DIR] [--save-dir DIR] [--autotest [--shots DIR] [--time-scale N]] [--diagnostics DIR [--diagnostics-interval N]]");
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    Args {
        reference,
        save_dir,
        autotest,
        shots,
        time_scale,
        diagnostics,
        diagnostics_interval,
    }
}

/// M7-04: the original's CD check is not ported; refuse to start unless the clean stock install
/// (`SLR_STOCK`, default `stock/`) verifies against its checksum manifest.
fn require_clean_stock() {
    let stock = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("stock"));
    match slr_io::stock::verify_stock(&stock) {
        Ok(r) => eprintln!(
            "stock install verified: {} files, {} bytes ({})",
            r.files,
            r.bytes,
            stock.display()
        ),
        Err(e) => {
            eprintln!("street-legal: {e}");
            std::process::exit(1);
        }
    }
}

#[macroquad::main(conf)]
async fn main() {
    require_clean_stock();
    let args = parse_args();
    let (cat, loaded) = match Catalog::load(&args.reference) {
        Ok(c) if !c.cars.is_empty() && !c.tyres.is_empty() => (c, true),
        Ok(_) | Err(_) => {
            eprintln!(
                "note: reference install not found at {}; using fallback content",
                args.reference.display()
            );
            (Catalog::fallback(), false)
        }
    };
    let font = load_ttf_font("/System/Library/Fonts/Supplemental/Arial.ttf")
        .await
        .ok();
    let autotest_on = args.autotest;
    let mut app = App::new(args, cat, loaded, font);
    if let Err(error) = app.start_diagnostics() {
        eprintln!("street-legal: {error}");
        std::process::exit(1);
    }
    let mut test = autotest_on.then(autotest::Autotest::new);
    loop {
        if let Some(t) = test.as_mut() {
            t.tick(&mut app);
        }
        app.frame();
        if let Some(t) = test.as_mut() {
            t.post_frame(&app);
        }
        if app.quit {
            break;
        }
        next_frame().await;
    }
    if let Some(t) = test {
        let ok = t.report();
        app.diagnostics.autotest(&t.passes, &t.failures);
        app.finish_diagnostics((!ok).then_some("autotest failures"));
        std::process::exit(if ok { 0 } else { 1 });
    }
    app.finish_diagnostics(None);
}
