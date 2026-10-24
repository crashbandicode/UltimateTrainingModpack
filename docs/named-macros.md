# Named CPU macros

Controls, files and limits behind the [illustrated guide](named-macros/README.md).
Build inputs are in [named-macros-build.md](named-macros-build.md). This local
implementation passed scoped emulator acceptance; physical Switch and online
behavior are unverified.

## Record and load

1. In **Input Recording → Macro Source**, select **Named Library**. The default
   stays numbered slots, so existing settings behave as before.
2. Press the **Input Record** chord (default **ZR + D-pad Down**) to possess the CPU and record; press it again
   to finish. Named recordings use a ten-minute safety limit instead of the
   numbered-slot duration setting.
3. The **Save Recording** panel shows the frame count and duration. **A** opens
   the system keyboard to name and save; cancelling or an invalid name keeps the
   draft for a retry. **Y** opens placement. **Recording Crop** optionally trims
   trailing neutral input.
4. **Named Library** lists saved files. Selecting one requests its CPU fighter;
   close the menu and let the change finish.
5. Enable save states and save your Training setup. The **Input Playback** chord
   (default **ZR + D-pad Up**) restores that setup, then starts the macro.

Files are versioned JSON in `sd:/ultimate/TrainingModpack/macros/`, with mapped
buttons and sticks, a start-state summary and placement metadata. There is no
five-slot limit, and the selected ID persists across restarts. A macro is an
input sequence, not a full simulation save state.

## Placement

After loading a macro and restoring a Training state, open **Edit Placement**.
The anchor is the CPU's restored position; a crosshair previews the target
without moving the real CPU.

![The Placement page: X/Y nudges, Momentum, Keep, Write, New and Back, with a zero offset.](named-macros/images/08-placement-controls.png)

*Earlier test build; the current source uses the same controls and footer. At zero offset no displaced target is shown.*

| Control | Effect |
| --- | --- |
| **X- / X+ / Y- / Y+** | Move the target one unit (lowercase controls: a quarter unit) |
| **Keep** | Apply the edit in memory; the file is unchanged |
| **Write** | Overwrite the loaded file's placement metadata |
| **New** | Name and write a separate recording |
| **Back** | Cancel the edit |
| **Momentum** | Switch between **Keep recorded speed** and **Start at rest** |

- Placement never rewrites inputs. The offset is in world units from the restored
  reference, including a mirrored save-state anchor. Repeated restores reuse that
  reference rather than adding to the CPU's current position; the recorded
  absolute position is not restored.
- Recorded momentum is applied just before input zero, after both fighters
  restore. Horizontal speed follows facing mirroring, and an airborne start
  requests normal falling physics even at zero Y offset. **Start at rest** clears
  the starting velocity, then ordinary physics resumes; it does not hold the CPU
  in the air. A plain load or recording start applies no momentum.
- Use an idle or falling setup. An unshifted ledge save is rejected rather than
  silently releasing the ledge. An aerial target on the floor can land before
  playback is ready; the macro then stops without consuming input 0 and asks you
  to adjust. Raise the placement or save an airborne setup.

## Capture a match (optional)

1. Start the mod once to create `sd:/ultimate/TrainingModpack/replay_capture.ini`.
   Capture defaults **off**. With the game closed, set:

   ```ini
   [capture]
   enabled=on
   ```

2. After a match, use the game's **Save Replay** on the results screen. The
   companion is written only after that native save succeeds; cancelling writes
   nothing, and an unsaved in-memory capture is not offered later.
3. In Training, even after a full restart or with capture turned back off, open
   **Input Recording → Replay Captures**, choose the replay name and press **A**.
