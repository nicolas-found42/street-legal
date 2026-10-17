// Build script: compile the vendored Berkeley SoftFloat 3e sources with the
// same configuration as the M0-07 oracle (docs/x87.md "Tool versions").
//
// Source: vendor/softfloat3e/source (+ 8086-SSE specialize). Preserved
// verbatim from upstream release 3e, including COPYING.txt (BSD-3-Clause;
// see vendor/softfloat3e/ATTRIBUTION.md). Only the include search paths
// point at the vendored tree; no source file is patched.
//
// Oracle configuration (must match re/x87probe/README.md):
//   - specialize: 8086-SSE
//   - SOFTFLOAT_ROUND_ODD, INLINE_LEVEL=5, SOFTFLOAT_FAST_INT64,
//     SOFTFLOAT_FAST_DIV32TO16, SOFTFLOAT_FAST_DIV64TO32
//   - tininess before-rounding is set at runtime (softfloat_detectTininess).
use std::path::Path;

// Object groups mirror build/Linux-x86_64-GCC/Makefile (OBJS_PRIMITIVES,
// OBJS_SPECIALIZE, OBJS_OTHERS) exactly: only listed translation units are
// compiled, specialize-listed ones from 8086-SSE/, the rest from source/.
// (Compiling every .c file breaks: threading-model `*M` sources such as
// s_addExtF80M.c need declarations that only exist in the non-FAST_INT64
// configuration.)
const SPECIALIZE: &str = "softfloat_raiseFlags s_f16UIToCommonNaN \
    s_commonNaNToF16UI s_propagateNaNF16UI s_bf16UIToCommonNaN \
    s_commonNaNToBF16UI s_f32UIToCommonNaN s_commonNaNToF32UI \
    s_propagateNaNF32UI s_f64UIToCommonNaN s_commonNaNToF64UI \
    s_propagateNaNF64UI extF80M_isSignalingNaN s_extF80UIToCommonNaN \
    s_commonNaNToExtF80UI s_propagateNaNExtF80UI f128M_isSignalingNaN \
    s_f128UIToCommonNaN s_commonNaNToF128UI s_propagateNaNF128UI";
const PRIMITIVES: &str = "s_eq128 s_le128 s_lt128 s_shortShiftLeft128 \
    s_shortShiftRight128 s_shortShiftRightJam64 s_shortShiftRightJam64Extra \
    s_shortShiftRightJam128 s_shortShiftRightJam128Extra s_shiftRightJam32 \
    s_shiftRightJam64 s_shiftRightJam64Extra s_shiftRightJam128 \
    s_shiftRightJam128Extra s_shiftRightJam256M s_countLeadingZeros8 \
    s_countLeadingZeros16 s_countLeadingZeros32 s_countLeadingZeros64 \
    s_add128 s_add256M s_sub128 s_sub256M s_mul64ByShifted32To128 \
    s_mul64To128 s_mul128By32 s_mul128To256M s_approxRecip_1Ks \
    s_approxRecip32_1 s_approxRecipSqrt_1Ks s_approxRecipSqrt32_1";

/// Parse one `VAR = \ ... $(OBJ)` list from the vendored Makefile so the
/// OBJS_OTHERS set can never drift from upstream.
fn makefile_objs(mk: &str, var: &str) -> Vec<String> {
    let header = format!("{var} =");
    let mut names = Vec::new();
    let mut in_var = false;
    for line in mk.lines() {
        if !in_var {
            if line.starts_with(&header) {
                in_var = true;
            }
            continue;
        }
        let t = line.trim();
        if t.is_empty() || (!line.starts_with(' ') && !line.starts_with('\t')) {
            break;
        }
        for tok in t.split_whitespace() {
            if let Some(name) = tok.strip_suffix("$(OBJ)") {
                names.push(name.to_string());
            }
        }
    }
    assert!(!names.is_empty(), "no objects parsed for {var}");
    names
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/softfloat3e");
    let source = root.join("source");
    let specialize = source.join("8086-SSE");
    let build_dir = root.join("build");

    let mk = std::fs::read_to_string(build_dir.join("Makefile")).expect("read vendored Makefile");
    let others = makefile_objs(&mk, "OBJS_OTHERS");

    let mut build = cc::Build::new();
    build
        .flag("-Werror=implicit-function-declaration")
        .define("SOFTFLOAT_FAST_INT64", None)
        .define("SOFTFLOAT_ROUND_ODD", None)
        .define("INLINE_LEVEL", "5")
        .define("SOFTFLOAT_FAST_DIV32TO16", None)
        .define("SOFTFLOAT_FAST_DIV64TO32", None)
        .define("LITTLEENDIAN", "1")
        .include(&build_dir)
        .include(&specialize)
        .include(source.join("include"))
        // Silence warnings from third-party C sources; they are vendored
        // verbatim and must not be patched for style.
        .flag("-Wno-unused-but-set-variable")
        .flag("-Wno-unused-function")
        .flag("-Wno-array-bounds");

    let mut count = 0;
    for name in SPECIALIZE.split_whitespace() {
        let path = specialize.join(format!("{name}.c"));
        assert!(path.is_file(), "missing vendored file {}", path.display());
        build.file(path);
        count += 1;
    }
    for name in PRIMITIVES.split_whitespace() {
        let path = source.join(format!("{name}.c"));
        assert!(path.is_file(), "missing vendored file {}", path.display());
        build.file(path);
        count += 1;
    }
    for name in &others {
        let path = source.join(format!("{name}.c"));
        assert!(path.is_file(), "missing vendored file {}", path.display());
        build.file(path);
        count += 1;
    }
    assert!(
        count == 20 + 31 + 258,
        "unexpected SoftFloat object count {count}"
    );

    // slr_softfloat_shim.c: global-state accessors plus the
    // fprem-truncating-quotient helper. Compiled with the same flags.
    let shim = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/slr_softfloat_shim.c");
    assert!(shim.is_file(), "missing {}", shim.display());
    build.file(shim);

    build.compile("softfloat3e");

    println!("cargo:rerun-if-changed=vendor/softfloat3e/source");
    println!("cargo:rerun-if-changed=vendor/softfloat3e/build/platform.h");
    println!("cargo:rerun-if-changed=vendor/softfloat3e/build/Makefile");
    println!("cargo:rerun-if-changed=src/slr_softfloat_shim.c");
    println!("cargo:rerun-if-changed=build.rs");
}
