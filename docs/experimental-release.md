# Full enhancements alpha

This opt-in build targets Smash Ultimate 13.0.5 and is the latest published
download. Current source is newer; see the [C89–C92 build notes](lobby-parity-release.md).

Known limits: intermittent pre-title console crashes remain unresolved. Cleanup
guards contain observed null-reference faults; the writer that corrupts those
references is still unknown.

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
  owned editor, Headgear, Outfit, Color, Voice and Random stay available and
  keep their unfinished screen, value, and cursor across a match. Name,
  Specials, Settings and Test keep their existing interruption support.
  Ordinary Training keeps the cosmetic options. Explicit Cancel does not save.
- Shared native-field checks protect selected cleanup and Mii chains. They do
  not cover every unsafe access or prove that a non-null pointer is alive.
- Cleanup guards are independent of optional tracing. The broad/focused trace
  writer and unlink-observer capture are absent from this full build.

See [how character changes work](character-changes.md) for the loading mechanism,
Mii storage, source-fighter limits, and same-lobby continuation.

## Install and logs

Use the **full** candidate on [Forgejo releases](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack/releases).
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

Emulator and host checks only; physical-console acceptance of this corrected
full candidate is pending. Not claimed: online arenas, every
fighter/costume/move/stage combination, Switch frame time or guest-heap use.

| Check | Result |
| --- | --- |
| Builds | Strict native all-target Clippy with zero warnings; no-default and full release builds |
| Host regressions | 16 native-field/descriptor; 14 saved-Mii catalog/auto-init; 5 focused ownership/cycle. Earlier unchanged guard, functional and resource tests carried forward |
| Saved-Mii catalog candidate, one Eden session | Saved-Mii catalog reads and writer waits run as bounded background work, off the menu-opening path. The reported G&W → Brawler → quit → original Ice Climbers sequence now starts with both Climbers when original Solo is Off, even with the cycle red S On. Return through slot 0, quick menu close/reopen, saved Brawler selection, Quickplay Add New/cancel, and the matched Deluxe/Elite passive status table and save also passed. Normal exit; owned profile changes restored |
| Corrected candidate | Five Player and two CPU swaps: immediate Solo/normal partner refresh, the next Brawler cycle entry, original-fighter return, fresh Training exit/re-entry. Both **Stages + Counterpick** settings saved with fighter rankings preserved. About 60 FPS; about 5.5 GiB sampled emulator host memory |
| Full-alpha integration | First-shortcut Mii initialization, managed Mii Brawler hat and Shot Put, native editor/Test/discard, Training quit/re-entry, Quickplay disabled-cosmetics navigation and cancel back to waiting practice. Normal exit; profile restored; host peak about 5.7 GiB under an 8 GiB/no-swap limit |
| Resource review, all seven plans | Complete within its documented scope. Eight 20-second windows (baseline/full idle, hat helper, native editor, held item, settled stage) stayed near 60 FPS without memory-pressure events; idle CPU about 0.945/0.951 cores; helper cleanup recorded zero tracked leases. Latest whole-emulator peak about 6.7 GiB across two game loads under 8 GiB/no-swap |
| Native-access refactor | Field/pointer and specialized call helpers preserve existing guards and ownership; targeted swaps, hat/item, editor cancel, stage change and clean exit passed. No universal memory-safety or crash-fix claim |
| Diagnostic efficiency | After exactly 64 snapshots and one notice, a new native stage request completed. Stops redundant diagnostic reads and two lock pairs after the limit. Same-process idle samples stayed near 60 FPS; CPU difference was too small to claim a speedup. No allocations added; NRO size unchanged |
| Direct Mii discovery | Saved two live LAN Online matches: unknown opponent Mii imported, already-owned Mii reused without a duplicate. No Vault playback or capture selection; both emulators exited normally |
| Custom Mii HUD | 36 focused core/policy/guard tests; base/full native Clippy and builds. Training role/class/configuration changes and original icon restoration passed. All three waiting CPU portraits passed. An actual LDN match preserved native results portraits and restored the custom CPU portrait automatically; both emulators exited normally. Console acceptance pending |
| Earlier coverage, not repeated | 85 selectable hats in both roles; 38 item grant/restore cases; saved-Mii swaps and replay discovery/dedup/restart; editor interruptions; local-wireless match/return/helper reconstruction |
| Stages + Counterpick (separate plugin) | Four accepted rematches against an unmodded peer; the both-modded check is tracked in that repository |

- Memory figures are emulator host samples that include native asset caches and
  different cache warmup. No isolated catalog speedup, feature-memory figure or
  long-term leak proof is claimed.
- A later GIF session (manual selector, opposite Solo cycle, direct stage change)
  completed, but Eden's Closing software overlay stalled on shutdown. The owned
  process was stopped and its profile restored; this is kept separate from the
  clean lifecycle passes above.
- Custom Mii portraits follow Player and CPU selections in Training and the CPU
  in Local Wireless waiting practice. The lobby Ready card preserves the fighter
  selected before entering; Quickplay portraits are unchanged.
- HUD rendering uses Smash's native assets. Temporary portrait resources are
  retired before leaving waiting practice and rebuilt on return; no portrait
  rendering or continuous diagnostic writes run during an actual match.

## Artifact identity

- NRO: 4308992 bytes.
- SHA-256: `4eb6ce417a55808b26c8ce2f2a9ba026146a0819f73ad176b34308f17c29ef28`.
- Frozen compiled-input SHA-256: `59edb618760b97464749e9f6ee83851209d3f13f78d12862d517cbea87e1a040`.
- Package version: 7.1.2; the experimental release tag distinguishes the candidate.

Enabled local feature closure:

```text
cpu-pilot-probe deluxe-sdk dep:deluxe-plugin-sdk enhancements-alpha lobby-stage-probe managed-sources mii-editor-alpha mii-hud-portrait-alpha mii-managed-helper-alpha mii-managed-source-coexist-alpha mii-ordinary-cpu-alpha mii-ordinary-player-alpha mii-saved-alpha native-unlink-guard practice-stage-observer practice-stage-request-alpha training_mod_consts/mii-managed-helper-alpha training_mod_consts/mii-saved-alpha training_mod_consts/p1-cpu-control training_mod_tui/mii-editor-alpha training_mod_tui/mii-ordinary-cpu-alpha training_mod_tui/mii-ordinary-player-alpha training_mod_tui/mii-saved-alpha waiting-roster-reload
```
