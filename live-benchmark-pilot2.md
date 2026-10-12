# Live benchmark pilot2 — rejected fixture, closed and restored

Pilot2 did not qualify the fixed benchmark fixture, warmup driver, or measured driver. Batched navigation selected Isabelle instead of Shulk and then started an unintended match against the still-Random CPU, which resolved to Rosalina & Luma. The game exited normally and the original profile, including its full cache, was restored exactly. There is no performance conclusion or benchmark acceptance from this run. Product revision 27 remains frozen.

## Scope and pinned candidate

This was one normal revision-27 run with capture ON and no Deluxe, token `27-bench-pilot2`, phase `live-r27-bench-pilot2`. Its intended fixture was Shulk versus Pikachu CPU level 9, Battlefield Normal, Time 60 seconds, items None and hazards Off, reusing fixture2's complete native saves and prewarmed cache. Intended work was warmup/cancel, a 40-second measured match, and native replay saving; those steps were not qualified.

The receipt pins the 3,346,432-byte Training NRO to SHA256 `4d6cab005296ea7e95082289f3825e4c6ad8e9cc3aefad3d37e391657c7daac7`, with `waiting-roster-reload`. Its profile was `/home/intpa/eden-replay/acceptance-fable/profile`. The screenshots show volume 0%; no emulator or profile was opened or changed by this documentation worker.

## What the archived images establish

All image paths below are under `/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/`. The worker inspected the actual images; these observations agree with the parent's inspection.

| Artifact | Actual state | Qualification consequence |
| --- | --- | --- |
| [navigation-preset.png](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/navigation-preset.png) | `Capture benchmark 60s`, Time `1:00`, Spirits Off, first to 1 win | Correct persisted preset; this image does not independently expose all item/hazard settings. |
| [navigation-battlefield.png](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/navigation-battlefield.png) | Battlefield, Normal | Correct stage selection checkpoint. |
| [navigation-initial-css.png](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/navigation-initial-css.png) | P1 unselected; CPU Random, level 9 | Cold fixture reuse did not preserve the required fighter selections. |
| [navigation-cpu-pick.png](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/navigation-cpu-pick.png) | Black game viewport during loading | Does not prove a CPU-token or fighter selection checkpoint. |
| [pilot2-css-state.png](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/pilot2-css-state.png) | Actual Battlefield combat: Isabelle versus Rosalina & Luma, `0:37.51` remaining | Wrong P1 and random CPU match; filename does not describe the actual phase. |

The action journal records the first fighter-confirmation A at `2026-10-12T08:56:14.247390+00:00`, followed by right/down navigation and another A at `08:56:16.749062+00:00`; the black loading screenshot followed at `08:56:17.786209+00:00`. The parent's inspected sequence identifies the first A as the wrong P1 selection and the later A as the unintended match launch. Frontend `accepted`/`observed` responses explicitly retain `guest_delivery: not_claimed`; they do not validate the intended native selection.

The earlier helper/handoff claim that the batch “never starts a match” is contradicted by these artifacts and superseded by the 09:07 UTC handoff checkpoint. The demonstrated navigation failure is unverified selection and transition before confirmation, not evidence of a product performance defect. The parent owns the navigation correction and subsequent qualification; this worker did not modify the helper.

## Exit, limits and closure

[run.json](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/run.json) and [benchmark-closure.json](/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/benchmark-closure.json) record:

- Game unit: `eden-control-c26e9e674b77.service`.
- Start `1791795287.4532359`; end `1791795482.1136544`; elapsed `194.65963275299873` seconds, rounded **194.659633 seconds**; reason `exited`, exit code **0**.
- Verified live limits: `MemoryMax=8589934592` bytes (8 GiB), `MemorySwapMax=0`, `RuntimeMaxUSec=15min` (900-second cap). The parent target was 480 seconds, with independent normal stop at 660 seconds, a 3 GiB host available-memory floor and launch floor above 10 GiB. Preflight recorded 19,143,040 KiB available and no conflicts.
- Peak observed game-unit memory **5,671,776,256 bytes**. Recorded `memory.events` low/high/max/oom/oom_kill/oom_group_kill and sock_throttled counters were all zero. No OOM, memory-max event, or swap use was reported; the manifest's no-swap limit was zero.
- Final status: `powered_on=false`, `running=false`, no pending game operation, no modal. [stopped-status.json](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/stopped-status.json) and [stop-confirmation.png](/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/stop-confirmation.png) preserve shutdown evidence.
- All five recorded runtime/monitor PIDs absent per closure: **3737374, 3737383, 3737384, 3737456, 3737447**; closure `errors=[]`.

The parent independently verified the game unit and stopped-evidence close unit `training-live-parent-bench-pilot2-close-1791795940` as **not-found / inactive / dead / MainPID 0**. The close operation passed in **26.663 seconds**, peak **345.7M**, with no swap. Its granted inspection bounds were 1 GiB memory, CPU 100%, no swap and 180 seconds. These independent unit checks and close resource figures are parent-provided evidence, also recorded in the latest handoff; this worker did not execute unit or resource-control operations. The pilot2 runtime/check lease was released at 09:06 UTC. Later parent pilot work is outside this report.

