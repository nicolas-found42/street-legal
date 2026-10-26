# Archived x87 value experiment

Diagnostic-only extraction from the dirty native-ABI continuation worktree;
[extraction.json](../extraction.json) records original source hashes. The
Bochs-derived `src/intel_trig.rs` is byte-identical to the preserved source.
Its old cross-reference describes its historical context; the current findings
and requirements are in [the investigation](../../../../docs/research/x87-target-investigation.md).

This package has its own Cargo workspace and lockfile, is not publishable, and
is absent from the production dependency graph. Do not add it to the root
workspace or depend on it from a game crate. It builds against the existing
Berkeley SoftFloat vendor tree and shim without editing them. The minimal carrier
and FFI are only for the diagnostic executable. The model has no complete
status/exception contract and supports finite m32 inputs at nearest rounding
only. Preserved source notices and `LICENSE` apply; no MIT/Apache relicensing of
the Bochs-derived model is claimed.

Run from the repository root:

```sh
cargo fmt --manifest-path re/x87probe/experimental/Cargo.toml -- --check
cargo clippy --locked --manifest-path re/x87probe/experimental/Cargo.toml --all-targets -- -D warnings
python3 re/x87probe/prove_model_error.py > re/out/model-proof.json
python3 re/x87probe/compare_intel_trig.py --upstream /path/to/pinned/Bochs --out re/out/x87-model-fresh
clang -arch x86_64 -O0 -Wall -Wextra -Werror re/x87probe/rosetta/trig_stream.c -o re/out/rosetta-trig-fresh
~/slr-oracle-venv/bin/python re/x87probe/reproduce_ambiguity.py \
  --bochs-build re/out/x87-model-fresh \
  --rosetta-probe re/out/rosetta-trig-fresh \
  --rosetta-source re/x87probe/rosetta/trig_stream.c \
  --exe stock/StreetLegal_Redline.exe \
  --primary /path/to/253665-078US.pdf \
  --out re/out/x87-ambiguity-fresh.json
```

Use a fresh model/output path each time. The independently built upstream must
be clean at `d245c751e6a6fab4bf68e72823598a48001adb3c`; its sparse tree needs
`bochs/cpu/{fpu,softfloat3e}` and their headers. The primary PDF must match the
hash in the investigation; its text extraction must exist alongside it with a
`.txt` suffix. The existing oracle Python environment supplies Unicorn, Capstone,
and pefile. Rosetta must already be available. These commands require local
read-only stock inputs; they are not data-free CI or acceptance commands.

The stream probe prints input, raw sine, raw cosine, combined C2, separate sine
SW and cosine SW; stderr records the translation query. It is a Mach-O probe,
not the stock PE under Wine. The ambiguity referee binds its inputs before and
after execution, verifies the model receipt and candidate source hashes, and
uses rational bounds rather than a libm tolerance. It expects two particular
FCOS disagreement witnesses; it is not a general operation-trust validator.
Missing worker/control profiles, arbitrary extF80, complete initial/final FPU
state, and original-consumer parity remain explicit blockers.

All raw results and binaries stay under ignored `re/out/`. Source/reports on this
branch preserve the experiment for later work; no production integration is
recommended. Even zero model mismatches cannot turn this experiment into G11.
