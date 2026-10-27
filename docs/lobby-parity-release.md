# Lobby parity candidate C72 — unstable

Quickplay and Local Wireless waiting areas now support the practice tools,
including save/load, recordings, macro previews and interrupted Mii editing.
All [22 gameplay items](plans/lobby-parity.md#required-behavior) have completed
their documented emulator or shared-code checks. **Console acceptance is still
pending.** This candidate is separate from the earlier published enhancements alpha.

## What changed

- Practice operations stop when a match starts. Unfinished macro names and Mii
  drafts resume on return to the same lobby; Cancel still discards them.
- Practice stages offer Normal, Ω and Battlefield forms. Training rotation is
  optional and defaults Off; lobby rotation remains independent.
- CPU slot 0 restores the original fighter. Mii equipment loading was checked
  with Gunner, Brawler and Swordfighter in ordinary CPU Training.
- Mii swaps avoid the captured portrait flash. Grounded saves now restore below
  platforms instead of catching a platform between respawn and the saved point.

Follow the [illustrated Lobby Practice guide](lobby-practice.md).

## Build identity

| Field | Value |
| --- | --- |
| Smash version | 13.0.5 |
| Branch | `lobby-parity` |
| Full NRO size | 4,370,432 bytes |
| NRO SHA-256 | `7941988cc4f188c2baa01de09a6b55c0ede40251035f60ddb41a2c6eccc26ed5` |
| Compiled source content SHA-256 | `74f93b64f17770e6ba3ef557096d99afd0fc3aa272fb626226e768306d30b73c` |

Seven final build/validation jobs passed, including strict Clippy with zero
warnings, the no-default-features build and the full release. The Mii host suite
passed 243 tests across seven harnesses. Earlier feature tests are retained in
the ledger; this is not a claim that every old case was rerun on C72. Final guide
updates and one comment correction followed compilation without changing code.

## Console review

Back up the installed NRO, then replace only `libtraining_modpack.nro` in
`atmosphere/contents/01006A800016E000/romfs/skyline/plugins/`. Keep dependencies,
other mods, settings and saves. Do not install a second Training Modpack NRO.

1. Enter Training and load your under-platform/CPU-ledge save. Confirm the player
   stays on the saved floor and the CPU completes its ledge option.
2. Try an exact saved Mii as the CPU, then restore the original fighter.
3. In a waiting area, try save/load or a recording. Finish a match and return;
   confirm practice resumes normally and your real-match fighter is unchanged.

Known limits: online arenas and physical-console stability are unverified.
Changed-headgear equipment coverage here is ordinary CPU Training, with the
documented generic portrait fallback. Earlier intermittent startup faults remain
unresolved; a later emulator loading stall is retained separately from the clean
C72 run. Broad tracing is off; keep the small practice logs and Atmosphère report
if a console fault occurs.
