# Experimental enhancements alpha — UNSTABLE

This is an opt-in diagnostic alpha for Smash Ultimate 13.0.5. Intermittent
crashes remain under investigation. Do not treat this release as stable or as
proof that the underlying native-memory corruption has been fixed.

The NRO includes character cycling/manual swapping, Solo Popo, Kirby hats,
character items, direct practice stages and lobby stage rotation, saved-Mii
cycling, and guarded native cleanup/garbage-collector paths. The repository
also contains native Mii-editor and helper experiments; those two feature
gates are **not enabled in this particular diagnostic NRO**.

The newest guard handles an observed exact-null Lua constant during native
garbage collection. It continues to later constants without rewriting the
malformed value or disabling collection. Existing exact-null cleanup guards
remain. Native instruction checks refuse a mismatched patch window. The
original writer of the malformed reference is unknown; non-null dangling
pointers are outside this containment.

## Install

Download `libtraining_modpack-UNSTABLE.nro` from this release. Back up the
currently installed NRO, then rename the download to `libtraining_modpack.nro`
and place it at
`atmosphere/contents/01006A800016E000/romfs/skyline/plugins/` on the SD card.
Retain the existing Skyline, ARCropolis, `libnro_hook.nro`, and
`libparam_hook.nro` installation. No other mod or configuration is bundled or
replaced by this download.

Focused diagnostic capture starts automatically and writes
`ultimate/TrainingModpack/practice-trace-swap.bin` (bounded below 4 GiB), with
a small native unlink capture and diagnostic log in the same directory.
Existing Trace Control users can stop/start capture there. This build does
not write the old broad `practice-trace-controlled.bin` recording.

## What was checked

- Native all-target Clippy: zero warnings; no-default build and optimized
  release build passed with source hashes checked before and after.
- Two exact ARM-loop regressions: valid traversal has identical mark calls
  and instruction count; the observed null-shaped constant faults in the
  original loop and advances to later valid objects in the guarded loop.
  The mark callee is stubbed in this isolated test.
- One Eden session with retained Switch plugins: Mr. Game & Watch to Ice
  Climbers, exit Training, then re-enter as Palutena. Both guards installed,
  normal emulator exit, original profile restored. This is not physical
  console acceptance or an emulator reproduction of the intermittent fault.

The physical console has not yet accepted this exact candidate. The new
Solo Popo opposite-configuration cycle behavior, L/R stage paging,
configurable menu shortcuts, and ownership-helper followups are queued for
a later build and are not claimed by this release.

## Artifact identity and build options

- NRO size: 5537792 bytes.
- NRO SHA-256: `97aa5a34282271a36a4d4072377ae5710c0cce366810066e6ede88c15bd77e8a`.
- Frozen build-input SHA-256: `10b8835a3b9aa18a4ff5e7125895734be12b1c470633bfffcd02cef62723a7ca`.
- Cargo package version remains 7.1.2; the experimental release tag identifies
  this opt-in candidate. This is not a replacement stable version.
- The source commit contains the exact compiled Rust/assets snapshot plus
  release documentation. README and release notes were edited after build;
  compiled sources/assets were not changed. Cargo.lock is retained to bind
  the dependency resolution. The existing optional Deluxe SDK path requires
  the project's pinned SDK snapshot as documented in `Cargo.toml`.

Enabled local feature closure:

```text
automation-results-native console-map-probe console-practice-trace console-swap-trace cpu-pilot-probe deluxe-sdk dep:deluxe-plugin-sdk lobby-stage-probe managed-sources mii-managed-source-coexist-alpha mii-ordinary-cpu-alpha mii-ordinary-player-alpha mii-saved-alpha native-unlink-guard native-unlink-probe practice-stage-observer practice-stage-request-alpha waiting-roster-reload
```
