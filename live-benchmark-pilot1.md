# Cold benchmark pilot1 — 2026-10-12

Normal27/noDeluxe/captureON, campaign `27-bench-pilot1`, run
`/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot1`.

- Tested stager copied the entire fixture2 native saves and full Eden/NVIDIA cache;
  readiness independently rehashed both. Source pins are in the campaign receipt.
- Cold preset `Capture benchmark 60s` persisted (Time1:00/SpiritsOff/first1win).
  **Fighters did not persist:** initialCSS was no P1 selection plus RandomCPU9.
  Parent reselected Shulk/PikachuCPU9 and explicit Battlefield Normal. This is
  preset/cache reuse, not automatic cold restoration of character selections.
- A batched copy of old cursor inputs selected Terry rather than Pikachu; parent
  corrected it and visually verified `pilot-fixed-ready.png` before starting.
  No incorrect-fighter match was launched. Native cursor timing needs visual checks.
- Bounded110s warmup driver completed. Start image shows Shulk/Pikachu on Battlefield
  with0:56.98 remaining; end image shows the Pikachu winner screen.
- Navigation consumed the measurement margin. Parent stopped at463.252560s, before
  the480s target, rather than starting a cycle that could not fit. **No measured
  window, replay-save attempt or companion publication occurred.** No matched or
  performance result is claimed.
- Normal CLI exit0 at1791794946.0472605, peak5803008000B/noOOM/max/swap. Unit
  `eden-control-bd25b346bf80.service` independentlynotfound/inactive/dead/MainPID0.
  PIDs3721673/3721674/3721675/3721750/3721738 independently absent.
- Stopped evidence/restoration unit`training-live-parent-bench-pilot1-close-1791795030`
  passed26.030s/693MiB/noSwap under1GiB/CPU100%/180s; independentlyclosed.
  Exact original config/save/SDMC/**fullcache** plus protectedkeys/load/ARC-only
  restoration at1791795061.4341354. All postrun trees preserved, no errors.
- Whole-session CSV21160samples, SHA256
  `258311a94347cf095445c833b7e688508ae9106d329e8c70af0948e158c12fa2`, retained as
  `live-acceptance-27-bench-pilot1/whole-session-frame-times.csv`. Not combat-only.

Next: bound and batch the already-observed menu route to remove model/tool-turn
waiting, verify final characters visually, and qualify warmup→measured→native-save
within the unchanged480s parent target. No acceptance threshold or runtime cap
was relaxed. A new finite lease is required.
