# Revision27 — keep export naming valid across ordinary preview restore

2026-10-12 05:52UTC. Same branch single-file-waiting-ui, HEAD
a16e5ebaff65cdf2f1df6f6e3feb0f28cb5a0400 plus inherited changes. Only
src/training/named_macros.rs differs frozen26; no commit/push.

Rootcause/evidence: revision26-qualification.md. SESSION_GENERATION is advanced on
clear_session alongside motionGENERATION; NameJob uses sessionepoch. Ordinary
begin_restore advances onlymotion. Matchingrequest-id/scene/mode guards retained;
no changed capture/save-to-native/shared/archive/preview input behavior.

- Normal strict1791784116/release1791784138 PASS.
- Trace strict1791784220/release1791784237 PASS.
- Normal3346432B SHA4d6cab005296ea7e95082289f3825e4c6ad8e9cc3aefad3d37e391657c7daac7.
- Trace3342336B SHA7f8863618d0c6dc0fd0d34f94cd2a40851b59332383abb005e64a20510f000eb.
- Both118product+12externalpins; immutable revision27-builds.19nativeguards/source
  unchangedfrom26 verifiedownedELF; no redundantELFjob. DiffcheckPASS,0loadedcheckunits.
- Native27acceptance pending;26bufferlifecycle/coldpreview evidence is historical,
  not proof that this naming-fence change persists a macro.

## Native evidence superseding the pending checkpoint — 2026-10-12 06:20UTC

Owned clone `/home/intpa/eden-replay/acceptance-fable/profile` only, pinned Eden
10bcd2d849843b146a79c61a21df615e1bdcfcde/game13.0.5/trace27, muted CLI-only.
Each game bounded900s/8GiB/noSwap with independent3GiB host floor. No concurrent
build/game, debugger, diagnostic environment flags or physical deployment.

### Capture and exact native save

- `live-r27-off`: native-created `[capture] enabled=off`, main-menu screenshot,
  console4136 at26.911034 `live_enabled=false`, no optional-adapter installation.
- `live-r27-on`: Shulk57P1 vs Pikachu8CPU9, Hollow Bastion,3stocks/7min. Observed
  results0:53,3CPU KOs/no SD. Fractional stick(-0.375,0.2) exercised; later jump/attack
  commands reached results and are not claimed as gameplay exercise for this run.
- Console4136 at26.336161 enabled=true;4178 at26.436361 guarded adapter ready;
  4638 at146.969788 record_start generation1/index0.
- Console25440 at207.833019 `fram_publish generation=1 index=0 native_counter=3550`;
  25442 at207.833074 matching `stop epoch=5 counter=Some(3550) partial=false pending=0`.
  No native-state or velocity rejection observed.
- Cancelled native Save Replay produced no capture files, save_bound or persisted
  event. Subsequent confirmation:25444 at337.590704 save_bound generation1/index0;
  25447 at338.191909 persisted matching native ID `replay202610122158.bin`, title
  `Replay`, archive ID `7265706c61793230323631303132323135382e62696e`.
- Stopped-only inspector `training-live-parent-inspect-1791784978367474516`, exit0,
  1GiB/noSwap/CPU100%/300s: skipped=[], truncated=false, cache_matches_projection=true.
  Both3232frames: entry0 kind57/nonneutral150/unquantized analog; entry1 kind8/
  nonneutral3232/quantized analog. Canonical manifest16165events, completion complete.
  Inspector binary SHA2561569c2121a21fe45aa1d7577389201b7c1abb02f588e64e6316aed90f84d58d8.

### Cold Training access, preview and persisted export

- `live-r27-reopen`: cold INIoff confirmed console4488 at30.287391, no optional live
  adapter. Picker showed `Replay · 3232f` without Vault recapture.
- Selecting entry1 automatically reloaded Dr.Mario to Pikachu. Saved a matching
  Pac-Man/Pikachu Training setup, then cropped the captured CPU track to `[0,8)`.
- Native preview revision111: native_play at680.198746; inputs_complete frames8
  at680.312559. Writer indices exactly0..7; observed gameplay included jump/attack
  motion. This is eight consumed inputs, not eight render frames.
