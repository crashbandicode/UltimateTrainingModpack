# Live capture performance / deliberate slowdown protocol — source only

**Publication status, October 12:** this is the original proposal. The later
[campaign](live-performance-campaign.md) defines the current fresh-process,
prewarmed-disk first-match lifecycle. Helpers and limited pilot measurements now
exist, but no matched OFF/ON pair or deliberate slowdown trial is complete; see
[the current archive overview](PUBLICATION.md). The original proposal below is
retained as written.

2026-10-12. Not implemented or executed. This worker wrote only this report;
no builds, tests, ELF inspection, game/profile/device/remote operations or
subdelegation. Parent owns fixture provenance, leases, execution and restoration.

## Decision

**A new Eden build is unnecessary for a useful, explicitly limited comparison.**
The existing profile INI can enable an export of actual per-composition wall-time
samples. The socket also supplies rolling FPS, and the owned cgroup supplies CPU
and memory counters. Together these support whole-session timing distributions
and matched combat-window throughput/resource comparisons.

They do **not** supply timestamped combat-only frame-time samples, a Smash
simulation/input-consumption counter, or evidence of deliberately discarded
presentations. A brief owned-cgroup CPU restriction can test capture under observed
underperformance. Exact preservation of the resulting native replay stream is
testable without new emulator instrumentation; lossless coverage of every gameplay
tick remains a separate blocked claim.

Final performance uses **normal revision27**, 3,346,432 bytes, SHA256
`4d6cab005296ea7e95082289f3825e4c6ad8e9cc3aefad3d37e391657c7daac7`,
not trace27. Eden executable pin supplied by the acceptance helper:
`64cf47519891440f4a5adfe4d3dfeff180fa4d90b61eb424ee1550e2a3fc3004`,
associated commit `10bcd2d849843b146a79c61a21df615e1bdcfcde`; game13.0.5.
This worker did not hash or inspect that executable.

## Supported paths and exact semantics

All source anchors below are under `/home/intpa/eden-cli-control`.

| Path | Evidence and usable meaning |
| --- | --- |
| `status {}` | `src/yuzu/control_adapter.cpp:210`: `game_fps`, `system_fps`, `shaders_building`, `stats_age_ms`, `last_frame_age_ms`, `running`, `powered_on`, `loading_complete`, `modal`, `commit`. No raw frame samples, frame totals or guest-consumption counters. |
| Rolling FPS | `src/yuzu/main_window.cpp:2020` updates statistics every nominal500ms. `src/core/perf_stats.cpp:101` computes interval system-frame rate and averages current/prior interval game FPS. `control_adapter.cpp:162` caches these results; polling faster adds repeated readings. `last_frame_age_ms` is age of a statistics update whose smoothed game FPS was positive, **not** age of the last individual frame. |
| Real timing history | `src/core/perf_stats.cpp:64` records steady-clock milliseconds from `BeginSystemFrame` to `EndSystemFrame`; `perf_stats.h:63` holds up to216,000 samples. `core/hle/service/nvdrv/devices/nvdisp_disp0.cpp:90` calls End/Begin after requesting composition and speed limiting. These are intervals between composition boundaries, not independently verified simulation ticks or scanout intervals. Despite the header's “excluding any waits” comment, implementation subtracts no waits; cadence, blocking and limiter time between boundaries can contribute. |
| CSV export | `perf_stats.cpp:35` writes history on `PerfStats` destruction when `record_frame_times` is true and title ID nonzero. Omits only first5 samples; newline-separated milliseconds, no header, timestamps or phase markers. Filename `<local-date>-<hour>-<minute>_01006A800016E000.csv` in Eden LogDir. Normal game shutdown is needed; a watchdog kill may leave no CSV. Separate each run's output immediately to prevent same-minute overwrite. |
| Enable export without rebuilding | `src/frontend_common/config.cpp:299,717,605`, `src/common/settings.cpp:263`: **global** `qt-config.ini`, `[Debugging]`, `record_frame_times=true`, `record_frame_times\default=false`. Parent edits only its stopped owned profile before fresh launch, enables identically in OFF/ON, and restores afterward. This is an existing INI option, not a diagnostic environment variable. `settings.h:873` declares a plain bool outside scalar linkage: **`core.record_frame_times` is not an existing socket setting**. |
| Input observation | `src/yuzu/control_input.cpp:203,315`: `input.state` is bookkeeping; button/stick replies expose `accepted`, `observed`, `frontend`, `guest_delivery:"not_claimed"`. `lease_ms`1..10000, expiration on20ms GUI timer. Neither replies nor commands count guest consumption; scripts scheduled by wall time are not deterministic frame schedules. |
| Existing slowdown settings | `src/common/settings.h:206,241`: `core.use_speed_limit`, `core.speed_limit`0..9999, `core.current_speed_mode` (`Standard`0, `Turbo`1, `Slow`2), runtime modifiable. `settings.cpp:423` selects the speed value for that mode. `core/hle/service/vi/conductor.cpp:102` scales multicore Vsync scheduling; `perf_stats.cpp:138` handles single-core limiting. Setting50% proves configured slower pacing, **not dropped simulation**. |
| Vsync setting | `settings.h:346`, `settings_enums.h:135`: `core.use_vsync` enum `Immediate`0, `Mailbox`1, `Fifo`2, `FifoRelaxed`3. `video_core/renderer_vulkan/vk_swapchain.cpp:44` applies device-dependent fallbacks. It exposes neither actual discarded-image counts nor a controlled skip-every-N command. Mailbox selection alone proves no drops. Keep unchanged in performance pairs. |
| Overlay | `src/yuzu/render/performance_overlay.cpp:96` charts interval aggregates, discards FPS≤3 and frame-time aggregates>500ms. Overlay extrema and screenshots cannot replace uncensored raw CSV tails. |

