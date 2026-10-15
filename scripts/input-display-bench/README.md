# Input display host benchmark

Compares the frozen git baseline of `src/training/ui/input_log.rs` with the current worktree file. The harness compiles those renderer sources with `include!` and host shims. It does not build the product crate, start an emulator, or measure FPS.

The baseline fixture is git blob `b78a0b6ac9667673418c3255f3bcc585e4685039` from commit `17c4e3f9b257000b17d099b4ed42b79448b5dcfd`. `run.ps1` checks that hash before compiling.

## Run

```powershell
/usr/bin/pwsh -NoLogo -NoProfile -NonInteractive -Command '& ./scripts/input-display-bench/run.ps1'
```

Rustc is the pinned host toolchain `nightly-2026-02-14-x86_64-unknown-linux-gnu`. Transcripts and `report.md` are written under `/tmp/input-display-bench/`.

## What is real and what is a fixture

The included file is the renderer. Pane lookup is a depth-first name walk over one root, one `TrModInputLog`, 15 rows, and each row's `Frame`, `Status`, and five slots of 37 icons (2897 panes). Child order is Frame, Status, then Input0 through Input4. That order is part of the visit count.

`InputLog` button and stick mapping is a fixture bitset, not the game's controller mapper. Status strings are `k{kind}/s{status}`, not the real status table. CPU time is host `Instant` at opt-level 3. It is not Switch frame time.

`fade_out` uses the same alpha curve as `src/training/ui/mod.rs`. The candidate build calls `pub fn invalidate()`; the baseline build does not have that function.

History is populated through renderer calls before measurements; this models rows that previously appeared at the head of the input log. Setup and this warm-up are outside timed regions. Timings cover each scenario body (which can contain multiple frames), not a single game frame. Allocation counts cover Rust allocations in the harness; the native lookup wrapper's temporary C strings and real status-name resolution are not modelled.

The comparison checks visible pane state, not captured screenshots. It normalises positive/negative zero positions and matrix-dirty flags. Hidden child dumps may differ. After same-address layout recreation, the candidate must restore its established history; a control without `invalidate()` must fail to restore it. The old renderer only repaints the newest row in that case.
