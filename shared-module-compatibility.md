# Shared match-data module compatibility audit (source-only)

Status: COMPLETE 2026-10-11 (source-only; nothing built, tested, executed or deployed).

- Auditor: Claude Code native worker, session `0497e6b0-f08a-4be8-bbbd-45fea4d16f67`, host `ubuntu-surface`.
  Exact transcript pair `claude-opus-5-5` / `xhigh` (Research/source-audit row of the routing table); owner PID 3311952.
- Delegated from `ses_eda4c5e1affe9opq1xJdL8HYIt` (parent Codex Astra xhigh). Parent owns architecture/integration.
- Read-only inputs: `/home/intpa/smash-match-data-design` (no VCS), `/home/intpa/smash-replay-export`
  (HEAD `70bbd9b2`), Training candidate `/home/intpa/smash-training-lobby/UltimateTrainingModpack-single-file`,
  pinned Switch std `/home/intpa/eden-gnw/tools/cargo/skyline/toolchain/skyline/lib/rustlib/src/rust/library/std/src/sys/fs/switch.rs`,
  Eden `/home/intpa/eden` rev `10bcd2d849`, skyline-smash checkout `01bf583`.
- Not touched: nativeclock review and staginghelper (other workers), older `/home/intpa/smash-match-data` material.

## 1. Answer: does the manifest rename need a platform publication adapter?

**Yes. This is a concrete blocker, proven from source.**

- Shared `Recorder::create` → `ArchiveWriter::create` writes `manifest.json` (open) through
  `archive::write_manifest`: it writes `manifest.json.tmp`, then renames it onto the *absent* `manifest.json`
  (`src/archive.rs:171-186`, `:252`). `Recorder::finish` → `ArchiveWriter::finish` → `write_manifest` again
  renames onto the *existing* `manifest.json` (`src/capture.rs:426-427`, `src/archive.rs:320-333`).
  `Capture::save` (`src/archive.rs:530-566`) and `WriteOptions { overwrite: true }` hit the same path.
- Pinned Switch std: `fs::rename` runs `stat(old)`, then calls `nnsdk::fs::RenameFile` (or `RenameDirectory`)
  directly (`switch.rs:665-679`). It has no replace semantics.
- Eden: `IFileSystem` backend = `VfsDirectoryServiceWrapper` (`core/file_sys/fsa/fs_i_filesystem.h:146,203`).
  `RenameFile` with the same parent and an existing destination returns `ResultPathAlreadyExists`
  (`core/hle/service/filesystem/filesystem.cpp:188-197`). HOS documents the same RenameFile contract (not verified here).
- Result on device: `finish` returns `CaptureError::Archive(Io{manifest.json})`. The timeline and `end` line exist,
  but `manifest.json` stays `open`, and the final manifest is left in `manifest.json.tmp`. Readers then report
  `Incomplete{ManifestOpen}`, and the shared adapters refuse the capture (`adapters.rs:733-739`).

**Minimal Training-side adapter (no shared-crate change). It uses only public shared API:**

1. `smash_match_data::validate::StreamChecker::new(&header)`; then `check(&event)` per event. Reject any `is_error()`.
2. `archive::ArchiveWriter::create(staging_dir, Manifest::new(header), &WriteOptions { flush_each_event: false, overwrite: false })`.
   This renames onto an absent path only, so it is safe. Then `write_event` per event, assigning `seq` 0.. consecutively.
3. Write the end line: `Event { seq: n, clocks, body: End { events: n } }`. Check it first, then `write_event` and `flush()`.
   On Switch, `File::flush` = `nn::fs::FlushFile` (`switch.rs:408-411,517-519`).
4. Build the final manifest from `writer.manifest().clone()`:
   - `completion` = `Completion::complete()` or `truncated(reason)`;
   - `completion.events = Some(writer.events_written())`, `completion.last_seq = writer.last_seq()`;
   - `boundaries = checker.boundaries().to_vec()`, `actors = checker.actor_summaries()`.
   This mirrors `Recorder::finish` at `capture.rs:410-433`.
