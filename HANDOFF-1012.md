# PR #1012 checkpoint — 2026-10-13

User requested saving this unfinished work to continue on another computer. This is a WIP checkpoint, not a release-ready integration. Read AGENTS.md and CLAUDE.md before continuing. Contributor preference: only @lukashold, no additional name entry.

## Remote state

- Original PR: https://github.com/storytold/pdfcraft/pull/1012
- Contributor correction https://github.com/lukashold/pdfcraft/pull/1 was reviewed and merged. Original PR branch `codex/scan-orientation-ocr` now contains merge commit `13f25e340bfdadf75856a9f35537a808cb019a9a`.
- This checkpoint merges upstream main `7437bde378f43856eada40af25b983812e615e8a`, resolving approximately 29 conflicts. The many changed files against the old PR head mostly come from main; compare against the main parent to review the feature changes.
- Latest fetched upstream main was `b5467d93a0ffaa5e5e6658bccdf8655a3ba689c9`. Its additional changes have NOT been integrated. Fetch and assess again before updating the original PR.
- Checkpoint branch: `codex/ocr-1012-handoff` on `lukashold/pdfcraft`. The unfinished integration has NOT been pushed to the original PR branch.
- Two original PR review threads remain open: `PRRT_kwDOU1UaEs6rNKVt` (crop padding) and `PRRT_kwDOU1UaEs6rNKVv` (Latin/CJK dictionary mask). The correction addresses those concerns; attempts to resolve them returned FORBIDDEN / Resource not accessible by integration. A person with appropriate rights must resolve them. No upstream merge or release has occurred.

## Changes prepared

- Retained correction PR's padded OCR line crops, original text height, Latin dictionary mask excluding CJK/full-width entries while Japanese retains its full dictionary, finite class scoring, and French save/reopen regression.
- Combined main's existing-text suppression with this PR's rotation, deskew, and crop: map all four recognized word corners to original page coordinates before overlap filtering. Preserve `already_on_page` reporting through engine and automation.
- Combined OCR dialog settings, including main's text-pages choice, localization, correction controls, and custom checkbox widgets required by current clippy rules. Retained camera enhancement and native labels.
- Reconciled sys-locale dependencies, translations and asset hashes, attribution, parity references, and roadmap. OCR progress notes are appended at the bottom of ROADMAP.md to reduce conflicts with later upstream progress entries.
- Added `corrected_scan_does_not_duplicate_existing_ocr_text` in `crates/engine/tests/scan_ocr.rs` for rotated/skewed scans with existing OCR text; this passed with real models.

## Validation at checkpoint

Passed: formatting; assets (270 repository assets, 6 bundled, 25 fetched); layers (35 crates); parity reference checks; `cargo clippy --workspace --all-targets -- -D warnings` (final log ended successfully).

`cargo test -p pdfcraft-engine --test scan_ocr`: 12 reported passed, 0 failed. The optional Japanese font case internally skipped because CRAFT_FONTS_DIR was absent. Models were downloaded and verified using `cargo xtask models`. Earlier OCR unit tests reported 15 passed, before the main integration.

**Full workspace tests failed** in `cargo test -p pdfcraft-engine --test ocr_edit`:

- `a_longer_ocr_phrase_grows_rightwards`, line 127: actual `["Project Coordinator Lead", "Lead", "Testing"]`, expected `["Project Coordinator Lead", "Testing"]`.
- `a_rewritten_ocr_phrase_keeps_its_word_widths`, line 142: actual `["John", "Smith Exam", "Example"]`, expected `["John", "Smith", "Exam"]`.
- The other four ocr_edit cases passed. Engine unit tests before these reported 233 passed, 1 ignored. The full workspace did not complete successfully. Root cause and whether these reproduce on upstream main remain uninvestigated.

Wasm gate and OCR dialog screenshot build were stopped at user's checkpoint request; neither completed. No screenshot has been inspected. cargo-deny was not run. Do not describe these as passing.

## Continue on another computer

Clone `https://github.com/lukashold/pdfcraft.git` and check out `codex/ocr-1012-handoff`. Add/fetch `upstream` at `https://github.com/storytold/pdfcraft.git` as needed. The checkpoint is a normal committed merge, with no unresolved index conflicts.

1. Reproduce and investigate the two ocr_edit failures above; compare upstream main if necessary. Integrate current upstream main without discarding the OCR changes.
2. Read the repository's current gates and rerun applicable validation after fixes: formatting, clippy, full workspace tests, assets, layers, parity, wasm, and real OCR dialog visual verification. Fetch models via `cargo xtask models`; configure approved Japanese fonts if validating that optional case.
3. Remove this temporary handoff file before the final PR update. Only push the tested integration to `origin/codex/scan-orientation-ocr` once release checks are satisfied. Upstream maintainers control acceptance and release timing.

Original machine used Windows ARM64, Rust stable 1.99, LLVM 23.1.3 ARM64 and Visual Studio 2022 build tools. LLVM on PATH was required for ring/aws-lc native assembly. Local toolchains, models, binaries, and logs are deliberately not in Git. Local `work/rust-env.ps1` set CARGO_BUILD_JOBS=4, RUST_TEST_THREADS=1, PDFCRAFT_SYSTEM_FONTS=0 plus task-local CARGO_HOME/RUSTUP_HOME and LLVM PATH. Recreate suitable tooling on the new machine; do not copy those absolute paths. Historical Windows test issues from the original PR (symlink privilege and updates executable UAC naming) are not grounds to skip tests without reproducing them.