**Source/binary provenance caveat:** checkout HEAD read here is
`39ed41ea21bddef5b834a5f65c9fba3785e74610`, not the assigned binary's base
commit. Targeted `git diff --stat 10bcd2d... -- <inspected files>` shows no
differences for PerfStats, config reader, settings, compositor or Vsync sources;
CLI adapter/server/input and launcher are additions relative to that base, and
main-window changes exist. Therefore commit alone does not pin the private CLI
patches. Parent must check the **pinned executable's** `schema`, `status.commit`
and `settings.list` before relying on these methods. No new build is justified
merely by the checkout's different HEAD.

Exact client form, for an already owned live socket, executed through the required
PowerShell7 shell (examples are proposed, not run):

```powershell
& /usr/bin/pwsh -NoLogo -NoProfile -NonInteractive -Command { & /usr/bin/python3 /home/intpa/eden-cli-control/tools/edenctl.py --socket /OWNED/control.sock call status '{}' }
```

Use that same `call METHOD JSON` form for these existing operations:

| Method | JSON |
| --- | --- |
| `schema`, `settings.list` | `{}` |
| `settings.set` before boot, then `settings.save` | `{"key":"core.volume","value":0}`; save with `{}` |
| `settings.get` | `{"key":"core.speed_limit"}` and other discovered keys |
| `settings.set` controlled pacing only | `{"key":"core.use_speed_limit","value":true}`, `{"key":"core.current_speed_mode","value":0}`, `{"key":"core.speed_limit","value":50}`; restore effective original values afterward |
| `input.button` | `{"player":0,"button":"a","state":"tap","duration_ms":100,"lease_ms":1000}` |
| `input.stick` | `{"player":0,"stick":"left","x":-0.375,"y":0.2,"lease_ms":1000}` |
| `input.reset`, `game.stop` | `{}`; wait for queued job completion / actual shutdown |

`settings.set` refuses non-runtime settings while powered on and direct changes
during a modal. Keys and enum descriptions come from `settings.list`, not guesses.

## Minimal replicated OFF/ON campaign

1. Parent obtains a new finite lease. Each game has **≤900s total lifetime,
   8GiB MemoryMax, zero swap, independent3GiB host-available floor**, with no
   concurrent build/game. Use the existing protected clone workflow, unique run
   paths, normal stopping target≤720s and retained evidence before restoration.
   Existing `acceptance.py` has historical frozen13/start defaults up to1800s:
   do not blindly run its `start` against27. Its call/journal logic is reusable.