5. `drop(writer)`, then `fs::remove_file(staging/manifest.json)`, then `archive::write_manifest(&staging, &m)`.
   That write is again a rename onto an absent path.
6. Publish by directory rename. The staging dir must be a **sibling in the same parent**:
   - Eden `RenameDirectory` across parents is `ASSERT ... UNIMPLEMENTED` (`filesystem.cpp:233-245`).
   - Eden's same-parent rename does not check the destination, so check that the final dir is absent first,
     as `capture_archive.rs:271-295` already does for files.
   - Name staging dirs with a leading dot so listing ignores them.

Steps 4–5 duplicate about 15 lines of `Recorder::finish`. A host parity test catches drift (T3 below).

## 2. Other concrete blockers and constraints (source-proven)

| # | Item | Evidence | Minimal resolution |
|---|---|---|---|
| B2 | `Vec3` needs `z` for both `Kinematics.position` and `velocity` (not optional). Training samples only `pos_x/pos_y` and `get_sum_speed_x/y`. Zero-filling would invent data. | `model.rs:496-500,537-544`; `replay_capture.rs:407-417` | Read `PostureModule::pos_z` (f32, `smash cpp.rs:10094`) and the z of `KineticModule::get_sum_speed3f(MAIN)` (`cpp.rs:16918`; there is no `get_sum_speed_z`). Keep the existing x/y getters. If speed3f cannot be qualified, `velocity` cannot be filled honestly; ask the shared owner for an optional/2-D velocity rather than writing 0. |
| B3 | `FighterSample` has no per-frame fighter kind or costume; identity exists only on `Spawn`/`FormChange` lifecycle events. Lifecycle events are rejected unless `channels.lifecycle` is non-unsupported. | `model.rs:367-395,618-623`; `validate.rs:468-475,761-777`; FORMAT.md:110-115 | Producer emits `spawn` before an entry's first sample. On a kind change with the same `battle_object_id`, emit `form_change`. On an object-id change, emit `despawn{transform{to}}` + `spawn{transform{from}}` on a new actor. Store `battle_object_id` per frame (+4 B). It is already read as `Pending.body` (`live_replay.rs:363-364`). Declare `lifecycle: Supply::partial(...)` with the note "no death/respawn observed". Without this, the projection cannot restore per-frame `fighter_kind`, and the editor's transformation guard (`replay.rs:52-54`) breaks. |
| B4 | The live pool stores rounded `InputFrame` (i8); floats are lost at `after_frame`. | `replay_capture.rs:448-452,478-489`; `live_capture.rs:100-145` | `observed_input` returns raw `get_button` bits plus 4 raw f32 axes. The pool stores a float frame. `quantized` is computed only in the projection. Extend `valid()` (`live_capture.rs:178-181`) to every float that is written, including sticks, `pos_z` and `speed_z`; otherwise `StreamChecker` NonFinite fails the whole save. |
| B5 | The shared quantizer cannot reproduce Training's i8 conversion. Shared computes `(v-origin)/span*max_abs` and clamps to ±`max_abs` (≤127); `HalfAwayFromZero` is `+0.5` then `trunc`. Training uses `round(v*120).clamp(-128,127)` and sets `quantized` when `abs(v*120 - rounded) > 0.001`. The two differ at least on the clamp range (±120/±127 vs −128..127) and on the f32 tie edge (`0.49999997 + 0.5` rounds to `1.0`). | `adapters.rs:147-173,278-314`; `replay_capture.rs:22,448-452` | Do **not** use `TrainingQuantizeOptions`. Move Training's `axis()` and `STICK_UNITS = 120.0` into a Training projector and apply it to the shared `Stick::F32` values. Shared f32 survive JSON bit-exactly (`serde_json/float_roundtrip`, `Channel` uses `visit_map`). |
| B6 | `training_macro` is unsuitable on device. It needs a fully loaded `Capture` (`Capture::load`, the whole `Vec<Event>`). `resolve_input`/`input_marks` rescan every event per input (`continue`, not `break`, at `adapters.rs:986`), so cost is O(events × inputs). It also requires Observed kinematics+situation (`adapters.rs:808-846`). | `archive.rs:506-525`; `adapters.rs:976-1030,1140-1242` | Reopen with streaming `archive::read_manifest` + `archive::open_timeline` → `TimelineReader::next_event` → one-pass Training projector → `ReplaySession`. No replay feature, no FRAM grammar. Keep `training_macro`/`clip_*` for host tools and bounded clips. |
| B7 | Do not parse JSONL on the menu/frame thread. `open_capture` runs from `browser_input` (`replay_capture.rs:116-157,183-187`). A canonical file is about 12–15× the `.tmrc` size (estimate below). | — | Keep `.tmrc` (`CaptureArchive`) as the compact editor cache. The writer thread produces it by teeing the same events into the projector (one projection function serves save and reopen). Rebuild from canonical only off-thread. |
| B8 | Shared `WriteOptions::default()` sets `flush_each_event: true`, which triggers `nn::fs::FlushFile` per event. Switch std `write` already sends `WriteOption{flags:1}` (flush) on every chunk. | `archive.rs:157-164,306-308`; `switch.rs:487-494,517-519` | Pass `flush_each_event: false`. Never use `overwrite: true` or `Capture::save` on device. |
| B9 | Unbounded reads: `read_manifest` does `fs::read` of the whole file, and `TimelineReader` does `read_line` without a limit. The profile uses `panic = "abort"` (root Cargo.toml:55,59), so OOM aborts the game. | `archive.rs:191,393-396` | Check `fs::metadata().len()` against Training bounds before reopening, as `capture_archive.rs` already does with `MAX_CAPTURE_*`. |
| B10 | Path/source pin. The shared crate has no VCS. Cargo resolves the optional path dep `../smash-replay-export/replay-core` into the lockfile even with `default-features = false` (it is not compiled). The shared HANDOFF recorded replay-export HEAD `e4c1130d`; it is now `70bbd9b2`. replay-core has no dependencies and no workspace root. | shared `Cargo.toml:13-25`; replay-core `Cargo.toml` | In `training_mod_macros/Cargo.toml`: `smash-match-data = { path = "../../../smash-match-data-design", default-features = false }`. `/home/intpa/smash-replay-export/replay-core/Cargo.toml` must exist where Cargo runs (it does on ubuntu-surface). Record a source digest of shared `Cargo.toml`, `src/**` and `docs/FORMAT.md` in Training evidence. Training's lock, not the shared lock (serde 1.0.229 / serde_json 1.0.152 vs Training serde_json 1.0.151), governs, so re-prove bit-exactness under Training's lock (T1). Feature unification turns on `float_roundtrip` for Training's own serde_json (harmless). |