- Export requested immediately after preview; restore_stop then keyboard id1
  result0x0/consume/applied at738.891795..738.891883. These log writes are buffered;
  they do not establish microsecond timing of keyboard or restore operations.
- Software keyboard text set via owned Qt CLI to `R27 Pikachu eight`, native OK
  clicked. `reopen-export-confirmed.png` shows modal/editor closed and Training
  returned to baseline; position-restored CPUkind8 logged at738.992125.
- New `macros/r27-pikachu-eight.json`, schema1/game13.0.5, exact requested name,
  kind8,8frames. Start matches captured entry1 start; frames exactly match cache
  `[0,8)` and independently projected canonical native-f32 axis bits. Placement
  retains zero deltas/preserve_recorded. Selected ID is `r27-pikachu-eight`.
- All12pre-existing macro JSONs are byte-identical to before-state; this is the
  only new macro. Export SHA256:
  `babd16c56b6331fd27eb7858719d27d4555c29accc03acfbddfb604de9b99b3b`.
- Read-only stopped verification `training-live-parent-export27-1791786050` exit0
  under1GiB/noSwap/CPU100%/300s,40MiB peak/0swap. Receipt:
  `live-acceptance-27/export-verification.json`. Verification script:
  `/tmp/opencode/training-r27-export-verify.py`.

### Shutdown and preservation

All3games normalexit0, no memory max/OOM/swap events. Peaks off4669706240B,
on5603647488B, reopen5111496704B. Reopen ended1791785803.3494773/06:16:43.349UTC,
elapsed753.546s: **missed conservative06:16 target by43.349s**, but stopped normally
before the independent900s cap. Do not describe this as meeting every stop target.

Units304c67a91fa5/c531dfc366a9/59ce7388b0ac independentlynot-found/inactive/dead/MainPID0;
all9launcher/guard/Eden PIDs absent. Inspector/export verification units independently
not-found/inactive/dead/MainPID0. No fourth game was launched.

`live_acceptance.py restore 27` verified restoration at1791785976.9214873:
exact before config/save/SDMC trees, protectedkeys/load unchanged, ARC-only plugin
baseline. All after-state preserved under `live-acceptance-27/after`, including:

- `config-1791785976424768964`
- `saves-1791785976441462773`
- `sdmc-1791785976464766585` (capture archive and exported macro).

Receipt `live-acceptance-27/receipt.json` is restored. Historical per-run `launched`
entries describe launch actions; run manifests/independent closure establish stopped
state. Runtime lease05:53→06:35 explicitly released early06:20; original profile
untouched. No commit, implementation push/release or physical deployment.

### Scope still pending

This accepts the observed native-buffer lifecycle, committed-save publication,
cold access with capture off, eight-input preview, and the naming/persistence
regression. No second cold restart after exporting the macro was performed.
First/last gameplay-tick coverage and input/state phase remain unqualified; clocks
remain empty and serializer counter opaque. Native-buffer completion does not
upgrade partial channel coverage, actual-rules/stage/controller metadata, or prove
the entire simulation was observed. Controlled off/on overhead, deliberate frame-drop
behavior, worst-case memory/queue pressure and current Deluxe coexistence remain
separate pending acceptance work. Physical Switch/network behavior remains pending.

## Independent naming-fence review — 2026-10-12 06:26UTC

`live-name27-review.md`, native ClaudeOpus5.5/xhigh session
8dbb82fb-2068-4d96-8e81-c249078ec655, concludes correct/minimal with no blocking defect.
It checked all relevant clear_session/begin_restore callers, mode/request-ID/draft
guards, and unchanged motion-restoration fencing.114frozen Rust/TOML files matched
product source. It did not duplicate parent runtime/content verification.
Wrapper stopped; owner3568838/watcher3570074 independently absent. Prior failed
OpenCode attempt/resume remains a failed route, not an approval.

The reviewer identified a possible second restore when successful export closes
an editor already stopping. Parent checked the native27 log: this **did occur**,
restore_stop at738.891795 then738.992160 (after applied), followed by restored_idle
at739.092342. Export/preservation checks still pass and the baseline settled. This
bounded redundant restore is recorded as a low-severity follow-up; no source change
was silently applied to frozen27. Cancel/Failed keyboard results after preview
restore and an additional cold restart of the newly exported macro remain untested.
