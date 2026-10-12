# Baseline OFF attempt — combat window retained, native save incomplete

Phase `live-r27-bench-b1off`, campaign `27-bench-b1off`. Normal27/noDeluxe/captureOFF,
same exact fullfixture2 saves and prewarmed diskcache. Fixedrules/BattlefieldNormal/
Shulk/PikachuCPU9 visually inspected. This is the first match in a fresh process,
as prospectively declared in live-performance-campaign.md, not warmed-process data.

`measure-window-start.png` shows0:57.05remaining; end shows0:16.81remaining, both
correctcombat. Collector09:31:53.331445→09:32:33.331825UTC completed40s,80/80valid
statusobservations and40resource samples; no missingstatus/RPCerrors/shaderwork.
RollinggameFPS min24.006986/median50.006049/max54.038732. These are polling aggregates,
not independent frames or a capture-overhead conclusion. CPU45.895851coreseconds
over38.998938coveredseconds=1.176849meancores; memory5493866496–5507072000B/swap0.
Detailed raw/analysis paths under
`/home/intpa/eden-replay/acceptance-fable/runs/live-r27-bench-b1off/`:
- `perf-2f0180284c3040a69272993874590aed.jsonl`
- `perf-analysis-5cdea2130a6e4d56a2be327d2f7677e6.json`
- `drive-measure.jsonl`, `cli-actions.jsonl`, fixedfixture/boundary screenshots.

`native-save-dialog.png` proves actual SaveReplay question/No selected, but parent
confirmation helper refused its remaining20smargin before issuing controls. No
newnative replay/save_bound/persist/companion. Independent465s normalstop completed
at1791797664.6373258,469.042228s/exit0, within480target. Subsequent parentstop saw
absent socket because the independentstop had already closed the frontend.
Not a completed protocol run or matchedpair. The unused b1on grant was cancelled.

Gameunit07342e1bb3ac and closeunittraining-live-parent-bench-b1off-close-1791797740
independentlynotfound/inactive/dead/MainPID0; all5runtimePIDs absent perclosure.
ClosePASS28.837s/607M/noSwap; exactoriginalfullcache+config/save/SDMC restored
1791797775.461606, protectedkeys/load/ARC-onlyverified, errorsnone. See
`live-acceptance-27-bench-b1off/benchmark-closure.json` and `receipt.json`.
Whole-sessionCSV21584samples/SHA256
`0c373db7fd90b4fa0676e9870215afd63390ac6847a8a679035a46f6ed87fe6d`, preserved there;
its boot/menu/combat/results scope is not combat-only frame tails.

Root operational issue: model/tool round trips between measurement, results and
confirmation exhausted the reserved margin. New `live_benchmark_drive.py measure
PHASE --save` performs results/save/normalstop as one bounded operation after the
collector finishes. It verifies the actual nativequestion and selectedNo against
the pinned screenshot, failing closed on mismatch. Parent still reviews all saved
images and exactnative association/projection afterstop; no automatic acceptance.
16focusedtests passed including a separate realnative dialog and wrongphase/text/
buttonrefusals plus stop-on-finish-failure. The new finish path is NOT livequalified.