Portability scan of shared production code (excluding `src/bin`): there is no `process::id`, `temp_dir`, time, thread or env use.
`#![forbid(unsafe_code)]` is set. The remaining `expect`s are guarded. Only `archive.rs` touches `std::fs`.
`sha2 0.10.9` → `cpufeatures 0.2.17` falls back on aarch64 for non-linux/android/apple targets (`aarch64.rs:160`).
That suggests it compiles for the skyline target, but no target build has proven it. `File::open`/`stat` need absolute paths.
`TRAINING_MODPACK_ROOT` already meets that, and the shared code only joins onto the caller's directory.
`OpenOptions::write(true)` sets APPEND, so timeline growth works. `.create(true)` (shared) avoids the `create_new`-only quirk.

## 3. Exact shared API to use (and to avoid on device)

Use, with `default-features = false`:
- Types: `CaptureHeader`, `ToolVersion::new`, `GameBuild::ultimate("13.0.5")`, `Source::live_match()`
  (with `description` = native replay title), `ChannelSupport` / `Supply::{full,partial,unsupported}`, `InputLayout`,
  `ButtonLayout{width_bits:32}`, `AxisLayout{encoding:F32, min/max/neutral/float_scale: None}`, `FighterEntry`,
  `ActorIdentity`, `ActorId`, `Event`, `EventBody::{Lifecycle,State,Input,Raw,Gap,Boundary,End}`, `Clocks::NONE`,
  `FighterSample::unsupported().with_kinematics(..).with_situation(..)`, `Phase::Before`,
  `InputEvent{actor:Some, entry:Some, controller:None, origin:InputOrigin::Effective, raw:None}`,
  `Stick::F32`, `Buttons`, `Completion`, `Manifest`, `Completeness`.
