# Named CPU macros

This local implementation has passed scoped native acceptance testing.

## Implemented scope and current verification

Implemented: named CPU recording/library, native keyboard naming, latest/previous
replay capture, native-input crop preview with independent In/Out handles,
relative placement, recorded/at-rest momentum, facing mirroring and guarded
restore-before-input-zero playback. Optional live-match capture also persists a
companion when its associated native replay is successfully saved. Revision27
verifies that saved capture across a process restart, with capture disabled, through
an eight-input Pikachu preview and named export. Revision21 verifies the native-preview backend;
revision20's earlier acceptance applies to the pose-preview backend it replaced.
The sections below give the current controls.

Historical checks of the earlier pose-preview backend covered loading input
isolation, B cancellation, grounded CPU acquisition, HUD/statistics hiding,
selection restart, anchor restoration and closed-editor Training leave/reentry.
Earlier native runs also cover manual recording, naming cancellation/validation/
retry, library operations and the scoped placement/momentum cases described below.

Revision21 passed a full 1,862-input native preview with a visible Dr. Mario capsule,
attacks/effects, restart, explicit stop/restore, two eight-input selection runs,
held-B close/restore and native-menu exit. Deliberately enabled legacy loop and
record/playback-on-load options did not interfere. Strict host/native checks pass.
See the parent's `research/named-macros/revision21-findings.md` for exact evidence.
Broader airborne/roster/controller behavior, forced timeout recovery and physical
Switch remain unverified. Getter observations cover every emitted input, but repeated
callbacks prevent a universal one-callback-per-gameplay-tick claim.

## Record and load

1. In **Input Recording → Macro Source**, select **Named Library**. The default
   remains numbered slots so existing settings keep their previous behavior.
2. Use the configured **Input Record** button combination to possess the CPU and
   record. Press the same combination again to finish. Named recordings use a
   ten-minute safety limit rather than the numbered-slot duration setting.
3. The **Save Recording** panel shows the frame count and duration. **A: Name and
   save** opens the system keyboard. Confirm a name to save; cancelling or entering
   an invalid name keeps the draft so you can retry. **Y** opens placement controls.
   **Recording Crop** optionally trims trailing neutral input.
4. **Named Library** lists saved files. Selecting a recording requests its CPU
   fighter; close the menu and allow the character change to finish.
5. Enable save states and save the desired Training setup. The configured
   **Input Playback** combination restores that setup before starting the loaded
   macro. Numbered recordings remain available by switching Macro Source back.

Files live in `sd:/ultimate/TrainingModpack/macros/`. They are versioned JSON with
mapped buttons/sticks, a start-state summary and placement metadata. The library
has no five-slot restriction. Selecting a recording persists its ID across game
restarts. A macro is an input sequence, not a complete simulation save state.

## Placement

After loading a macro and restoring a Training state, open **Edit Placement**.
The anchor is the CPU's restored position. **X-/X+/Y-/Y+** move the target one
unit; lowercase controls move it a quarter unit. The crosshair previews the
target without moving the real CPU while editing.

- **Keep** applies an in-memory edit without changing the file.
- **Write** explicitly overwrites the loaded file's placement metadata.
- **New** opens naming and writes a separate recording.
- **Back** cancels the edit.
- **Momentum** switches between **Keep recorded speed** and **Start at rest**.

Placement does not rewrite recorded inputs. The XY offset is in world units from
the restored reference, including when the save-state anchor is mirrored. The
recorded absolute position is not restored. Repeated restores use that reference
instead of adding the offset to the CPU's current position.

Recorded momentum is applied immediately before named playback input zero, after
both fighters finish restoring. Horizontal speed follows input-facing mirroring;
an airborne start requests normal falling physics, including at zero Y offset.
**Start at rest** clears
starting velocity, then ordinary physics resumes; it does not hold the CPU in the
air. A plain load or recording start does not apply the selected macro's momentum.
Native Dr. Mario checks cover rising/falling momentum, starting at rest, mirrored
controls and speed, and repeated noncompounding restores. See the parent test
record for the exact cases and remaining acceptance limits.

Use an idle/fall setup for named restore-playback. An unshifted ledge save is
rejected rather than silently releasing the ledge. An aerial target on the floor
can land before playback is ready, including with recorded upward momentum. The
macro then stops without consuming input 0 and asks you to adjust the placement.
Raise the placement or save an airborne setup to practice the aerial.

## Export from a saved replay

### Optional live-match capture across restarts

1. Start the mod once to create
   `sd:/ultimate/TrainingModpack/replay_capture.ini`. Capture defaults **off**.
   To enable it, edit the file while the game is closed, then restart:

   ```ini
   [capture]
   enabled=on
   ```

2. Play a match and use the game's **Save Replay** action on the results screen.
   The companion is published only after the matching native replay save succeeds.
   Cancelling the save creates no companion. An unsaved RAM capture is not a
   persistent replay choice.
3. Enter Training, including after a complete game restart. Open **Input Recording
   → Replay Captures**, choose the native replay name and press **A** to load it.
   These saved choices remain accessible after setting `enabled=off` and restarting.