2. Freeze one ordinary Smash human/CPU fixture, preferably a60s **Time** match
   so early KOs cannot shorten the workload: same P1/CPU identities, CPU level,
   stage/form/hazards, rules, controller settings, resolution/GPU/Vsync, saves,
   plugin set and normal27. Use the same bounded human input pattern. CPU AI/RNG
   and wall-time CLI inputs are uncontrolled variability, not bit-identical replay.
3. Start from identical stopped snapshots with only capture INI `enabled=off/on`
   changed. Existing export flag is on in both; mute is persisted before boot.
   Freeze a prewarmed shader/disk-cache snapshot for all replicas. In each fresh
   launch play one identical60s warmup match, cancel its replay save, then one60s
   measured match. Keep this lifecycle identical, including warmup capture-buffer
   effects. Begin the combat measurement after the native start/countdown; exclude
   results and saving from that window. If shader work persists or menu/start
   delays differ materially, flag the pair rather than quietly trimming tails.
4. Minimum **3 matched pairs /6 fresh launches**, alternating order
   `OFF,ON / ON,OFF / OFF,ON`; a fourth `ON,OFF` pair improves order balance if
   the lease allows. Pair adjacent runs; identical warmup and cache starting state
   reduce compilation/order effects. Do not pool baseline and Deluxe conditions.
5. Poll `status` once per500ms with local monotonic request/response timestamps;
   retain ages, RTT, shaders, modal/running flags and missing samples. Repeated
   cached readings are not independent observations. No screenshots, tree dumps,
   debugger, trace27 or diagnostic environment flags during the combat window.
6. Sample owned cgroup `cpu.stat` (`usage_usec`, `user_usec`, `system_usec`,
   `nr_throttled`, `throttled_usec`) and `memory.current` at1Hz. Use differences in
   CPU usage divided by monotonic window seconds (one core=100%). Record final
   `memory.peak`, `memory.events`, swap counters and closure. `memory.current`
   includes charged cache/descendants; peak is whole-run high-water unless separately
   supported/reset. Do not call either heap allocation counts. The launcher already
   samples memory at5s but does not supply a CPU time series
   (`tools/eden-run.py:48,230`). Keep the independent host guard active.
7. Save the native replay in both conditions, using distinct identities. Measure
   save-confirm→companion-first-readable/validated wall time by monotonic bounded
   polling outside the combat window. This is **publication latency**, including
   native save/queue/poll uncertainty, not isolated writer duration. OFF must not
   publish a new companion. After normal game stop, archive the CSV, resources,
   actual match/result length and source/NRO/settings pins; preserve then restore.

Report each run's full-session CSV count, mean, p50/p95/p99/max and counts/fraction
above33.33ms and50ms, using a declared quantile definition (nearest-rank suffices).
**Label these boot/menu/warmup/combat/results composition-interval statistics.**
Do not reconstruct a combat-only window by summing untimestamped CSV values or
discarding an assumed frame count. Report combat-window rolling-FPS median/min
and time below a declared rate separately, with sample ages/RTTs; those are
500ms throughput aggregates, not frame-time percentiles. Report per-pair absolute
and relative changes, CPU core-seconds, sampled combat memory range and whole-run
peak. Three pairs give a screening comparison, not statistical equivalence.

Predeclare any numeric regression budget with the parent before execution. This
request supplies none: report measured deltas and pair-to-pair spread, not an
invented “zero-cost” or performance PASS threshold. Resource guard trips, missing
CSV, serious sampling gaps or mismatched fixtures make the affected pair inconclusive.

## Deliberate underperformance / capture-preservation check

Use **2 additional serialized fresh INI-on launches**, same fixture: one unperturbed,
one with a single5s CPU-quota pulse during combat, preceded/followed by≥10s recovery
windows. Parent targets only its owned Eden unit, records original `cpu.max`, and
restores it using an independent bounded controller even if GUI requests stall.
An eventual command is `systemctl --user set-property --runtime <OWNED.unit>
CPUQuota=100%`; restore the original quota afterward (empty `CPUQuota=` when
originally unlimited). This restricts the unit to one core's quota; it creates
contention/throttling without a competing stress process. Parent must record
actual `cpu.max`/`cpu.stat` before/during/after. No adaptive stress escalation.

Require an **observed** multi-update FPS reduction/stall plus increased cgroup
throttling, and recovery afterward. If the pulse has no effect, label perturbation
unproven; do not infer drops from the quota command. A50% `core.speed_limit` pulse
is an alternative pacing test, explicitly weaker than forced underperformance.
Neither case is a verified presentation-only frame-drop injector.