- Write path: `validate::StreamChecker`, `archive::ArchiveWriter::{create,write_event,flush,events_written,last_seq,manifest}`,
  `archive::write_manifest` (only onto an absent `manifest.json`).
- Read path: `archive::read_manifest`, `archive::open_timeline`, `TimelineReader::{next_event,completeness}`.
- Host tests only: `Recorder::create/in_memory`, `Capture::load`, `validate_capture`, `training_macro`.

Avoid on device: `Recorder::finish`/`abort` (blocker B1), `ArchiveWriter::finish`, `Capture::save`, `overwrite:true`,
`Recorder::in_memory`, `keep_events:true`, `Capture::load` on full-length captures, and `TrainingQuantizeOptions`.

Header facts: do not fabricate `ReplayAssociation`, because its `sha256` field is required and must hash the whole replay
file, which is unproven here. Use `capture_id` = the existing hex replay id, which decodes back exactly. Leave `rules.actual`,
`stage`, controllers and `ControlKind` unknown. Declare animation, vitals, objects and random as `Unsupported`.
Never emit `unchanged`: the producer always observes, and no death/respawn boundary exists to make carry safe.

## 4. Clocks, phase and the native counter (unproven → leave unavailable)

- `ClockKind` is closed: tick, input_seq, record, video_frame, host_ns (`model.rs:266-274`). The counter at `state+0x200078`
  (`live_replay.rs:65`) is unproven as tick, input_seq or FRAM ordinal.
  - Emit `Clocks::NONE` and `clocks.supplied = []`.
  - With no tick, the `PhaseOrder` check does not run (`validate.rs:522-532`), and Tick/Record selection reports `ClockUnavailable`.
  - Seq selection still works.
- Preserving the raw counter is optional and cheap. Keep `first_tick` in the snapshot (it is now dropped with `LiveRecord`,
  `live_capture.rs:22-27,150`). Under the pool's contiguity invariant, group `i` ↔ counter `first_tick + i`.
- Write it either as one `EventBody::Raw(RawUnit{raw: RawBytes::new("<named u32le encoding>", &v.to_le_bytes()), source_offset: Some(0x200078)})`
  at the start plus a manifest note, or as one Raw marker per tick group (explicit grouping for AI/Export; about 110 B per tick).
  Promote it to `Clocks.tick`/`record` only after proof.
- Phase/origin: `before_frame` state → `State{phase: Before}`; post-frame `ControlModule` reads → `Input{origin: Effective}`.
  The model has no "unqualified" variant, so mark `inputs`/`kinematics` as `Supply::partial("...pending runtime qualification")`
  plus `capture.notes`. Labelling is the parent's decision.

## 5. Buffering and algorithm constraints

- Keep the full bounded in-memory capture until native save success; the existing save association needs it. Canonical
  JSONL is written only afterwards, on the existing `replay-capture-save` thread. Native callbacks stay O(1) with no I/O:
  three extra getters, and allocation stays the single `try_reserve_exact` per track.
- Frame size grows from 44 B to about 68 B: u32 buttons + 4×f32 sticks + `StartState` (28) + pos xyz (12) + speed_z (4)
  + object id (4). That is about 2.45 MB reserved per track. Worst case for 4 retained 8-track records rises from
  about 48 MiB to about 75 MiB. Heap headroom is unproven.
- The writer streams one `Event` at a time, so extra memory is O(1). Order: for tick `i`, for each track by entry →
  lifecycle (if identity changed) → `State(Before)` → `Input(Effective)`. For a track sealed early, an optional
  `Gap{actor}`; a session-level `truncated` → `Completion::truncated`.
