# smash-match-data format 1.0

Open, versioned recording format shared by Training, Export and AI consumers.
One capture is a directory:

```text
<capture>/manifest.json    # provenance, rules, roster, declared channels, completion
<capture>/timeline.jsonl   # one JSON event per line, strictly increasing `seq`
```

The Rust types in `src/model.rs` are the normative definitions; this document
explains them. Package `smash-match-data`, library `smash_match_data`, binary
`match-data`. Feature `replay` (default) pulls in `smash-replay-core`; the
library, adapters and CLI must build with `--no-default-features`.

## Principles

* **Unknown is never zero.** Every sampled group carries a presence marker
  (`Channel`); every unknown manifest field is `null`/absent, never a default.
* **Clocks stay independent** until a relationship is declared. A saved-replay
  record is not a gameplay tick or a video frame.
* **Raw before derived.** Native `f32` values, raw bytes and native integers are
  stored as the producer saw them. Conversions (e.g. stick float to signed byte)
  belong to consumer adapters.
* **Actors are stable.** `ActorId` is unique per capture and never reused; it is
  distinct from controller slot, fighter entry and the game's reusable object id.
* **No implicit carry.** `unchanged` resolves only to a valid predecessor in the
  same actor lifetime segment with no gap or missing sample in between. For an
  input, that predecessor must also share origin, layout, controller and entry.
  Spawn, death, respawn, despawn and form change each start a new segment, so
  a sample from an earlier life is not a predecessor.
* **Truncation is reported, never accepted silently.** Readers compute their own
  `Completeness` from the file; the manifest's claim is cross-checked.

## Float exactness

JSON is written with `serde_json` and read with its `float_roundtrip` feature.
`f32` fields parse at single precision and round-trip bit-exactly, signed zero
included. Two rules in the model keep that true: no `#[serde(flatten)]` and no
internally/adjacently tagged enums anywhere a float can appear (serde would
buffer through `f64`). Non-finite floats are rejected by validation.

## Presence markers (`Channel<T>`)

| JSON value | Meaning |
| --- | --- |
| `{...}` (object) | Observed value. Zero/neutral is an observation. |
| `"unchanged"` | Same as the nearest valid predecessor in this lifetime segment. |
| `"missing"` | Producer supports the channel but has no sample here. |
| `"unsupported"` | Producer never supplies the channel. |
| `null` | **Invalid**, rejected on read. |

The manifest `capture.channels` block declares each channel's `level`
(`full`, `partial`, `unsupported`) with an optional `note`. An observed value on
a channel declared `unsupported` is a validation error.

## Clocks

Every event carries `clocks`, each field optional:

| Field | Meaning |
| --- | --- |
| `tick` | Gameplay tick (engine fixed-step scene counter during play). |
| `input_seq` | Input-consumption sequence. |
| `record` | Saved-replay record ordinal from `smash_replay_core::records`. |
| `video_frame` | Rendered frame counter. |
| `host_ns` | Host monotonic nanoseconds. |

`capture.clocks.supplied` lists the clocks the producer sets;
`capture.clocks.relations` lists established relationships (`identity`,
`offset {b_minus_a}`, `unestablished`). Absent relation means not established.
Within one capture each clock is non-decreasing across events.

## Timeline events

Each line is `{"seq": N, "clocks": {...}, "body": {"<kind>": ...}}`. `seq`
starts at 0 and is consecutive when written by this crate; readers require it to
be strictly increasing. Kinds:

| Kind | Body | Purpose |
| --- | --- | --- |
| `boundary` | `"recording_start" \| "first_playable" \| "match_end" \| "recording_end"` | Recording/match boundaries, each at most once, in that order. |
| `pace` | `{paused, speed?, speed_index?, reason?}` | Global pause / speed change effective from here on. |
| `raw` | `{raw: {encoding, hex}, source_offset?}` | Original source unit, such as a whole replay record, with its original byte offset. |
| `lifecycle` | `{actor, event}` | Actor lifetime boundaries (below). |
| `state` | `{actor, phase, sample}` | Fighter sample; `phase` is `before` or `after` this tick's input. |
| `input` | `InputEvent` | One input reading (below). |
| `object` | `{object, event}` | Scene object spawn / state / despawn. |
| `random` | `{actor?, basis, outcome, note?}` | Random outcome, `basis` is `observed`, `requested` or `inferred`. |
| `gap` | `{actor?, reason?}` | Samples between the previous event (for the actor, or globally) and here were lost. |
| `note` | `{severity, text}` | Producer diagnostics. |
| `end` | `{events}` | Terminator; `events` counts the lines before it. |

Sampling order per tick is `state(before)`, `input`, `state(after)`, one group
per actor, in that sequence. Producers that only observe one phase emit that
phase. A `before` sample that follows the same actor's `after` sample at the
same tick is a validation error (`phase_order`). Hitstop is per actor (`situation.hitstop_frames`); pause and slow motion
are global (`pace`); respawn is a lifecycle event.

### Lifecycle

```json
{"lifecycle":{"actor":1,"event":{"spawn":{"identity":{...},"cause":"match_start"}}}}
{"lifecycle":{"actor":1,"event":{"death":{"cause":"blast_zone_left"}}}}
{"lifecycle":{"actor":1,"event":"respawn"}}
{"lifecycle":{"actor":1,"event":{"despawn":{"cause":"match_end"}}}}
{"lifecycle":{"actor":1,"event":{"form_change":{"identity":{...}}}}}
```

Rules enforced by the recorder and validator: `spawn` first and once per id;
`death` only while alive; `respawn` only while dead; nothing after `despawn`.
A transformation that creates a new native object is a `despawn {transform
{to}}` plus a `spawn {transform {from}}` on a new actor; a same-object change
is `form_change`. Every lifecycle event starts a new **lifetime segment**;
`unchanged` never resolves across one.

`ActorIdentity`: `fighter_kind`, `fighter_name`, `form`, `costume`, `entry`,
`controller`, `control` (`"human"`, `{"cpu":{"level"}}`, `"unknown"`),
`native_object_id`, `team`. All optional except `control`.

### Fighter sample

```json
{"kinematics":{"position":{"x":-20.0,"y":0.0,"z":0.0},"facing":1.0,"velocity":{"x":0.0,"y":-0.0,"z":0.0}},
 "situation":{"situation_kind":0,"status_kind":0,"grounded":true,"hitstop_frames":0.0},
 "animation":{"motion":{"hash40":123,"label":"wait1"},"frame":12.5,"rate":1.0,
              "root_rotation":{"euler":{"x":0.0,"y":0.0,"z":0.0}},"root_scale":{"x":1.0,"y":1.0,"z":1.0},
              "secondary":{"motion":{"hash40":456},"frame":3.0,"weight":0.25}},
 "vitals":{"damage":12.3,"stocks":3,"shield":50.0}}
```

Each of the four groups is a `Channel`. Animation `frame` is a playback
position (possibly fractional) for the engine's own assets; no images.
`root_rotation` is `{"euler":{x,y,z}}` or `{"quaternion":{x,y,z,w}}`.

### Input

```json
{"input":{"controller":0,"entry":0,"actor":1,"origin":"physical","layout":"pro_v1",
 "buttons":{"bits":5},"left_stick":{"f32":{"x":0.70710677,"y":-0.0}},"right_stick":{"f32":{"x":0.0,"y":0.0}},
 "raw":{"encoding":"hid_report_v1","hex":"0500..."}}}
```

* `origin`: `physical` (controller), `replay_decoded` (mapped replay input),
  `effective` (what the game applied), `cpu_generated`.
* Sticks are `{"f32":{x,y}}` or `{"i8":{x,y}}`, whichever the producer natively
  has. Decoded replay packets are `i8`; live producers store the native floats.
