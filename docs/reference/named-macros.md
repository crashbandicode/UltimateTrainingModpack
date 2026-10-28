# Named CPU macros reference

Controls, files and limits behind the [named macros walkthrough](../named-macros.md).
Build inputs are in [named-macros-build.md](../named-macros-build.md). This local
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

### Numbered Record Slots

*Emulator check, run150: a 51-frame named recording was put into Record Slot 3 with Y.
The slot was then exported as `slot-three-verified.json`. Its schema version, game version,
start state, placement and frames match the source file; only the name differs. That run
did not repeat numbered-slot playback, which an earlier emulator run covered. Nobody has
checked the 600-frame refusal or the exact-Mii refusal in a running game yet. Console
testing is pending.*

| Action | Rule |
| --- | --- |
| **Save Slot to Library** | Exports the current Record Slot as it was captured when that recording locked in: its frames, its own facing, and its start state. A slot with no recording since launch is refused: `Record into this slot first` |
| **Named Library → Y** | Copies the highlighted file into the current Record Slot (footer `Y binds to Record Slot N`). 1–600 frames replace that slot. An empty file or one longer than 600 frames is refused, and nothing changes. Nothing is cut short |
| Exact Mii | A Mii Fighter slot stores the CPU's exact Mii from when recording started. If that was unavailable, export refuses with `Exact Mii was not available when recording started. Record this slot again.` Builds without the Mii alpha features always refuse a Mii slot |

Each slot keeps its own facing. Playing one slot does not change the facing
used by the others. Named playback from the library still allows up to 36,000
frames.

## Placement

After loading a macro and restoring a Training state, open **Edit Placement**.
The anchor is the CPU's restored position; a crosshair previews the target
without moving the real CPU.

![The Placement page: X/Y nudges, Momentum, Keep, Write, New and Back, with a zero offset.](../named-macros/images/08-placement-controls.png)

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
   nothing, and an unsaved in-memory capture is not offered later. No Vault
   playback is needed.
3. In Training, even after a full restart or with capture turned back off, open
   **Input Recording → Replay Window → Replay Captures**, choose the replay name
   and press **A**.
4. Choose the fighter, wait for the CPU reload, then crop, preview and name it
   in the [Replay Window](#replay-window-controls).

Companions live in `sd:/ultimate/TrainingModpack/replay_captures/`. Each `.smd`
bundle holds `manifest.json`, `timeline.jsonl` and a Training `.tmrc` cache; keep
the bundle together. The timeline keeps native floating-point observations, and
rounding to the Training input format happens only in the cache/export projection.
A capture holds at most 36,000 samples per track and 8 tracks; a partial capture
is marked as partial, not presented as a complete match.

## Vault fallback

Optional. Use this only for older or imported replays, matches recorded with
capture off, or a replay without a companion. A live match saved with capture on
does not need it. It observes the CPU while the native replay runs.

1. Play the replay through **Vault → Replays → Replay Data → Play**, only through
   the section you want, including the start of the move and its setup.
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
| **X** | Play the selection from the temporary preview setup, or stop and restore it. A refusal is written in the editor's help strip |
| **Y** | Restart from the temporary preview setup |
| **ZL + X** | Switch latest / previous capture if Vault looped (Z as modifier on GameCube) |
| **ZL + Y** | Snap In to the move's initiating inputs, including its short movement/jump setup |
| **A** | Name and save with the system keyboard; cancelling keeps the crop |
| **B** | Close; an already-started fighter install finishes |

- The crop is `[In, Out)`: it includes In, excludes Out and holds at least one
  input. Handles cannot cross. An unsupported start is rejected without moving In.
- A mid-action In asks for **ZL+Y**. Snapping groups startup, charge and release
  into one move and includes up to two seconds of nearby setup. On setup frames,
  it selects the upcoming move. If no nearby move or trigger was captured, In
  stays unchanged and the editor explains why. Existing captures need no conversion.
  Preview and save refusals go in the editor's two-line help strip, not in the narrow
  notification. If a running preview stops on an error, its status reads
  `Preview stopped. Fix the issue above, then press X.`
- During a fighter load, P1 input is isolated and the CPU must settle to grounded
  idle. The first **X** after both fighters are ready captures a temporary
  in-memory pair checkpoint. It needs no save-state file, works with Save States
  off, and never writes the five numbered slots or setup files. **Y**, and **X**
  after a crop edit, reuse the same anchor. Closing the editor retires it. If
  the fighters change or are on a ledge, the help strip asks you to close, let
  them settle and reopen.
- Preview is transient: it does not replace the loaded library entry or change
  loop, record or playback-on-load settings. Named-macro playback outside the
  editor still uses the saved Training setup.
- Editing a handle stops the preview and restores the checkpoint; it never seeks
  the game to a recorded pose. The playhead counts emitted inputs, independent of
  rendering. At the end, input stops while attacks and effects finish normally.

Preview runs the inputs through the native CPU simulation, so attacks, articles
and effects happen normally and both fighters react; P1's controls stay with the
editor. Stop, restart and close use the native death/rebirth save-state cleanup
and restore the temporary checkpoint's fighter positions, not the whole world.
This is one fighter's input sequence, not a whole-match reconstruction: there is
no pause, backward seek or frame-step, and starting mid-animation is not
supported. AI analog values can fall between controller steps; the editor
reports when it rounded them. Capture reads the game's native simulation rather
than treating the replay file's `FRAM` records as gameplay frames. Never write
the upstream `clamped_rstick` fields: native review identified those bytes as
rumble state.

Snapped clips restart from neutral and replay the captured setup inputs. A
buffered attack can fire earlier than it did during the original move's endlag;
hits and spacing depend on your saved Training position. Snapping does not
restore an animation halfway through, hitstun, or the original opponent's state.

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
| C92 | Temporary preview checkpoint: the 12,692-frame Mii Gunner P2 capture, In 4531 → **ZL+Y** 4470, Out 4615, previewed with no save-state file and Save States off; three **Y** restarts and an Out edit plus **X** kept the same anchor; **B** closed. Eden, normal exit | Physical console review pending |
| C91 | **ZL+Y** move snap on the same Gunner crop: 145 inputs, forward-smash preview and fresh-process named playback | Short neutral setup, then the move; not a mid-animation pose |
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
