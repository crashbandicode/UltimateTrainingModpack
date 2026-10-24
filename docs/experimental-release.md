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
  Changing the currently cycled form applies when the menu closes; starting
  as Ice Climbers no longer prevents saved-Mii entries from joining the cycle.
  A fresh Training visit clears the previous cycle position, including after
  a Mii swap, so original Ice Climbers follows its own Solo setting.
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
at most 128 KiB each, plus bounded feature diagnostics. The continuous trace,
console-map and boot/load probes are not enabled in this candidate.
This build does **not** create a multi-gigabyte trace. Keep those small logs and
Atmosphère crash reports if a fault occurs.

## Validation and limits

The current candidate moves saved-Mii catalog reads and writer waits off the
menu-opening path into bounded background work. Fourteen catalog/auto-init
regressions, strict native Clippy, no-default and full release builds pass.
The reported G&W → Brawler → quit → original Ice Climbers sequence now starts
with both Climbers when original Solo is Off, even with the cycle red S On.
Return through slot 0, quick menu close/reopen, saved Brawler selection, and
Quickplay Add New/cancel passed in the same emulator session. The matched
Deluxe/Elite pair also passed compact passive status-table navigation and save.
The session exited normally and all owned profile changes were restored.

The all-seven-plan resource review is complete within its documented scope.
The latest whole-emulator peak was about 6.7 GiB across two game loads under
an 8 GiB/no-swap limit, without memory-pressure events. This includes native
asset caches; no isolated catalog speedup or long-term leak proof is claimed.
The earlier measurements below remain evidence for unchanged features.


Native all-target Clippy produced zero warnings; no-default and full release
builds passed. Five focused ownership/cycle regressions passed. Earlier
unchanged guard, functional and resource tests were carried forward.

The corrected candidate completed five Player and two CPU swaps in Eden,
including immediate Solo/normal partner refresh, the next Brawler cycle entry,
original-fighter return, and fresh Training exit/reentry. Both settings on the
new Deluxe **Stages + Counterpick** plugin page saved successfully while
preserving fighter rankings. Gameplay settled near 60 FPS; the sampled emulator
host usage was about 5.5 GiB, not a console-heap measurement.

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
small portrait can retain the previous Mii. The separate standalone counterpick has passed four accepted-rematch cases
against an unmodded peer. Its both-modded check is tracked in that repository.
Physical-console acceptance of this corrected full candidate remains pending. No claim is made for online arenas or every possible
fighter/costume/move/stage combination.

## Artifact identity

- NRO: 4272128 bytes.
- SHA-256: `97f01b715a9b455ecf9751495ea3d935496da17ed529a1f06c9673cd223665c6`.
- Frozen compiled-input SHA-256: `8a43e10a61c2b0a7183c6e9e7ef3015357c455b585be1014838d1d7d34206701`.
- Package version: 7.1.2; the experimental release tag distinguishes the candidate.

Enabled local feature closure:

```text
cpu-pilot-probe deluxe-sdk dep:deluxe-plugin-sdk enhancements-alpha lobby-stage-probe managed-sources mii-editor-alpha mii-managed-helper-alpha mii-managed-source-coexist-alpha mii-ordinary-cpu-alpha mii-ordinary-player-alpha mii-saved-alpha native-unlink-guard practice-stage-observer practice-stage-request-alpha training_mod_consts/mii-managed-helper-alpha training_mod_consts/mii-saved-alpha training_mod_consts/p1-cpu-control training_mod_tui/mii-editor-alpha training_mod_tui/mii-ordinary-cpu-alpha training_mod_tui/mii-ordinary-player-alpha training_mod_tui/mii-saved-alpha waiting-roster-reload
```