* `buttons`, `left_stick`, `right_stick` are `Channel`s.
* `layout` names an `InputLayout` in `capture.input_layouts`: button bit names
  (`width_bits`, `names` map bit→name), per-stick `AxisLayout` (`encoding`,
  `min`, `max`, `neutral`, `float_scale`, `note`; unknown scaling stays absent),
  and the raw `encoding` name.
* `raw` keeps the exact bytes (lower-case hex) before any decoding.
* Validation rejects an observed stick whose encoding differs from the layout's
  axis encoding, and a `raw.encoding` that differs from the layout's
  `raw_encoding` when one is declared (`layout_mismatch`).

### Scene objects

`object.event` is `{"spawn":{identity, transform, animation}}`,
`{"state":{transform, animation}}` or `{"despawn":{cause?}}`. `identity` has
`class` (`article`, `projectile`, `item`, `other`), `native_kind`, `name`,
`owner` (actor id), `native_object_id`. `transform` is a `Channel` of
`{position, rotation?, scale?, velocity?}`; `animation` a `Channel` of the same
animation structure used for fighters.

### Random outcomes

`outcome` is one of `{"judge":{number}}`, `{"item_pull":{item_kind?, name?,
object?}}`, `{"hero_menu":{options:[{id?,name?}]}}`,
`{"rng_observation":{stream?, seed?, counter?, words?, value?}}` or
`{"other":{domain, value}}`. `basis` keeps observed, requested and inferred
outcomes apart. Identical inputs do not guarantee identical outcomes.

## Manifest

```json
{
  "format": "smash-match-data",
  "format_version": {"major": 1, "minor": 0},
  "capture": {
    "capture_id": "…", "created_utc": "2026-10-11T19:30:00Z",
    "recorder": {"name": "…", "version": "…"},
    "game": {"title": "Super Smash Bros. Ultimate", "version": "13.0.5", "build_id": null, "region": null, "platform": null},
    "source": {"kind": "live_match" | "replay_regeneration" | "file_decode",
               "replay": {"sha256": "…", "size_bytes": 0, "path_hint": "…", "header_version_word": 53253, "stored_seed_word": 0},
               "decoder": {"name": "smash-replay-core", "version": "0.1.0"}},
    "rules": {"target": "competitive", "actual": {"mode": "stock", "stocks": 3, "time_limit_seconds": 420,
              "items": "off", "stage_hazards": false, "final_smash_meter": false, "spirits": false,
              "damage_ratio": 1.0, "launch_rate": 1.0, "team_battle": false, "stage_morph": false,
              "underdog_boost": false, "raw_setup": {"encoding": "cfmk_setup_block", "hex": "…"}}},
    "stage": {"stage_id": 0, "name": "…", "form": "normal" | "battlefield" | "omega" | {"other": "…"}},
    "controllers": [{"slot": 0, "kind": "pro_controller", "human": true, "label": "P1"}],
    "entries": [{"entry": 0, "control": "human", "controller": 0, "fighter_kind": 8, "fighter_kind_group": [],
                 "fighter_name": "…", "costume": 0, "team": null, "present": true}],
    "input_layouts": [ … ],
    "channels": {"kinematics": {"level": "full"}, "situation": {"level": "full"}, "animation": {"level": "partial", "note": "no secondary track"},
                 "vitals": {"level": "full"}, "inputs": {"level": "full"}, "lifecycle": {"level": "full"},
                 "objects": {"level": "unsupported"}, "random": {"level": "unsupported"}},
    "clocks": {"supplied": ["tick", "input_seq", "host_ns"], "relations": []},
    "notes": ["…"]
  },
  "timeline_file": "timeline.jsonl",
  "completion": {"state": "open" | "complete" | "truncated", "reason": "…", "events": 1234, "last_seq": 1233},
  "boundaries": [{"boundary": "first_playable", "seq": 12, "clocks": {"tick": 0}}],
  "actors": [{"actor": 1, "identity": {...}, "spawn_seq": 1, "despawn_seq": 900, "deaths": 2, "respawns": 2, "form_changes": 0}]
}
```

