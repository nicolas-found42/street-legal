//! `slr-census <install-root>`: print the G2 whole-install census as JSON and
//! exit 1 when any file is neither decoded nor explained.

fn main() {
    let Some(root) = std::env::args_os().nth(1) else {
        eprintln!("usage: slr-census <install-root>");
        std::process::exit(2);
    };
    let census = match slr_formats::census::run(std::path::Path::new(&root)) {
        Ok(census) => census,
        Err(error) => {
            eprintln!("slr-census: {error}");
            std::process::exit(2);
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&census.to_json()).expect("census json")
    );
    std::process::exit(i32::from(!census.unexplained.is_empty()));
}