Save normally. After full shutdown use the parent's existing stopped inspector:
require exact matching committed native ID/generation, no unexpected truncation,
all retained canonical events, and exact per-entry projected sample equality with
the companion cache. Include CPU/human channel sizes and actual match-timer/result
evidence. Use held-input plateaus longer than the perturbation with leases renewed
before/after; a stalled GUI may delay lease expiration, so command timing is not
guest input timing. Include visible combat/hitstop through the same fixed fixture;
do not infer missing inputs from animation freezes or expect a fabricated60×timer
sample count. Review whether repeated native samples survive projection.

This can accept **native-stream archival preservation during observed slowdown**.
It cannot accept “every simulation tick captured,” “no guest inputs dropped,” or
“N presentations deliberately discarded.” Revision27's native serializer counter
at`+0x200078` remains opaque; event `Clocks::NONE`, trace-log timestamps and
screenshot FPS do not fill that gap. Native replay/cache equality is not an
independent simulation oracle, and timer endpoints are coarse gameplay evidence.

## Exact remaining gaps / smallest instrumentation if stronger claims are required

- **Combat-only frame tails:** existing CSV has no timestamps/index query. The
  smallest Eden addition is a read-only history-index mark/snapshot API with
  steady-clock phase markers, bounded copies and no new per-frame logging or
  allocation. Inspect existing history rather than adding a profiler. This would
  require a separately pinned emulator build; do not silently replace the assigned
  Eden binary for final performance.
- **Actual presentation drops:** FPS counts renderer completions
  (`src/video_core/gpu.cpp:172`), not display consumption. No inspected CLI method
  supplies discarded-image totals or deterministic presentation suppression.
  A verified present-only skip gate/counter, downstream of guest execution, would
  be additional emulator instrumentation, not a setting already available here.
- **Simulation/input completeness:** needs an independently validated guest
  once-per-simulation/input-consumption clock correlated to native records,
  including hitstop semantics. Minimal diagnostic guest-side counters and a
  match-end summary are narrower than per-frame logs, but the correct boundary
  is not proven by these sources. Do not attach meaning to the serializer offset.
- **Allocation behavior / isolated file-write duration:** no existing counters or
  monotonic worker-span endpoint found; cgroup memory and publication latency are
  the honest existing substitutes. Stronger claims need narrow counters/timers in
  a diagnostic mod, then normal27 performance measured separately.
- **Deluxe:** parent supplies proven fixture and actual effective mode. Repeat
  the same OFF/ON strata only when runtime/mode is established. Inactive Auto in
  ordinary Smash establishes loaded-plugin coexistence, not active Low-Lag costs.
  This worker did not audit naming, Deluxe provenance or runtime acceptance.

## Worker identity, cleanup and raw errors

Exact native Codex thread `01a12ec7-3f7e-7253-9bac-df8235cb2b3d`; read-only exact
`state_5.sqlite` row and latest rollout `turn_context` both
`gpt-6.1-sol/xhigh`, verified initially and at cleanup. Parent marker
`ses_eda4c5e1affe9opq1xJdL8HYIt` was not used as this worker's identity.
Owner Codex PID**3576314**, start ticks**14903243**. Watcher PID**3578301**,
temporary directory `/tmp/live-perf-watch-wco5kg_t`; no terminal, polls only exact
model/effort and owner liveness/start identity, exits with owner. At cleanup SIGTERM
handled, `exited` recorded, PID verified absent and temporary directory removed;
no model/effort-change event. No operational resources were created.

Raw read-only discovery errors (corrected by locating actual frontend config):

```text
rg: /home/intpa/eden-cli-control/src/yuzu/configuration/config.cpp: No such file or directory (os error 2)
rg: /home/intpa/eden-cli-control/src/yuzu/configuration/config.h: No such file or directory (os error 2)
```

An initial filename filter accidentally matched the `eden-cli-control` path prefix
and was truncated by the tool (`Warning: truncated output (original token count:
76558)`). It was replaced with basename glob discovery and narrow symbol searches;
no giant logs were read or retained. A no-match `rg` returned exit1 with empty
output. No execution/build/runtime error occurred because none was attempted.