4. Choose the fighter, wait for the CPU reload, save a matching Training setup,
   then crop, preview and name it in the [Replay Window](#replay-window-controls).

Companions live in `sd:/ultimate/TrainingModpack/replay_captures/`. Each `.smd`
bundle holds `manifest.json`, `timeline.jsonl` and a Training `.tmrc` cache; keep
the bundle together. The timeline keeps native floating-point observations, and
rounding to the Training input format happens only in the cache/export projection.
A capture holds at most 36,000 samples per track and 8 tracks; a partial capture
is marked as partial, not presented as a complete match.

## Vault fallback

Use this for older or imported replays, matches recorded with capture off, or a
replay without a companion. It observes the CPU while the native replay runs.

1. Play the replay through **Vault → Replays → Replay Data → Play**, only through
   the section you want plus enough lead-in for an idle or falling start.
2. Quit with the game's displayed combination and enter Training **without
   restarting**. The latest two observed timelines stay in memory for that game
   process, so they can be reused without replaying.
3. Open **Input Recording → Replay Window** while the CPU is idle or falling.

## Replay Window controls

| Control | Effect |
| --- | --- |
| **D-pad Up / Down** | Previous / next captured fighter. Changing kind reloads the CPU and keeps the crop; only a clean Up/Down changes fighter |
| **L** / **R** | Select the In / Out handle |
| **Left / Right** | Move the selected handle. Holding speeds up from one frame to three minutes of replay per second; a diagonal still moves the handle |
| Hold **ZL** or **ZR** (Z on GameCube) | Start moving at the fast rate |
| **X** | Play the selection from the saved setup, or stop and restore it |
| **Y** | Restart from the saved setup |
| **ZL + X** | Switch latest / previous capture if Vault looped (Z as modifier on GameCube) |
| **ZL + Y** | Include the previous clean-start lead-in |
| **A** | Name and save with the system keyboard; cancelling keeps the crop |
| **B** | Close; an already-started fighter install finishes |

- The crop is `[In, Out)`: it includes In, excludes Out and holds at least one
  input. Handles cannot cross. An unsupported start is rejected without moving In.
- During a fighter load, P1 input is isolated and the CPU must settle to grounded
  idle. Before previewing, save a setup with the current pair of fighters; after
  choosing a different CPU, close the editor, save a new setup and reopen.
- Preview uses the explicitly selected save slot with its original facing and
  saved damage, ignoring random-slot, mirroring and damage options. It is
  transient: it does not replace the loaded library entry or change loop,
  record or playback-on-load settings.
- Editing a handle stops the preview and restores the setup; it never seeks the
  game to a recorded pose. The playhead counts emitted inputs, independent of
  rendering. At the end, input stops while attacks and effects finish normally.

Preview runs the inputs through the native CPU simulation, so attacks, articles
and effects happen normally and both fighters react; P1's controls stay with the
editor. Stop, restart and close use the native death/rebirth save-state cleanup
and restore the saved setup, not the world as it was when the editor opened.
This is one fighter's input sequence, not a whole-match reconstruction: there is
no pause, backward seek or frame-step, and starting mid-animation is not
supported. AI analog values can fall between controller steps; the editor
reports when it rounded them. Capture reads the game's native simulation rather
than treating the replay file's `FRAM` records as gameplay frames. Never write
the upstream `clamped_rstick` fields: native review identified those bytes as
rumble state.

## Direct file import

Not integrated yet. A file-only decoder reads the researched human-controller
packets, but the adapter must still establish record-to-gameplay timing and
fighter/controller ownership before feeding the editor. In the tested CPU
replays, the original AI decisions are not stored as controller packets; their
absence must not be read as neutral input. Human inputs imported from a file
could still drive a Training CPU.

## Test record

| Revision | Passed | Notes |
| --- | --- | --- |
| 27 | Saved capture reopened after a process restart with capture off; eight-input Pikachu preview; named export. With Deluxe `6c396f10` in active manual LessLag: native match/save, cold Training with capture off, CPU reload, eight-input preview, exact named export | Current capture route |
| 21 | Native-preview backend: full 1,862-input preview with a visible Dr. Mario capsule, attacks/effects, restart, explicit stop/restore, two eight-input selection runs, held-B close/restore, native-menu exit. Deliberately enabled legacy loop and record/playback-on-load options did not interfere. Strict host/native checks | Evidence: the parent's `research/named-macros/revision21-findings.md` |
| 20 | Earlier pose-preview backend (since replaced): loading input isolation, B cancellation, grounded CPU acquisition, HUD/statistics hiding, selection restart, anchor restoration, closed-editor Training leave/re-entry. Six short closes and holds up to three seconds without B leaking into gameplay, a fresh-press positive control, held-B loading cancellation; the non-ready loading timeout kept the selection | The fix preserves the physical B level between repeated mappings while consuming gameplay input. Default single-controller King Dedede with Dr. Mario/Lucario |
| Earlier | Manual recording, naming cancel/validation/retry, library operations. Dr. Mario placement: rising/falling momentum, start at rest, mirrored controls and speed, repeated non-compounding restores | — |

Capture accuracy: a 120-frame writer/native-getter comparison matched exactly.
All 1,862 inputs of the longer fixture were written once, in order, and matched
native getter values, with three extra observations of one sequence. Repeated
callbacks prevent a universal one-callback-per-gameplay-tick claim.

Not yet verified: broader airborne, roster and controller behavior; forced
timeout recovery; strict callback-to-tick timing, first/last simulation-tick
coverage and every sampling phase; general C-stick/remapping coverage;
controlled performance and deliberate frame-drop checks (capture stays opt-in);
physical Switch and online play.
