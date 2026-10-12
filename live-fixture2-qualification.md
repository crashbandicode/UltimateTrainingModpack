# Fixed live-match fixture and collector smoke — revision27

2026-10-12. **Native fixed-fixture warmup/save and collector feasibility passed.**
Matched performance comparisons and deliberate slowdown remain pending.

## Fixture

Normal27 `4d6cab005296ea7e95082289f3825e4c6ad8e9cc3aefad3d37e391657c7daac7`,
no Deluxe, capture INI on; pinned Eden `64cf47519891440f4a5adfe4d3dfeff180fa4d90b61eb424ee1550e2a3fc3004`.
Owned run `/home/intpa/eden-replay/acceptance-fable/runs/live-r27-fixture2`.

- Native preset **Capture benchmark 60s** was saved in fixture1 and independently
  read on this cold launch (`fixture2-presets.png`): Time1:00, SpiritsOff, first1win.
- Native fixture1 screenshots establish itemsNone, hazardsOff and restricted random
  stage list. This run explicitly selected **Battlefield, Normal form**
  (`fixture2-stage-final.png`), rather than relying on random selection.
- **Shulk human / Pikachu CPU level9**, both explicitly selected
  (`fixture2-pikachu-fixed.png`); initial CPU Random was replaced before launching.
- `fixture2-match-start.png` shows actual Battlefield combat at0:56.91remaining.
  `fixture2-warmup-end.png` shows results; `fixture2-native-saved.png` confirms the
  native replay saved. The input pattern exercised fractional axes and attack/jump.
  Frontend acceptance alone is not guest-delivery evidence.

Native keyed `save_bound generation=1 index=0` followed by `persisted` ties the
companion to **replay202610124136.bin**, titleReplay, bundle
`7265706c61793230323631303132343133362e62696e.smd`. Stopped inspector:

| Entry | Fighter kind | Retained inputs | Nonneutral | Analog |
| --- | --- | ---: | ---: | --- |
| 0 | Shulk57 | 3600 | 483 | Unquantized native f32 values retained |
| 1 | Pikachu8 | 3600 | 3600 | Quantized values observed in native CPU stream |

No skipped capture, `truncated:false`, exact canonical/cache projection. These are
retained-stream/lifecycle results; no independent every-simulation-tick proof.
Full inspector JSON/log and closure receipt are in `live-acceptance-27-fixture2/`.

## Collector qualification

`live_perf.py` and `test_live_perf.py` were delivered source-only by Solmedium,
then reviewed and tested by the parent. **15 synthetic tests passed** under
`training-live-parent-perf-tests-1791791430`,1GiB/noSwap/CPU100%/180s,
exit0/226ms/23.4MiB. Coverage includes quantiles/tails, malformed and bounded CSV,
ownership/path escape, clocks/gaps/reset exclusion, rolling-cache ambiguity, and
closed collection boundaries. Worker owner3651257/watcher3652305 absent, wrapperclosed.

Live5s navigation smoke and45s warmup smoke exercised exact pinned status, owned
cgroup sampling, normal-shutdown CSV preservation and offline analysis. The45s
collection `perf-0a358b1b6a4341639233ad42a31693a9.jsonl` has90/90status observations,
45resource samples, no missing slots/RPC/read errors, median RTT0.391ms/max3.526ms.
One shader-work observation occurred. The parent label does not establish that
every poll was in combat; this is collector smoke, not a qualified benchmark window.

Its90rolling-FPS observations have min25.012/median58.461/max60.057. CPU counters
cover43.999139s,55.161530core-seconds, mean1.253696cores. Sampled charged memory
5704937472–5781606400B, swap0. No performance budget or OFF/ON delta inferred.

Whole-session CSV:33131samples,mean18.877996ms,nearest-rankp5016.6724/p9533.2728/
p9938.6327,max1816.68;1414samples>33.33ms,61>50ms. It includes menus/warmup/results.
SHA256 `ec383d907210cc799b561a973c2ad17cf9c67942f5e1230c8679b77dbe0874b7`;
byte-identical copies and full statistics are in the two `perf-analysis-*` reports.

## Cache, preservation and closure

Full stopped **profile/cache**, including Eden and NVIDIA subdirectories, preserved
as `live-acceptance-27-fixture2/prewarmed-cache`:7files/122065737B, exact tree hashes
in `prewarmed-cache-manifest.json`. Equal starting-cache restoration and cold fighter
reuse still need qualification before pairs. CPU AI/RNG and wall-time inputs remain
uncontrolled variability even with matched fixtures.

Run started1791791477.1960557 and exited normally0 at1791792140.5023525,
663.305475s. Automatic660sstop succeeded without a naming modal. Parent480starget
missed183.305475s; real modal-retry behavior is still unexercised. Peak6013255680B,
zero max/OOM/swap events. Unit`eden-control-a0688dbc1809.service` independently
not-found/inactive/dead/MainPID0;3676487/3676493/3676494/3676552/3676565 absent.

Stopped inspection/cachecopy/CSVanalysis/restore unit
`training-live-parent-fixture2-close-1791792270`:exit0,17.317s,389MiB,swap0,
1GiB/noSwap/CPU100%/180s, independentlyclosed. Exact original config/save/SDMC,
protectedkeys/load and ARC-only inventory restored at1791792297.1348326.
All after-state preserved in receipt:

- config-1791792296674725296
- saves-1791792296680098112
- sdmc-1791792296700370278

No resource lease remains. Original operator profile untouched. Product source
remains frozen27; no new NRO build, implementation commit/push/release/deployment.
