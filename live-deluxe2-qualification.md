# Revision27 + Deluxe6c39 — scoped coexistence accepted

2026-10-12. Product `single-file-waiting-ui` at
`a16e5ebaff65cdf2f1df6f6e3feb0f28cb5a0400` plus inherited changes; runtime source
unchanged frozen27. No implementation commit, push, release or device deployment.

## Pins and scope

- Normal27 NRO SHA256 `4d6cab005296ea7e95082289f3825e4c6ad8e9cc3aefad3d37e391657c7daac7`.
- Deluxe NRO SHA256 `6c396f10e8c4c3bb5bce41de7922f63f77500de37c39c7e477ab54f2f527f8a9`,
  2428928B, verified manifest/source in `deluxe-fixture-6c396f10`.
- Pinned Eden `10bcd2d849843b146a79c61a21df615e1bdcfcde`, binary
  `64cf47519891440f4a5adfe4d3dfeff180fa4d90b61eb424ee1550e2a3fc3004`, game13.0.5.
- Owned acceptance-fable clone only; full config/save/SDMC before-backup,
  protected keys/load, volume0/gpu_log_level0 before boot. Each game900s/8GiB/noSwap,
  independent3GiB host floor; no concurrent build/game.
- Deluxe manual LessLag, emulator, effects on/all, input_buffer auto,
  auto_profile false, fps_boost false. Combat parent read verified committed/effective
  LessLag, active2/allocated3, no policy failure. Selected-field transcription in
  `runs/live-r27-deluxe2-on/combat-deluxe-observation.json`; not a full raw-file snapshot.

## Native capture and cold Training

`live-r27-deluxe2-on` created native Time1:00/CPU9 rules, played Shulk57 vs Pikachu8
on Battlefield. CSS CPU began Random; this is **not a frozen deterministic benchmark
fixture**. Fractional stick(-.375,.2), jump/attack/special exercised; `retry-combat.png`
shows Shulk airborne with Jump art/56.5%, PikachuCPU, 0:06.70 remaining. Results and
native replay-save confirmation observed. Actual rule/stage observations do not add
unqualified fields to the canonical schema.

Console `save_bound generation=1 index=0` at401.859516, persisted402.460022:
`replay202610123730.bin`, title `Replay`, archive
`7265706c61793230323631303132333733302e62696e`. Stopped inspector
`training-live-parent-inspect-1791788947880570132` passed: skippednone,
truncatedfalse, exact canonical/cache projection. Both tracks3600inputs;
Shulk nonneutral161/unquantized analog, Pikachu nonneutral3600/quantized analog.
This is native-buffer completion, not independent simulation-tick completeness.

`live-r27-deluxe2-reopen` cold booted with capture off (console5427), no live adapter.
Deluxe main loaded/returned0x0. An initial navigation detour reached Mob Smash CSS
but did not start that mode; returned and entered actual Training. Picker screenshot
`cold-saved-capture.png` shows `Replay · 3600f` with Pac-Man/Dr.Mario. Selecting entry1
reloaded CPU to Pikachu; matching Training setup saved.

First crop attempt was on the settings menu, not editor: R opened Reset All with
No selected; no successful preview/export is attributed to that attempt. Its three
screenshots remain separate. Corrected editor entry via A,A then crop `[0,8)` passed:
`cold-eight-v2-previewed.png` shows Frame7, Out8, Inputs complete. Normal-build console
revision97 native_play642.785712, inputs_complete642.886317/frames8. Native keyboard
id1 returned/consumed/applied0x0, name **Deluxe Pikachu eight**. Log batching is not
keyboard latency. Two restore_stop messages, then restored_idle660.984409, consistent
with the previously reviewed redundant-restore follow-up.

Stopped verification `training-live-parent-deluxe2-export-1791789900` passed,
625ms/40MiB/0swap under1GiB/noSwap/CPU100%/300s. Exact requested name, fighterkind8,
start, eight cached inputs and independent native-f32 projection match. Placement
zero deltas/preserve_recorded; selected ID correct. All12original macro JSONs unchanged;
only added `deluxe-pikachu-eight.json`. SHA256
`31ce8587d18df271699a958cb7e00e83409ac63aef3e4087b8fbbf27889a8d55`.
Native saved replay file independently exists, SHA256
`8684c2aba75f29688b492b25470916b8a8192c5950d3149bacd6827bde9eef7d`.
Receipt `live-acceptance-27-deluxe2/export-verification.json`.

## Timing export feasibility, not an overhead comparison

Existing global Debugging record_frame_times INI produced both normal-shutdown CSVs.
Byte-identical originals preserved as `live-acceptance-27-deluxe2/frame-times-{on,reopen}.csv`.

| Whole session | Samples | Mean ms | p50 | p95 | p99 | Max ms |
|---|---:|---:|---:|---:|---:|---:|
| Smash capture on |20999|19.05794|16.6699|33.3109|47.7606|8216.69|
| Cold Training capture off |35025|18.80408|16.6692|33.3100|33.3984|16200.1|

Nearest-rank quantiles. These untimestamped composition intervals include boot,
menus, keyboard pauses, gameplay and results. They are **different workloads**;
do not subtract them or call them a matched OFF/ON overhead measurement. No combat-only
frame tails, displayed-frame/input-to-photon measurements, simulation clock or
verified deliberate frame drops are supplied. Full protocol `live-performance-protocol.md`.

## Closure and operational defect

On: normalexit0,444.310519s, ended1791788893.4844604, peak6192701440B.
Reopen: normalexit0,695.601043s, ended1791789693.2337859, peak5340020736B.
No memory max/OOM/swap events. On met8min parent target; reopen missed it215.601s.
Independent660s normal-stop attempt during the naming dialog failed with
`modal_active: Handle the active dialog through ui commands first`; raw traceback
in `normal-stop-live-r27-deluxe2-reopen.log`. Parent completed the name and stopped
normally before900s cap. This exposed a helper retry defect, not a successful automatic
shutdown. Parent correcting only that known transient case within the same fixed
180s shutdown budget; no new native product change.

Both Eden unitsbee9f399ed1a/4c17d4c0b6b7 and inspector/export units independently
not-found/inactive/dead/MainPID0. All10recorded runtime/monitor PIDs absent.
Fullrestore27-deluxe2 verified1791789840.4337406: exactbefore config/save/SDMC,
protected keys/load and ARC-only baseline. After-state preserved:

- config-1791789839969313022
- saves-1791789839972773277
- sdmc-1791789839994487782 (capture/export/Deluxe logs).

Original pilot1 remains an honest runtime_limit/exit1 failure with no saved capture
or normal-shutdown CSV, separately restored. Its mechanical review was corrected
for Diddy's generated banana (not an item-frequency contradiction), remaining match
time, and terminal vs pre-stop unit status. Worker452de2f1-d3d7-46c9-b1b6-649d5e66644e
stopped; recorded owner3609662/watcher3623566 absent.

## Remaining gates

Matched normal-build capture OFF/ON cost and bounded deliberate slowdown still
pending; a fixed selected CPU/stage fixture and equal cache snapshots are needed.
No second cold restart after the new macro export, broad roster/stress coverage,
physical Switch or network qualification. Actual presentation drops and every
simulation tick remain unqualified under the existing pinned emulator.
