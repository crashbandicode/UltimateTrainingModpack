# Experimental enhancements alpha — UNSTABLE

This opt-in diagnostic alpha targets Smash Ultimate 13.0.5. Intermittent console
crashes remain under investigation. It is not a stable release or proof that the
underlying native-memory corruption has been fixed.

The NRO includes character cycling/manual swapping, Solo Popo, Kirby hats,
character items, direct practice stages and lobby stage rotation, and saved-Mii
cycling. Native Mii-editor and managed Mii-helper feature gates remain **disabled
in this diagnostic NRO**. The source contains those separate experiments.

## Changes in this alpha

- Original P1 Ice Climbers uses Character Extras → Solo Popo; numbered Ice
  Climbers uses the independent red S. Opposite configurations can share a cycle,
  and returning through slot 0 restores the original configuration. The original
  portrait is grey only when both configurations match. CPU's blue S is unchanged.
- Practice Stages includes the actual Training stage as well as Waiting Room.
  L/R changes pages. Direct selection works in Training; rotation remains for
  lobby practice.
- Two optional [INI menu shortcuts](menu-shortcuts.md), initially Minus and
  disabled. B + D-pad Up is permanent. Plus opens normal pause unless explicitly
  configured as a mod-menu shortcut.
- Shared native-field checks cover selected cleanup and Mii pointer chains.
  They check null parents, offsets and alignment; they do not cover every unsafe
  access or prove that a non-null pointer is alive. Native object lifetime and
  synchronization still belong to the caller. Mii callbacks also avoid redundant
  large copies and unnecessary work when inactive.

The existing exact-null cleanup and Lua garbage-collector guards remain. The
malformed-reference writer is unknown; non-null dangling pointers are outside
this containment. Native instruction checks refuse mismatched patch windows.

## Install

Download `libtraining_modpack-UNSTABLE.nro` from the [experimental release](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack/releases/tag/v7.1.2-enhancements-experimental.20261024).
Back up the current NRO, rename the download to `libtraining_modpack.nro`, and put
it in `atmosphere/contents/01006A800016E000/romfs/skyline/plugins/` on the SD card.
Retain Skyline, ARCropolis, `libnro_hook.nro`, and `libparam_hook.nro`. This download
does not replace other mods or their settings.

Focused diagnostic capture starts automatically in
`ultimate/TrainingModpack/practice-trace-swap.bin`, bounded below 4 GiB. Small
native unlink and diagnostic logs are retained alongside it. Existing Trace
Control users can stop/start capture. This build does not write the former broad
`practice-trace-controlled.bin` recording.

## What was checked

- Native all-target Clippy: zero warnings; no-default and optimized release builds
  passed with source identities checked before and after.
- 18 focused Cargo tests and 8 real-source helper tests passed. Their code is
  unchanged by the final startup file-I/O correction, so those results were
  carried forward rather than rerun. Three native controller tests were compile
  checked only and are not counted as executed.
- Eden verified original Solo Popo → numbered paired Ice Climbers → original Solo
  Popo; default Minus and permanent B + D-pad Up; normal Plus pause; L/R stage
  paging; direct Waiting Room → Training stage changes; normal Training exit.
- A second Eden session verified creation of the missing shortcut INI, then a
  normal game stop/relaunch with X configured: X opened the menu, Minus no longer
  did, and the permanent shortcut still worked. Normal Training exit passed.
- Both sessions retained the Switch mod set plus trace controls, exited normally,
  used no swap, and restored their profiles. Observed emulator peaks were about
  5.41 and 6.16 GiB. These are emulator measurements, not Switch memory usage or
  proof that the intermittent console fault was reproduced or fixed.

Earlier native ARM-loop regression evidence for the unchanged guard assembly is
retained. This build has no new physical-console or LDN acceptance claim. The
updated Solo cycle behavior is not yet represented by the older demonstration GIF.

## Artifact identity and build options

- NRO size: 5550080 bytes.
- NRO SHA-256: `c72fa274f239a3d30a51cc802d8d2088bbd75b342ba084b1e7af02678670b17e`.
- Frozen build-input SHA-256: `b1a1cff8456cf719b48257a1d9b527c62fc4e47b2200a2c677e467540133f7f0`.
- Package version remains 7.1.2; the experimental tag identifies this candidate.
- The source commit preserves the exact compiled Rust/assets and lockfile;
  listed guide/release documentation was updated after the build. The optional
  Deluxe SDK requires the pinned snapshot described in `Cargo.toml`.

Enabled local feature closure:

```text
automation-results-native console-map-probe console-practice-trace console-swap-trace cpu-pilot-probe deluxe-sdk dep:deluxe-plugin-sdk lobby-stage-probe managed-sources mii-managed-source-coexist-alpha mii-ordinary-cpu-alpha mii-ordinary-player-alpha mii-saved-alpha native-unlink-guard native-unlink-probe practice-stage-observer practice-stage-request-alpha training_mod_consts/mii-saved-alpha training_mod_consts/p1-cpu-control training_mod_tui/mii-ordinary-cpu-alpha training_mod_tui/mii-ordinary-player-alpha training_mod_tui/mii-saved-alpha waiting-roster-reload
```