4. Choose the fighter, save a matching Training setup, crop, preview and name/save
   using the controls below. Selecting a different fighter requests the CPU reload;
   save the matching setup after that change finishes.

Companions live in `sd:/ultimate/TrainingModpack/replay_captures/`. Each shared
`.smd` bundle contains `manifest.json`, `timeline.jsonl` and a Training `.tmrc` cache.
The shared timeline retains native floating-point observations; rounding to the
Training input format happens only in the cache/export projection. Keep the whole
bundle together. A capture is bounded to36,000samples per track, up to8tracks; partial
captures remain explicitly marked rather than being presented as a complete match.

Normal revision27 also passed a scoped emulator coexistence check with Deluxe
`6c396f10` in active manual LessLag mode: native match/save, cold Training access
with capture off, CPU reload, eight-input preview, and exact named export.

Observed native-buffer completion and exact export do not establish first/last
simulation-tick coverage or every input/state sampling phase. Controlled performance
and deliberate frame-drop checks are still pending; capture remains opt-in.
Physical Switch and online behavior are unverified.

### Vault fallback

For older/imported replays, matches recorded with capture off, or a replay without
a companion, use the existing Vault route. It also observes CPU behavior while the
native replay simulation runs:

1. Play the replay through **Vault → Replays → Replay Data → Play**. You only need
   to play through the end of the section you want, not the whole match. Include
   enough lead-in for a supported idle/fall start.
2. End playback using the game's displayed quit combination, then enter Training
   **without restarting the game**. The latest two observed timelines are held in memory.
3. Open **Input Recording → Replay Window** while the CPU is idle or falling.
   **Up/Down** selects the previous/next fighter; changing kind reloads the CPU and
   retains the crop. During loading, P1 input is isolated and the CPU must settle
   to grounded idle. **B** closes the editor while an already-started install finishes.
   Before previewing, enable save states and save a setup with the current pair of
   fighters. If selecting a different CPU, close the editor, save the new setup,
   and reopen it. Preview uses the explicitly selected saved slot, its original
   facing and saved damage, independently of random-slot/mirroring/damage options.
4. **L** selects In and **R** selects Out. **Left/Right** edits that boundary by one
   frame; holding accelerates. Hold **ZL or ZR** for 60-frame edits. Handles cannot
   cross: `[In, Out)` includes In, excludes Out, and contains at least one input.
   Editing a boundary stops preview and restores the saved setup; it does not seek
   the game to an arbitrary recorded pose.
5. **X** plays the selected inputs from the saved setup, or stops and restores it.
   **Y** restarts from that setup. The playhead reports emitted inputs, independently
   of rendering. At the end, input injection stops while native attacks/effects
   continue normally. Restart, edit or close to restore the setup. **ZL+X** switches
   latest/previous capture if Vault looped. On a GameCube controller, use Z as the
   modifier instead of ZL/ZR. Preview is transient and does not replace the loaded
   library entry or change loop/record/playback-on-load settings.
6. Choose an idle/fall start, or explicitly press **ZL+Y** to include the previous
   clean-start lead-in. An unsupported start is rejected without silently moving
   In. **A** opens the system keyboard to name/save; cancellation retains the crop.

Preview runs inputs through the native CPU simulation so attacks, articles and
effects happen normally, with both fighters free to react. P1's controls remain
owned by the editor. Stop/restart/close use the existing native death/rebirth
save-state cleanup; they restore the saved Training setup, not the world present
when the editor opened. This is one fighter's input sequence, not exact whole-match
reconstruction. Pause, arbitrary backward seeking and frame-step are not offered.
Use a suitable airborne saved setup for aerial starts. Starting in an arbitrary
animation is not supported. AI analog values can lie
between controller steps; the editor reports when these were rounded. Replay
capture observes the game's native simulation rather than treating the replay
file's `FRAM` records as gameplay frames. A 120-frame writer/native-getter comparison
passed exactly. All 1,862 inputs of the longer fixture were written once in order
and matched native getter values, with three extra observations of one sequence;
strict callback-to-gameplay-tick timing and general C-stick/remapping coverage
remain unverified. Do not write the upstream `clamped_rstick` fields: native review
identified those bytes as rumble state.

The native non-ready loading timeout preserved the selection and returned the
editor. Revision 20 passed six short closes and holds up to three seconds without
leaking B into gameplay, plus a fresh-press positive control and held-B loading
cancellation. The fix preserves the physical B level between repeated mappings
while consuming gameplay input. These checks used a default single-controller
King Dedede setup with Dr. Mario/Lucario; broader mappings and hardware remain
unverified.

### Direct file import

Direct import without native replay playback is not yet integrated. An existing
file-only decoder reads the researched human-controller packets. The remaining
adapter must establish general record-to-gameplay timing and fighter/controller
ownership before supplying the macro editor. Recorded poses are not required by
native input preview. In the tested CPU replays, original AI decisions are not stored
controller packets; absence must not be treated as a neutral recording. Human
inputs imported from a file could still drive a Training CPU.

For the Vault route, play only through the desired section, then crop the observed timeline. The latest
and previous captures can be reused without replaying that material while the same
game process remains open.
