# Character cycles development validation

The shared portrait editor, grouped character cycles, Solo Popo and input-display
follow-up are development changes, not part of the published 7.1.2 package.

## Earlier portrait/input-display validation — 2026-10-15

The checks in this section describe the earlier development source and NRO,
before the character-cycle/Solo Popo plan. Their counts and runtime scope remain
historical evidence, not a claim that every case was repeated on the new build.

- Reproduced the loading lockout from native Pyra with the seven requested
  player choices. The readiness check incorrectly required a single-body
  fighter. The corrected entry/body checks passed Captain Falcon → Palutena →
  Ness → Lucas → Kirby → Byleth → Inkling from that same native starting fighter.
- Tested Pyra, Mythra, Squirtle, Ivysaur and Charizard for both roles in Training,
  interleaved with ordinary fighters: 32 completed requests across two fresh
  processes. Normal transformations and Pokémon saved-form reloads passed.
- Tested the final portrait build in a fresh Training process: correct icons,
  red/blue orders, role switching, form previews, preserved legacy ordering,
  saved settings after restart, and four completed replacements.
- Tested the final build in a local-wireless waiting room: the shared grid,
  both roles, all five CPU starting forms, ordinary interleaves, CPU icon
  refresh and the unchanged original player match card. Fourteen replacements
  completed. This was a solo waiting-room test, not an online-arena or new
  two-peer match/return test.

That source passed all-target Clippy with warnings denied, the release
build and the no-default-features build. Host checks passed 116 TUI tests across
six harnesses and 25 constants/menu tests in one harness. These include legacy
roster migration, independent role order, hidden CPU settings, defaults and
form-preview behavior. Final source and binary identities were checked; the
remaining changes in that validation round were documentation/media only.

The earlier Training hot-swap demonstration was 800×450 / 18 seconds and showed
P1 Mythra → Pyra, then CPU Charizard → Mythra. Its same-path GIF has since been
replaced by the new demonstration below; this earlier behavior is not the
new grouped-cycle contract.

## Enhancements development build — 2026-10-16

Validated NRO SHA-256:
`2c81ac2a28649e955a202dbc79faece52ecde039b03232c5c336b14f82cb3024`.
This is the enhancements development build, not a newly published release.

- **Builds PASS:** strict root all-target Clippy with `-D warnings`, release
  build and no-default-features build.
- **Host menu checks PASS:** **150 suite tests** (34 constants/menu + 116 TUI)
  and **5 independent probes**. Repeated constants tests and nested continuation
  summaries are not counted again. The independent correction review and exact
  run/list counts are retained beside the worktree in
  `artifacts/enhancements/menu-review-candidate2/REPORT.md`. Fresh-process
  save/restore continuation verifies host menu state; it is not native gameplay
  evidence. The earlier failing probe and report remain preserved.
- **Native Training PASS:** original P1 Ice Climbers stays paired when only the
  red S is enabled; Character Extras switches that original P1 immediately
  between pair and Solo on menu close; CPU blue S operates independently.
- **Native Training PASS:** automatic Solo after fighter readiness, Solo
  save/load, and actual P1 and CPU KO followed by Solo respawn. The accepted CPU
  KO used native CPU Control and a second controller to walk offstage; an earlier
  up-tilt attempt was not accepted as KO evidence.
- **Native Training PASS:** native Mario → cycled Solo Ice Climbers; Mythra →
  normal transformation to Pyra → next cycle group Charizard → normal
  transformation to Squirtle → next original Mario **0**. This checks group
  advancement after normal transformations and P1's return through slot 0.
- **Local-wireless waiting room PASS:** original P1 Ice Climbers stays paired
  with red S enabled and Character Extras off; the original CPU becomes Solo
  under blue S. Enabling Character Extras changes P1 to Solo on menu close.
- **Local-wireless waiting room PASS:** both orders and S indicators, grey P1
  slot 0, selectable CPU original, and independent All selections. All advances
  P1 through Mario and Donkey Kong and CPU from Ice Climbers to Sheik; the CPU
  icon updates while the original P1 match card remains Ice Climbers.
- **Local-wireless waiting room PASS:** X previews Mythra on the CPU Aegis tile,
  A commits it as one custom slot, and the shortcut loads Mythra with its updated
  CPU icon. The initial four-second screenshot was still loading; completion was
  checked afterward and corroborated by the native completion log.

Primary-owned native captures are retained in
`artifacts/enhancements/plan1-native-03/` and `plan1-native-04/` beside the
worktree. Local-wireless evidence is in `artifacts/enhancements/plan1-lobby-01/`.
All four refreshed GIFs were visually accepted. Each owned emulator was stopped
and its temporary profile settings restored. This solo waiting-room test does
not establish a new two-peer match/return, online-arena or physical Switch pass.

## Performance and limits

The [input-display measurements](input-display-followup.md) show fewer pane
visits with matching rendered states. They do not establish a Switch FPS gain.
The hot-swap logging experiment showed no repeatable speedup and was reverted;
this change makes no hot-swap latency claim.

The portrait grid matches the tested all-unlocked save's character-select
layout. It does not dynamically read other save layouts. Ice Climbers is now
supported, with independent original-P1 Character Extras, cycled-P1 red S and
CPU blue S settings. Miis remain unavailable pending the separate Plan 2.
Loading a Pyra/Mythra save currently restores position without switching back to
the saved form. Physical Switch acceptance remains pending for these changes.

## Refreshed demonstrations

These Linux Eden clips contain real UI frames captured at 10 fps. Only emulator
window margins were removed. Start, action and completion frames were visually
reviewed. All four use the validated development NRO identified above.

| Clip | Recorded behavior | Size / duration |
| --- | --- | --- |
| [Character cycles](fighter-hotswap/character-cycles.gif) | Original Solo Popo slot 0, adding paired Ice Climbers slot 2, then Kirby → pair → original | 960×541 / 26 seconds |
| [Solo Popo](fighter-hotswap/solo-popo.gif) | Original P1 pair → Solo via Character Extras; CPU Young Link unchanged | 960×541 / 11 seconds |
| [Manual hot-swap](fighter-hotswap/manual-hotswap.gif) | Portrait grid selects Bowser; CPU Young Link unchanged | 960×541 / 12 seconds |
| [Practice Stages](lobby-practice/stage-menu.gif) | L/R paging, rotation view, and complete direct change to Battlefield | 960×541 / 29.8 seconds |
| [Training hot-swap](fighter-hotswap/hotswap.gif) | Earlier footage: Mario → Solo Popo → Mythra; CPU Solo Popo → Kirby | 800×450 / 16 seconds |
| [Waiting-room practice](lobby-practice/waiting-room.gif) | P1 Solo Popo → Mario → Donkey Kong; CPU Solo Popo → Sheik; CPU icon updates | 960×540 / 16 seconds |

The waiting GIF uses the complete 16-second capture. The existing opponent-input macro GIF is
unchanged, preserving its verified match-source alignment.

Machine-readable identities are in [media-provenance.json](media-provenance.json).
Earlier raw captures, build logs, manifests and runtime diagnostics are retained
locally under `artifacts/training-performance-followup-20261015/` beside the
worktree; current evidence is under `artifacts/enhancements/` as listed above.