`capture` is what the producer declares; `completion`, `boundaries` and
`actors` are derived by the writer at finish. `rules.target` is the intended
ruleset; `rules.actual` is what was actually set, every field optional.
`capture.source.digests` optionally records `{scope, sha256, size_bytes}` for
extracted source buffers. Their scope is explicit: a FRAM payload hash is not
the whole saved-replay hash held in `source.replay.sha256`.
`validate::competitive_conformance` reports conforms / deviates / undetermined
without guessing unknown fields.

## Completion and truncation

Writing: the manifest is written first with `completion.state = "open"`, events
are appended (flushed per event by default), and `finish` appends the `end`
event, syncs, then atomically rewrites the manifest as `complete` or
`truncated` (producer stopped early on purpose, with a reason).

Reading produces a `Completeness` from the file itself:

| Completeness | When |
| --- | --- |
| `complete` | Manifest `complete`, `end` present, counts agree. |
| `declared_truncated {reason}` | Manifest `truncated`, `end` present, counts agree. |
| `incomplete {problems}` | Anything else: `manifest_open`, `missing_end`, `partial_trailing_line`, `missing_trailing_newline`, `end_count_mismatch`, `manifest_count_mismatch`, `manifest_last_seq_mismatch`. |

A partial trailing line is reported, not treated as corruption; a malformed
line anywhere else, a `seq` that does not increase, or an event after `end` is
an error. Re-saving an `incomplete` capture produces a `declared_truncated` one
whose reason records the original problems.

## Selected frames and clip start

`clip::clip_context` resolves the state of every actor and live object at an
unscoped selection point: `at_seq(n)` (events with `seq <= n`), `at_tick(t)`
(everything up to, but not including, the first `input` from any fighter at
tick `t`, so the start state is that tick's `before` sample when every
fighter's before-state is already recorded), or `at_record(r)` (same rule
using the record clock). Actor-selected Training, playback and resolved-AI
clips use `clip_context_for` / `selection_end_for` instead. Those functions
cut a tick or record on the selected actor's own input, so that actor's
current before-state stays in the context even when another fighter already
has an input on the same clock. An input with no actor id does not define an
actor's cut, and controller numbers are not mapped to actors. `Seq` is the
same either way. The end cut uses the same rule, so the body stops before the
selected actor's first input of the end tick or record.
For each channel it walks back within the actor's current lifetime segment:

* latest sample observed → `observed`;
* latest `unchanged` → keep walking; an observed predecessor is returned with
  `carried_from`; a `missing` sample, a `gap` or the segment start ends the
  search as `missing` with the reason;
* a `gap` between the latest sample and the selection point → `missing`;
* no sample in the segment → `no_sample`; `unsupported` stays `unsupported`.

Actors are reported as not spawned, alive, dead (after `death`, before
`respawn`) or despawned. This is state for selection and preview, not a full
restorable game checkpoint.

## What each source can supply

| Channel | Live hook | Replay regeneration | Saved-file decode (`smash-replay-core`) |
| --- | --- | --- | --- |
| Clocks | tick, input_seq, host_ns, video_frame as hooked | same | `record` only; no tick/video mapping is established |
| Inputs | native floats + raw bytes | as hooked | `i8` sticks + 32-bit buttons per present packet, raw 8-byte packet, header bits 7/15 |
| Fighter state, animation, vitals | as hooked | as hooked | **unsupported** |
| Lifecycle | as hooked | as hooked | unsupported (no actor observations) |
| Roster | full | full | fighter-kind groups per saved entry only; no costume, controller, rules, presence |
| Random | as hooked | as hooked | unsupported; a stored seed stays setup metadata, never an inferred RNG observation |
| Objects | as hooked | as hooked | unsupported |

Saved-payload decoding therefore supplies far fewer channels than a live
producer and must declare them `unsupported`. Nothing about engine hook wiring
can be inferred from this module; producers own that.

## Versioning

`format` must equal `smash-match-data`. A reader accepts files whose `major`
equals its own and whose `minor` is not newer. Minor versions only add optional
fields; unknown fields are ignored on read.
