# G11 stream tooling

Diagnostic and validator inputs for obligation G11 (x87 float parity). Nothing here is linked into
the workspace; the Bochs checkout and every output live outside the repository.

| file | role |
|---|---|
| `xcorpus.py`, `make_corpus.py` | deterministic synthetic operand corpus (21-byte records); no original data |
| `bochs_stream.cc`, `bochs_build.py` | the pinned Bochs FPU transcendentals as a streaming leg |
| `x87_stream.c` | host x87 as a streaming leg: Rosetta 2 on Apple Silicon, **native silicon on an Intel Mac** |
| `adjudicate.py` | pairwise bit-for-bit comparison of any streamed legs (80-bit and f32-rounded) |
| `run_compare.py`, `xanalyze.py`, `xcompare.py` | agreement against a 200-bit mpmath reference (needs `~/slr-oracle-venv`) |
| `build_harnesses.sh` | builds `x87-stream` and, with `BOCHS=...`, `bochs-stream` |

The shipped implementation is the fourth leg: `cargo run -p slr-x87 --example stream --release`.
`acceptance/g11.py` runs all of it fresh and derives the trust table. For the Intel-hardware kit and
the decision it unlocks see `docs/research/x87-g11-decision.md`.
