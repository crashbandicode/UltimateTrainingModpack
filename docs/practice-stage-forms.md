# Practice stage forms

**Status:** implemented and checked on an emulator in ordinary Training and Local
Wireless waiting. Quickplay waiting uses the same waiting path and counts as
shared-code coverage. Ordinary Training rotation sits behind an explicit toggle
(see [Training entry rotation](#training-entry-rotation)). Physical-console
acceptance is pending. User controls are in [Practice Stages](practice-stages.md).

## Selection and persistence contract

- A selection consists of the existing base-stage ID and a form: `normal`,
  `omega`, or `battlefield`. `final_destination` deserializes as `omega` and is
  always saved as `omega`.
- Catalog order and the existing rank arrays stay unchanged. The 117-cell
  `rotation_forms` metadata array uses the existing stage indices. The Direct
  preference has a separate `direct_form`.
- Missing legacy form metadata means Normal. A present unknown form, wrong
  array length, unsupported stage/form pair or non-catalog base ID is an error.
  Validate every form cell, including stages not currently ranked.
- Ordinary Training manual changes remain session-local. A waiting-room Direct
  selection saves its stage/form for later entries. A form edit does not reorder
  stages, submit a request by itself, or advance a rotation cursor.
- On a highlighted named stage, **Y Form** cycles the supported Normal/Omega/
  Battlefield forms as three separate choices. The help line names the current
  form (`Normal`, `Omega` or `Battlefield`), then the rest of that row's Y order,
  for example `Omega | Y Form: Battlefield, Normal`. A form the row lacks is
  skipped. An emulator check (C77, Training) showed this wording for all three
  Peach's Castle forms; two-form and Normal-only rows and the console are
  unchecked. L/R paging
  preserves the draft and ranks; All has no form and cannot submit a Direct action.
  Cancelling a Direct draft leaves the saved direct stage/form unchanged.

## Native mapping

The recorded 13.0.5 stage table has 364 rows at `0x454a7e8`, stride 72.
`training_mod_consts::lobby_stages::native_stage` resolves an explicit mapping
from base ID plus table form to native ID. Resource lookup uses that resolved
ID's `+0x18` package hash; it does not use universal `base + form` arithmetic.

| Base identity | Normal | Omega | Battlefield |
| --- | ---: | ---: | ---: |
| Battlefield | 0 | 1 | unavailable |
| Final Destination | 3 | unavailable | 4 |
| Big Battlefield | 2 | unavailable | unavailable |
| Small Battlefield | 347 | unavailable | unavailable |
| Training | 308 | unavailable | unavailable |
| Waiting Room | 311 | unavailable | unavailable |
| Smashville | 95 | 96 | 97 |

Battlefield Normal/Omega share a package hash, as do Smashville Omega/Battlefield.
The native **kind and hash together** identify a request; equal package hashes
do not mean equal forms. TestStage 327 is unresolved in the recorded asset join
and is not a selectable base identity.

## Request and entry invariants

The request queue retains the full selection. Validation occurs before changing
queue state, and native preparation resolves the selection again before any
publication. Existing scene/epoch, active allocation, native readiness and match
cache checks remain in force. The descriptor change is confined to stage kind
and package hash; music, hazards and other bytes are copied unchanged.

`StageCycle<Selection>` reserves the complete selection through repeated
preloads. The cursor remembers the base identity, so editing a form does not
make the last stage look removed. Entry consumes one reservation; fixed Direct
entries do not advance the rotation cursor. Each native practice family needs
its own cycle. These are pure policy properties; native wiring evidence is the
emulator acceptance below.

The waiting adapter uses the reserved selection for **both** preload and scene
setter, including saved forms in non-alpha rotation. Only immediate/fixed Direct
admission depends on the existing alpha feature. New invalid preferences fail
before reserving; later metadata edits do not replace an already owned preload.
Training's entry event, cancellation rules and activation are described in the
next section; a shared loader call or per-frame live update alone is still not
treated as a new Training entry.

## Training entry rotation

Builds with the `training-stage-rotation-alpha` feature (part of
`enhancements-alpha`) add **Training Stage Rotation** under Misc Settings next
to Lobby Stages. It is Off by default and absent from builds that lack the
feature, so a legacy or ordinary file never activates it. While On:

- Each genuine Training entry from character select prepares the next
  selection of the Lobby Stages order with the saved rotation forms, using an
  independent cycle (the waiting-room cursors are untouched). A user-chosen
  Waiting Room rank is entered as chosen; its runtime behavior in Training is
  unverified.
- The prepared selection is applied once at the Training loader's call, writing
  only the resolved stage id and package hash on the descriptor the loader
  reads, so resource and scene use the same pair. Later loader calls in the same
  entry reapply the same pair and never advance.
- The Mii editor return is suppressed by the product's own successful
  re-entry request, one-shot, and reapplies the last confirmed stage: a
  completed direct change replaces that stage; a direct change still in flight
   when the scene departed leaves it unknown. In that case the adapter writes
   nothing, and the loader uses whatever the descriptor holds.
- A new Training entry cancels any unapplied reservation of an abandoned
  earlier entry and resolves the current preferences. Constructing a new
  Training sequence resets every owner-bound value, even at a reused address;
  the rotation cursor itself continues across Training visits.
- Turning the toggle Off at any decision point writes nothing and clears the
  reservation and the reapply snapshot; the cursor is kept.
- Actual matches, waiting rooms, live direct changes, resets and fighter
  reloads are excluded by mode, melee, caller, slot and descriptor checks and by
  the state transition they do not pass.

Emulator checks: three On entries each prepared and applied once, in the order
Battlefield Ω (1) → Final Destination Battlefield (4) → wrap to Battlefield Ω.
An Off entry between them restored ordinary Training (308) and its native
resource. A rotated entry whose match-preload slot already held Training (308)
completed with the requested form and cleared that slot. A Waiting Room rank in
Training, allocator/performance behavior and the physical console are not
established.

## Regression and acceptance boundaries

New regression sources cover:

- Explicit mapping exceptions, internal-ID rejection and all stored form cells.
- Legacy four-/117-element rank migration, current ranks and form round trips.
- Real `MenuJsonStruct` ingress, UI form edits and one-shot close intent.
- Form reservations, reordering, cancellation and independent cursors.
- Same-package/different-kind native transitions, exact completion, stale menu
  ownership and pending-match cache rejection in Training/LDN/Quickplay policy.
- Saving Omega and Battlefield forms after actual UI edits, exiting that child
  process, then restoring and continuing both edits in a second process.
- Training rotation policy counterexamples: one entry consumes one step across
  repeated loader calls, the suppressed return reapplies without advancing,
  direct completion/uncertainty, abandoned reservations at a reused context,
  Off/On around a return, invalid forms, lobby Direct mode ignored, and the
  exact loader predicate. Plus a separate fresh-process test that turns the
  toggle On and edits a form through the real menu, exits, then reloads the
  file through the production loader and continues editing.

`all_forms_match_recorded_native_catalog` is explicitly ignored by default:
the parent supplies the private recorded JSON with `STAGE185_NATIVE_CATALOG` and
runs that exact test. It compares every supported base/form against the saved
table, rather than regenerating expected values from the implementation.

Emulator acceptance showed the selected geometry after direct changes (Training
Battlefield Normal 0; waiting 1, 3, 4 and WarioWare 104), after repeated rotated
entries, and after returning from a real Local Wireless match. That match used
its own normal Battlefield (0) rather than the waiting Ω form, and ordinary
Training entry with the toggle Off was unchanged. A separate hazards check
showed that a practice Off setting does not leak into a hazards-On match.