- Size estimate (from hand-counted line lengths, not measured): about 290 B per state line and 230 B per input line, so
  about 520 B per actor-tick vs 44 B in `.tmrc`.
  - 7-minute, 2-fighter match: about 26 MB.
  - Worst case (8 × 36,000): about 150 MB.
  - The adapter's `BufWriter` is the shared 8 KiB one, and each chunk is flushed by Switch std.
  - The save queue is `sync_channel(4)` + `try_send` (`replay_store.rs:37,64`); a long write can drop later saves (existing behaviour, made worse).

## 6. Minimal tested seams (host, in `training_mod_macros`)

- T1 Bit-exactness: synthetic float session including −0.0, `0.49999997/120`, ±(k+0.5)/120 neighbours, values >1.0
  (no_clamp) and subnormals → adapter publish → `Capture::load`. Then `validate_capture` must report no Error and
  completeness `Complete`/`DeclaredTruncated`, and every f32 must round-trip `to_bits` equal.
- T2 Projection parity: projector(tee at save) == projector(streamed reopen) == the legacy `axis()` path. Compare i8 axes,
  `quantized_analog`, `StartState`, and x/y bitwise.
- T3 Publication: on host, same event list through `Recorder::create`+`finish` vs the adapter gives equal manifests
  (`read_manifest`) and identical timeline bytes. The adapter performs only renames onto absent paths. An existing final
  dir → `already_present` with no change. Listing ignores dot-staging dirs.
- T4 Lifecycle: kind change with the same object → `form_change`; object change → despawn+spawn transform. The projector
  restores per-frame `fighter_kind`, and `ReplayTrack::export` still rejects windows that cross a transformation.
- T5 Truncation/sealing: truncated session → `DeclaredTruncated`. The projector sets `truncated`; host `training_macro`
  refuses unless `allow_declared_truncation`.
- T6 Bounds: an oversized manifest or timeline is rejected before any read.

## 7. Runtime concerns (unproven; qualify on Eden/device before relying on them)

R1 Before-state is pre-input, and post-frame `ControlModule` equals the applied input, for human and CPU.
R2 Meaning of `state+0x200078`.
R3 `get_sum_speed3f` struct-return ABI; its x/y must be bit-equal to `get_sum_speed_x/y`. Validity of `pos_z`.
R4 `battle_object_id` behaviour across Trainer/Aegis-style transformations.
R5 Heap headroom at about 68 B/frame.
R6 SD throughput and queue drops at about 12–15× file size.
R7 `RenameDirectory` same-parent on HOS SD and on Eden (host `RenameDir`, no existence check).
R8 A skyline-target build of shared deps (`sha2`/`cpufeatures`).

## 8. Training file map (no edits made)

`training_mod_macros/Cargo.toml` (dependency); `replay.rs` (float frame/track/session + projector + `axis`);
`live_capture.rs` (float pool, `valid()`, keep `first_tick`); new adapter module (StreamChecker+ArchiveWriter publish,
streamed reopen); `capture_archive.rs` (unchanged `.tmrc` cache); `src/replay_capture.rs` (`observed_input` returns
raw floats, `state()` adds `pos_z`/speed z/object id; the Vault path keeps i8 through the projector's `axis`);
`src/live_replay.rs` (push floats, carry object id); `src/replay_store.rs` (writer publishes canonical, then the cache).

## Raw errors during this audit

1. `rg: The term 'rg' is not recognized as a name of a cmdlet, function, script file, or executable program.`
   `Check the spelling of the name, or if a path was included, verify that the path is correct and try again.`
   (rg is not on PATH for pwsh; `/snap/bin/rg` was used afterwards.)
2. `regex parse error:` / `    ^name = \` / `            ^` / `error: incomplete escape sequence, reached end of pattern prematurely`
   (quoting inside `-Command`; rewritten with `.` wildcards.)
No other tool errors occurred. No build, test or executable was run.

## Watcher closure

Watcher PID 3314548 (owner 3311952) polled transcript model/effort and owner liveness every 30 s. Its last heartbeat
was `ok model=claude-opus-5-5 effort=xhigh at 2026-10-11T22:58:46-04:00`. No WARN file and an empty stderr log.
It was stopped and reaped: the process is gone, `/proc/3314548` is absent, and `/tmp/cc-watch-0497e6b0` was removed.
