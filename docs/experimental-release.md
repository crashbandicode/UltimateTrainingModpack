# Full enhancements alpha — UNSTABLE

This opt-in alpha targets Smash Ultimate 13.0.5. Console stability remains under
review. Cleanup guards contain observed null-reference faults; the writer that
corrupts those references is still unknown. This is not a stable release.

The full alpha includes character cycles and manual portrait selection, Solo
Popo, Kirby hats, character items, direct practice stages and lobby rotation,
saved-Mii cycling, opponent-Mii replay discovery, and Add New through the game's
native Mii editor and Test screen.

## Included behavior

- Original P1 Ice Climbers uses **Character Extras → Solo Popo**. Numbered
  Ice Climbers uses the independent red S; CPU uses blue S. Opposite Player
  configurations can share a cycle, with the original restored at slot 0.
- Practice Stages includes Training and Waiting Room. **L/R** pages; direct
  selection changes the current Training stage. **X** edits lobby rotation,
  which advances only on lobby-practice entry.
- Two optional [INI menu shortcuts](menu-shortcuts.md) default to Minus and
  disabled. **B + D-pad Up** is permanent. Plus opens normal pause unless
  explicitly configured as a modpack shortcut.
- Native Mii editing and managed Mii hats are enabled again. In Quickplay's
  owned editor, cosmetics are disabled; Name, Specials, Settings and Test retain
  interruption support. Ordinary Training keeps the cosmetic options.
- Shared native-field checks protect selected cleanup and Mii chains. They do
  not cover every unsafe access or prove that a non-null pointer is alive.
- Cleanup guards are independent of optional tracing. The broad/focused trace
  writer and unlink-observer capture are absent from this full build.

See [how character changes work](character-changes.md) for the loading mechanism,
Mii storage, source-fighter limits, and same-lobby continuation.

## Install and logs

Use the **full** unstable candidate on [Forgejo releases](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack/releases).
Back up the current NRO. Rename the download to `libtraining_modpack.nro` and
replace only that file in
`atmosphere/contents/01006A800016E000/romfs/skyline/plugins/`.
Retain Skyline, ARCropolis, `libnro_hook.nro`, `libparam_hook.nro`, other mods,
settings and saves. Never keep two Training Modpack NROs installed.

Small diagnostics remain in `ultimate/TrainingModpack/`: two practice logs of
at most 128 KiB each, a bounded console-map record, and bounded boot/load records.
This build does **not** create a multi-gigabyte trace. Keep those small logs and
Atmosphère crash reports if a fault occurs.

## Validation and limits

Native all-target Clippy produced zero warnings. The no-default build, combined
guard/probe build, full release and seven focused Cargo tests passed. Earlier
unchanged guard and functional tests were carried forward.

The full-alpha integration session observed first-shortcut Mii initialization,
managed Mii Brawler hat and Shot Put, ordinary native editor/Test/discard,
Training Quit and reentry, and Quickplay's disabled-cosmetics navigation with
normal cancellation back to waiting practice. Eden exited normally and the
test profile was restored. Its sampled host peak was about 5.7 GiB under an
8 GiB/no-swap limit; this is not Switch or guest-heap accounting.

Earlier coverage includes 85 selectable hats in both roles, 38 item grant and
restore cases, saved-Mii swaps and replay discovery/dedup/restart, editor
interruptions, and local-wireless match/return/helper reconstruction. Those
matrices were not repeated for this integration.

The later GIF session completed the manual selector, opposite Solo cycle and
direct stage change, but Eden's Closing software overlay stalled on shutdown.
The owned process was stopped and its profile restored. That observation is
kept separate from the earlier clean lifecycle pass.

The bounded resource pass compared baseline/full idle activity and sampled the
hat helper, native editor, held item and settled stage. All eight 20-second
windows stayed near 60 FPS without memory-pressure events. Idle CPU use was
about 0.945/0.951 cores. Absolute host-memory comparison includes different
native cache warmup and is not an isolated feature-memory measurement. The full
run exited normally, with helper cleanup recording zero tracked leases. These
are host samples, not console frame-time percentiles or a long-term leak test.

Custom Mii HUD portraits remain deferred: the body and moves change, but the
small portrait can retain the previous Mii. The separate standalone counterpick's paired
accepted-rematch check and physical-console acceptance remain pending. No claim is made for online arenas or every possible
fighter/costume/move/stage combination.

## Artifact identity

- NRO: 4276224 bytes.
- SHA-256: `3412c7505cf7b50625daa6c23f47d48748b4c37d900a5efcc2947d0e7217b0d1`.
- Frozen compiled-input SHA-256: `146fdea8cea99d46d70f0f989aafddb00825dd8be20bda9679af09ef726e802b`.
- Package version: 7.1.2; the experimental release tag distinguishes the candidate.

Enabled local feature closure:

```text
console-map-probe cpu-pilot-probe deluxe-sdk dep:deluxe-plugin-sdk enhancements-alpha lobby-stage-probe managed-sources mii-editor-alpha mii-managed-helper-alpha mii-managed-source-coexist-alpha mii-ordinary-cpu-alpha mii-ordinary-player-alpha mii-saved-alpha native-unlink-guard practice-stage-observer practice-stage-request-alpha training_mod_consts/mii-managed-helper-alpha training_mod_consts/mii-saved-alpha training_mod_consts/p1-cpu-control training_mod_tui/mii-editor-alpha training_mod_tui/mii-ordinary-cpu-alpha training_mod_tui/mii-ordinary-player-alpha training_mod_tui/mii-saved-alpha waiting-roster-reload
```