## Exact restoration and retained evidence

[receipt.json](/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/receipt.json) is `state=restored`, `benchmark.cache_restored=true`, `benchmark.restore_state=done`, with restored trees `config`, `saves`, and `sdmc`. Closure records **`restoration_exact=true` at `1791795945.927073`**. The parent verified the complete original cache plus these original trees, protected keys/load trees, and ARC-only baseline. This is pilot2's recorded restoration, not a claim about the profile during any later run.

Pilot2 includes full original-cache backup and rollback. Earlier historical fixture qualification runs lacked that original-cache rollback; their preservation claims must not be broadened retroactively. The preserved post-run cache may differ from its original, as expected, while the original cache was restored.

Exact preserved paths:

- Fixture saves source: `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-fixture2/after/saves-1791792296680098112`.
- Fixture cache: `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-fixture2/prewarmed-cache`; manifest `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-fixture2/prewarmed-cache-manifest.json`.
- Pilot2 post-run config: `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/after/config-1791795945459170499`.
- Pilot2 post-run saves: `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/after/saves-1791795945474017949`.
- Pilot2 post-run SDMC: `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/after/sdmc-1791795945492557750`.
- Pilot2 post-run cache: `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/after/cache-1791795936937776822`.

The authoritative run directory is `/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-pilot2/`, including `cold-navigation.jsonl`, `cli-actions.jsonl`, `control.jsonl`, `console.log`, `runtime-identity.json`, `preboot-settings.json`, `memory.events`, `memory.peak`, `cgroup.events`, and the images above. The authoritative receipt/closure directory is `/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/`.

## CSV scope and acceptance limits

[whole-session-frame-times.csv](/home/intpa/smash-training-lobby/research/named-macros/live-acceptance-27-bench-pilot2/whole-session-frame-times.csv) contains **7,363** headerless millisecond samples; SHA256 **`8ea46ad34ede8f7632e76bb463a09fdbf60704f18baad7aaaf5d878c14c69945`**. This worker independently read its line count and file hash, matching closure. Original source: `/home/intpa/eden-replay/acceptance-fable/profile/data/eden/log/2026-10-12-04-57_01006A800016E000.csv`.

These are whole-session composition intervals spanning boot/menu/loading and unintended combat, with Eden's first five intervals omitted. Closure's generic scope string also names warmup/results; it does not establish that qualified warmup or results occurred in pilot2. The CSV is neither an isolated measured window nor a comparable OFF/ON dataset. Its raw closure summary is mean 20.2611336873557 ms, p50 16.6707 ms, p95 33.355 ms, p99 49.969 ms, maximum 1783.51 ms, using nearest-rank quantiles. These descriptive values support no capture-overhead or performance verdict.

Closure has `performance_acceptance=false`, `analyses=[]`, empty native bound keys and persisted archive IDs, `new_native_replays={}`, and no capture inspection directory. Capture-enabled log evidence is `true`, but **no measured collector, new native replay save, or new companion capture was produced**. Existing fixture replays in the overlaid saves are pre-existing evidence, not pilot2 saves. No fixed fixture, warmup/measured-driver qualification, paired comparison, slowdown/recovery, or native-save acceptance is claimed.

## Documentation worker provenance

Bounded documentation worker native thread `01a12f5d-4463-7ff2-9a59-53e62160157d`, delegated from `ses_eda4c5e1affe9opq1xJdL8HYIt`. Exact thread database fields and latest native rollout `turn_context` both verified **`gpt-6.1-sol` / `high`**, rather than configuration defaults or a parent thread. Owner Codex PID **3748528**, process start ticks **15886412**. Temporary watcher PID **3750094** polls this exact thread's live model/effort and owner liveness/start identity; its expected pair is the verified launch pair. Temporary files are under `/tmp/pilot2-doc-01a12f5d-model-watch*`, outside the repository.

Only this report and temporary identity watcher files were written. No subdelegation, tests, project imports, builds, games, profile/helper/product writes, shared-ledger writes, or runtime/resource controls were performed. The recorded dispatch snapshot was 09:06:07 UTC: documentation pools claude/codex/cursor/opencode eligible, Claude signal incomplete, all zero in flight, ring after last Claude chose Codex; Cursor 12.7%, Codex 16.1%, OpenCode 18.9% were informational only. Last gateway remained `nvidia-b`.

Watcher closure: live pair stayed `gpt-6.1-sol/high` through final poll at **1791796300.7228622**; watcher emitted `state=stopped` and its execution session **13837** completed with exit **0**. A subsequent read-only process-presence check confirmed **/proc/3750094 absent**, establishing the watcher was reaped before final handback. Owner PID **3748528** was not stopped. Temporary closure evidence remains in `/tmp/pilot2-doc-01a12f5d-model-watch.json`.
