# Character cycles development validation

The shared portrait editor, transforming-fighter support and input-display
follow-up are development changes, not part of the published 7.1.2 package.

## Behavior checked

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

The final source passed all-target Clippy with warnings denied, the release
build and the no-default-features build. Host checks passed 116 TUI tests across
six harnesses and 25 constants/menu tests in one harness. These include legacy
roster migration, independent role order, hidden CPU settings, defaults and
form-preview behavior. Final source and binary identities were checked; later
changes were documentation/media only.

## Performance and limits

The [input-display measurements](input-display-followup.md) show fewer pane
visits with matching rendered states. They do not establish a Switch FPS gain.
The hot-swap logging experiment showed no repeatable speedup and was reverted;
this change makes no hot-swap latency claim.

The portrait grid matches the tested all-unlocked save's character-select
layout. It does not dynamically read other save layouts. Ice Climbers and Miis
remain unavailable. Loading a Pyra/Mythra save currently restores position
without switching back to the saved form. Physical Switch acceptance remains
pending for these changes.

## Refreshed demonstrations

All clips use the same final development NRO in Linux Eden, with real UI frames
captured at 10 fps. Only the emulator window margins were removed. Start,
action and completion frames were visually reviewed.

| Clip | Recorded behavior | Size / duration |
| --- | --- | --- |
| [Character cycles](fighter-hotswap/character-cycles.gif) | Pyra/Mythra previews and separate player/CPU additions | 960×540 / 13 seconds |
| [Training hot-swap](fighter-hotswap/hotswap.gif) | Player Mythra → Pyra, then CPU Charizard → Mythra; both loads complete | 800×450 / 18 seconds |
| [Waiting-room practice](lobby-practice/waiting-room.gif) | Shared CPU grid, then Charizard → Link; CPU icon updates | 960×540 / 14 seconds |

The waiting GIF uses the first 140 consecutive frames of a 22-second capture;
only the final idle tail was removed. The existing opponent-input macro GIF is
unchanged, preserving its verified match-source alignment.

Machine-readable identities are in [media-provenance.json](media-provenance.json).
Raw captures, build logs, manifests and runtime diagnostics are retained locally
under `artifacts/training-performance-followup-20261015/` beside the worktree.
