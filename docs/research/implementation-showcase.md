# Implementation showcase for external video review

Created 2026-10-22. The delivery is a **12:42.033 silent video with 179 chapters**, intended to accompany original *Street Legal Racing: Redline* footage in an external AI chat with no repository access.

- [Video](../../re/out/implementation-showcase-2026-10-22/IMPLEMENTATION_SHOWCASE.mp4)
- [Portable review package](../../re/out/implementation-showcase-2026-10-22/EXTERNAL_REVIEW_PACKAGE.zip)
- [Ready-to-paste review prompt](external-gameplay-review-prompt.md)
- [Chapter index](../../re/out/implementation-showcase-2026-10-22/CHAPTERS.md)
- [Coverage audit](../../re/out/implementation-showcase-2026-10-22/COVERAGE_AUDIT.md)
- [Source-cited state inventory](implementation-showcase-state-inventory.md)
- [Prompt-pack research and prompt adaptations](external-review-prompt-design.md)

Upload the implementation MP4 and appropriate original-game recording(s), then paste the review prompt. The ZIP includes the video, prompt, chapter indexes, all 20 YouTube source links, upload guide and a text-only coverage ledger; it contains no game source, diagnostics, or original-game videos. The previously downloaded reference collection remains in `re/out/gameplay-comparison-2026-10-22/videos/`. The upload guide identifies the closest declared stock-build reference and its version/overlay limitations.

## Coverage and evidence limits

The recording covers all seven garage tabs and their subviews, catalog scrolling and transactions, all 24 car models, 12 paint choices, tuning and service actions, save/load and error fixtures, day/night cameras, driving controls, all six event starts, pause/forfeit, representative outcomes and Quit. The inventory maps 71 IDs to footage; T08 is partial because button-focus appearance was staged rather than demonstrating physical keyboard navigation. “Every state” means the finite visible screen/mode/action/outcome families, not all continuous or combinatorial configurations.

This shows the currently playable **retired `slr-game` approximation**. The separate exact-port `slr-boot` runtime is unshown. Follow ADR 0006 and the current boot-status documents for that distinction; this capture is not fidelity acceptance for either runtime.

Capture inputs are scripted, with a fixed simulation step of 1/30 second. Money/reputation and selected conditions are seeded to expose guarded states. Some lap, false-start and finish outcomes are directly staged. Each chapter discloses its setup. One fresh-starter rookie drag completes naturally under the built-in AI pilot; other events include explicitly abandoned samples. Physical input, latency, real-time performance, audio, complete natural progression and exact physics are not validated by this video.

## Capture and verification

All capture instrumentation lives in the ignored `re/out/implementation-showcase-2026-10-22/capture-workspace/` source copy. The eight original `crates/slr-game` files match their pre-capture SHA-256 hashes. No production game source was changed for this task. Isolated saves preserve user progress.

`build_plan.py` creates the 179-chapter plan. The copied app records actual game framebuffer pixels into `render-raw.mp4`; `package_video.py --contact-sheet` adds a 144-pixel caption strip below the intact 1280×720 game image, embeds chapters and verifies the export. The final file is H.264/yuv420p, 1280×864, 30 fps, 22,861 frames, with no audio track. The genuine final Quit action lasts one frame; it is retained without artificial padding, and its unreadable caption is omitted and documented in the chapter index.

Verification includes 179 midpoint screenshots, 24 distinct model previews, **62 observed state/action checks with zero failures**, actual transient FINISH-overlay inspection at 12:14.200, chapter/frame-count checks, complete video decoding and visual checks of the final captioned export. The earlier capture with premature night-drag settlement is retained under `attempt-1/`; the delivery uses the corrected recording. See `packaging-validation.json`, `audit-state.json`, `VISUAL_AUDIT.md` and `source-preservation-check.json` in the output directory for receipts. The review prompt itself has not been evaluated by an external video-review model.

Final video SHA-256: `bf88d2e3ad6515a0d7e6744f5b1712ddb2b0d4f56f32ce1579cd98b911b91434`.
