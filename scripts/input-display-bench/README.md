# Input display host benchmark

Compares the frozen git baseline of `src/training/ui/input_log.rs` with the current worktree file. The harness compiles those renderer sources with `include!` and host shims. It does not build the product crate, start an emulator, or measure FPS.

The baseline fixture is the 7.1.2 renderer, git blob `9c1bcc221da0be1faf9d429ceeec4db090de0f09`. `run.ps1` checks that hash before compiling. The older renderer remains at `fixtures/pre-7.1.2-input_log.rs` and is not compiled. Both the 7.1.2 baseline and the candidate export `invalidate()`, and the harness calls it for each.

## Run

```powershell
/usr/bin/pwsh -NoLogo -NoProfile -NonInteractive -Command '& ./scripts/input-display-bench/run.ps1'
```

Rustc is the pinned host toolchain `nightly-2026-02-14-x86_64-unknown-linux-gnu`. Transcripts and `report.md` are written under `/tmp/input-display-bench/`.

## What is real and what is a fixture

The included file is the renderer. Pane lookup is a depth-first name walk over one root, one `TrModInputLog`, 15 rows, and each row's `Frame`, `Status`, and five slots of 37 icons (2897 panes). Child order is Frame, Status, then Input0 through Input4. That order is part of the visit count.

`InputLog` button and stick mapping is a fixture bitset, not the game's controller mapper. Status strings are `k{kind}/s{status}`, not the real status table. CPU time is host `Instant` at opt-level 3. It is not Switch frame time.

`fade_out` uses the same alpha curve as `src/training/ui/mod.rs`. `ttl_decay` decrements every live ttl for three frames after warm-up. `ring_rotate` advances the ring three times. `late_head` ticks the head frame count while that head is `TrModInputLog14`. These are host pane counts, not FPS.

History is populated through renderer calls before measurements; this models rows that previously appeared at the head of the input log. Setup and this warm-up are outside timed regions. Timings cover each scenario body (which can contain multiple frames), not a single game frame. Allocation counts cover Rust allocations in the harness; the native lookup wrapper's temporary C strings and real status-name resolution are not modelled.

The comparison checks visible pane state, not captured screenshots. It normalises positive/negative zero positions and matrix-dirty flags. Hidden child dumps may differ. After same-address layout recreation, both the 7.1.2 baseline and candidate must restore their established history; a candidate control without `invalidate()` must fail to restore it.
