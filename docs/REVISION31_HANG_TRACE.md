# Revision 31 — hang tracing for the console (2026-10-13)

Revision 30 (hook-proof UI-manager locator) booted on the operator's Switch beside Deluxe 0.3.1 but froze on a
loading screen when a scene with fighters loaded (Training mode; a Local Wireless entry stopped at the same point).
No panic, no Atmosphere crash report; the diagnostics file ended at the four `[TrioBoundary] allocation` rows. The
same build with the console's complete plugin set entered Training mode on Eden without a hang, and the only
install-time difference the console shows is Deluxe 0.3.1's console-only dispatcher hook group, which Eden never
installs. The console therefore has to tell us where it stops.

## What revision 31 adds (`src/hang_trace.rs`)

- **Probes** on the modpack's scene-load hooks, lock-free atomics only: `ui_pass` (per-frame UI anchor, UI job
  thread), `ui_register` (manager insert, main thread), `tick_pass` / `tick_register` (the modpack's own `ui::tick`
  body, marked ACTIVE until it returns), `roster_service` (fighter entry service, game thread), `alloc` / `reload` /
  `destroy` (fighter entry boundaries), `layout_arc` (layout archive hook, ACTIVE while the archive is rebuilt) and
  `nro_load` (Skyline NRO callback, ACTIVE while the modpack installs its common hooks).
- **Heartbeat thread** (`practice-heartbeat`, every 5 s): one `[Heartbeat] n=… up=…s mode=… <probe>=<count>/<ms since
  last call> [ACTIVE <ms> detail=…]` line in `practice-diagnostics.log`. A frozen game keeps producing heartbeats, so
  the last ones show which probes stopped advancing (which thread is stuck) and whether a modpack hook never returned
  (ACTIVE with a growing age and the layout pointer or archive size as detail).
- **Progress lines** during a scene load: `[HangTrace] register layout=<name> holders=<n>` for each layout
  registration (budget 400 per session) and `[HangTrace] nro_load name=<module>` for each NRO the game loads (budget
  200). On Eden the Training load registers `info_training` within a few seconds of the allocations.
- The `[ReplayCapture]` initialization verdicts (Deluxe row registration, `live_enabled`, guard failures, adapter
  ready) now go to the diagnostics file as well, so the console reports whether the Deluxe host registered the capture
  row and whether the revision 28 adapter hooks are resident.

Nothing else changes: the UI anchors, offsets and locator are revision 30's. The diagnostics file keeps its two
128 KiB files; at 5 s per heartbeat that is about 45 minutes per file.

## Reading a hang

1. Find the last `[LobbyPractice] scene initialized` and `[TrioBoundary] allocation` lines: the scene being loaded.
2. Look at the `[HangTrace] register layout=` and `nro_load` lines after them: the last successful load step.
3. Read the final heartbeats: a probe whose count stops advancing names a thread that stopped; `ACTIVE` names the
   modpack hook that did not return and its detail word; `mode=` shows whether the scene tuple still moved.
4. Compare with the Eden record in `research/named-macros/switch-r30/SWITCH_SMOKE2_HANG_NOTES.md`.
