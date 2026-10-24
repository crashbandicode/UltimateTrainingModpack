# Revision 32 — smaller release profile (2026-10-13)

The release profile uses `opt-level = "z"` and `codegen-units = 1`,
shrinking the NRO from 3,379,200 to 2,174,976 bytes with no source change. The
profile is still in `Cargo.toml` as of 2026-10-24.

The console freezes of 2026-10-13 are read by the Deluxe author as the game heap running out: the crash reports
fault at address zero in unrelated game threads with no plugin on any stack, and every plugin hook sits idle while
the game never produces a frame. Skyline plugins live in that heap, and Skyline takes the whole NRO file plus the
bss into it, so every plugin's file size is part of one shared budget. Deluxe 0.3.1 had moved a 65536-event trace
into .bss (2.10 → 3.98 MB, fixed in 0.3.3). The Training Modpack's share was the research builds themselves:

| build | NRO file | .text | .ro | .data | bss |
|---|---|---|---|---|---|
| 32w release (bb3a486) | 2,494,464 | 1,548,288 | 819,200 | 126,976 | 28,672 |
| revision 31 trace build (68b3334) | 3,379,200 | 2,371,584 | 864,256 | 143,360 | 28,672 |

The growth is code only. `llvm-nm` attributes about two thirds of it to the `.smd` shared-capture path
(smash-match-data JSON model, archive and validator; serde_json's `float_roundtrip` lexical code; sha2), which is
resident even when `replay_capture.ini` says `enabled=off`; the rest is the named-macro, replay-capture and
clip-editor code. There are no static buffers; the largest static object is the 24 KB input-recording table.

## What revision 32 changes

`Cargo.toml` `[profile.release]` gains `opt-level = "z"` and `codegen-units = 1`. No source file changes; the
hooks, anchors, offsets, probes and heartbeat are revision 31's. Measured on the revision 31 sources with the
pinned toolchain, same features (`waiting-roster-reload,deluxe-sdk`), fat LTO and `panic = "abort"` as before:

| profile | NRO file | .text | vs revision 31 | vs 32w |
|---|---|---|---|---|
| opt-level 3 (shipped revision 31) | 3,379,200 | 2,371,584 | 0 | +884,736 |
| codegen-units 1 | 3,346,432 | 2,351,104 | −32,768 | +851,968 |
| opt-level "s" | 2,621,440 | 1,626,112 | −757,760 | +126,976 |
| opt-level "s", codegen-units 1 | 2,646,016 | 1,654,784 | −733,184 | +151,552 |
| opt-level "z" | 2,211,840 | 1,200,128 | −1,167,360 | −282,624 |
| **opt-level "z", codegen-units 1** | **2,174,976** | **1,171,456** | **−1,204,224** | **−319,488** |

The chosen profile makes the plugin smaller than the 32w release that last worked on the console beside Deluxe
0.3.0. Evidence: `research/../artifacts/footprint-experiments/` (results.json, one NRO and ELF per variant) and
`artifacts/switch-r31-smoke-1/NOTES.md` "Footprint".

## What to watch

- `"z"` disables loop vectorisation and most inlining, so the per-frame hooks cost somewhat more CPU. The modpack's
  per-frame work is small (input mapping, UI tick, probe atomics); the heartbeat's `ui_pass`/`tick_pass` (+120/s)
  and `roster_service` (+240/s with four entries) rates must stay the same as revision 31's.
- Unsafe code that happened to work at opt-level 3 can behave differently at another level. Revision 32 was not
  run on Eden before the card (same precedent as revision 31); a panic or missing HUD on the console points here.
- Rollback: the revision 31 NRO (`381046f2…`, 3,379,200 B) and the 32w NRO (`4610b842…`, 2,494,464 B) are kept
  locally and in the card's handoff folders.

## Left for later

A cargo feature that drops the `.smd` shared-capture path from console builds that do not capture (~0.5 MB at
opt-level 3), a slimmer `.smd` writer without the read-back validator (smash-match-data design), and releasing the
decompressed layout archives between scene loads (4.2 MB each, pre-existing behaviour).
