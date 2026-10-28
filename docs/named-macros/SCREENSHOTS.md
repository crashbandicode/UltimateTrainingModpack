# Image provenance

## Move-start snapping and preview without saving (run162)

[move-snap.gif](images/move-snap.gif) shows an existing armored Mii Gunner replay
capture: **In 4531 → ZL + Y → In 4470**, with Out 4615 unchanged. **X** previews
those 145 inputs without a manual save; the final notice confirms completion.
The private profile had no save-state file and Save States was off. The editor
captured its temporary setup automatically. This starts the nearby move with
its short setup, not the match opening or an arbitrary mid-animation pose.

Recorded on Yoga in Eden `40b6b78a53`, C92 NRO SHA-256
`e8a0a01424f199c93c65754f73f2de47b6dc3aa7ebff1bb07e24ebc741a9ae87`.
The retained capture has 12,692 frames, P2 Gunner, SHA-256
`2ef198ba9f528f68cb70c64c7fa1bbf1b23f55996cbce08003d3ea930be56384`.
Local evidence: `preview-checkpoint162/preview-guide162/` and `gif-accepted.json`.
The earlier run161 fresh-process check also reloaded the saved 145-input macro.

Processing: 120 consecutive frames at 10 fps, 12 seconds. Trim 14 pixels from
top and bottom of the 1920 × 1108 render window, then downscale to 1280 × 720.
No game-pixel edits or annotations. GIF SHA-256
`fbcf43dae78a3fbdb5edc740878a956239489df74b84838a9bb9a5a94b334aba`.

## Earlier Kirby/Hero walkthrough

The [walkthrough](../named-macros.md) retains six of seven verified game captures added on
2026-10-14 (commits `d3f2ae1` and `6fd5b3a`): `match.gif`, `replay-list.png`,
`fighter.gif`, `crop.gif`, `playback.gif`, `playback-p2.gif` and `naming.gif`.
The older `crop.gif` remains available; the walkthrough now uses the Gunner move-start example below.
They show one saved Kirby (P1) versus Hero (P2) match, its capture, and the
**In 0 / Out 449** crop played back on the Training CPU for each player. Their
capture identities are not recorded in a repository manifest; the commit
messages are the in-repository record.

## Earlier screenshots

The sixteen numbered PNGs are real native game captures from the October 10–12,
2026 acceptance tests. None is embedded in the current walkthrough;
`08-placement-controls.png` illustrates the [macro reference](../reference/named-macros.md#placement).
Several show superseded numeric-editor, pose-preview and custom-keyboard
screens. They come from separate test moments, not one continuous recording.

Processing: original PNGs were **1280 × 800**. The game viewport crop is left 0,
top 39, right 1280, bottom 759 (right/bottom exclusive), giving **1280 × 720**
lossless PNGs with no resizing, annotations, compositing or game-pixel edits.
Each crop was compared pixel-for-pixel with its original viewport region. The
crop removes emulator chrome; original captures and runtime evidence stay in the
local test archive.

Source names are relative to the local native-test runs archive. For 09/10/12
and 13–15 they are under the isolated `eden-replay/acceptance-fable` archive
rather than the older `eden-replay/runs` archive.

| Image | Source capture | Context |
| --- | --- | --- |
| [01-native-replay.png](images/01-native-replay.png) | `macro-6/replay-progress.png` | Native Vault replay: Lucario versus CPU Dr. Mario |
| [02-input-recording.png](images/02-input-recording.png) | `macro-6/input-menu.png` | Input Recording menu in Training, P1 Steve |
| [03-fighter-capture.png](images/03-fighter-capture.png) | `macro-6/previous-p2-full-window.png` | **Historical numeric editor.** Previous capture; P2 Dr. Mario; full 1,862-frame window |
| [04-eight-frame-window.png](images/04-eight-frame-window.png) | `macro-6/replay-eight-frame-window.png` | **Historical numeric editor.** In 0, Out 8, Length 8f |
| [05-name-export.png](images/05-name-export.png) | `macro-6/replay-export-naming.png` | **Historical custom keyboard**, superseded by the system keyboard. Full 1,862-frame export |
| [06-named-library.png](images/06-named-library.png) | `macro-6/library-before-failure.png` | Saved exports plus synthetic Acceptance A–F fixtures; names truncated in the tiles |
| [07-macro-playback.png](images/07-macro-playback.png) | `macro-6/exported-replay-playback.png` | Playback indicator in Training; the HUD does not show the macro name or length |
| [08-placement-controls.png](images/08-placement-controls.png) | `macro-2/placement-editor.png` | Earlier build, P1 Link; zero-offset controls, no displaced-target preview |
| [09-visual-editor.png](images/09-visual-editor.png) | `acceptance-fable/runs/parent-17/editor-initial.png` | **Historical pose-preview strip:** P2 Dr. Mario, 1,862 frames, playhead 0 |
| [10-eight-frame-selection.png](images/10-eight-frame-selection.png) | `acceptance-fable/runs/parent-17/g2-selection-restart.png` | **Historical pose preview:** crop [0, 8), playhead 7 after restart and endpoint stop |
| [11-system-keyboard.png](images/11-system-keyboard.png) | `macro-13/keyboard.png` | System keyboard before confirming the eight-frame export; separate from revision 17 |
| [12-preview-loading.png](images/12-preview-loading.png) | `acceptance-fable/runs/parent-17/r1-neutral-00.png` | Character-loading interval after Y/Plus test inputs; B closes the editor |
| [13-native-preview.png](images/13-native-preview.png) | `acceptance-fable/runs/parent-21/native-restart.png` | Revision 21 native preview after Restart, input marker 93; Dr. Mario's capsule visible; Steve is Training P1 |
| [14-native-selection.png](images/14-native-selection.png) | `acceptance-fable/runs/parent-21/g2-selection-restart.png` | Revision 21 [0, 8) selection completed at marker 7; effects and animation finish freely |
| [15-native-stop-restored.png](images/15-native-stop-restored.png) | `acceptance-fable/runs/parent-21/native-stop-restored.png` | Revision 21 after explicit Stop: both saved anchors restored, marker reset to the selection start |
| [16-persistent-capture.png](images/16-persistent-capture.png) | `acceptance-fable/runs/live-r27-reopen/reopen-saved-capture-picker.png` | Revision 27 cold restart with capture off: saved native-name Replay, 3,232 inputs. Pac-Man/Dr. Mario are the Training fighters before captured Pikachu is selected |

Images 01–07 use revision 6 of the local test build. Image 08 shows the earlier
placement UI; later momentum/restore checks are in the reference's test record,
not inferred from this screenshot. Image 16's capture predates CPU selection; it
is not a screenshot of the later eight-input preview or export. All earlier
images were preserved byte-for-byte when later ones were added, using the same
viewport crop and pixel-equality check.

## Record Slot images (run150)

An animated bind and a keyboard still illustrate [Share a numbered Record Slot](../named-macros.md#share-a-numbered-record-slot).
They come from emulator run150 on 2026-10-27, which used the installed NRO with SHA-256
`9d537cd543c04d380cc616abeed047632c366ddac68aeb57e026e974693cc8ba`. Sources are relative to
that run's local archive, `collected150/live/`. The run's receipt `RECORDING150.json` records
the result. Y put the 51-frame `Duck Hunt dash` into Record Slot 3 while the menu stayed open.
Exporting that slot as `Slot Three Verified` produced a file that matches the source in
everything except its name.

| Image | Source | Processing |
| --- | --- | --- |
| [record-slot-library.png](images/record-slot-library.png) | `bind-record-slot3/transition.gif`, frame at 0.48 s, before Y at 1.06 s | 1280 × 720 native frame, 256 colours |
| [record-slot-bind.gif](images/record-slot-bind.gif) | `bind-record-slot3/transition.gif`, 0–5 s | 1280 × 720, 10 fps, 256-colour palette; Y binds and returns to Input Recording |
| [record-slot-keyboard.png](images/record-slot-keyboard.png) | `ui-name-1793118948425029532/b.png`, the native keyboard while naming the export (game paused) | Crop 0, 53, 900, 524 of the 900 × 650 Eden window, which removes only the menu bar, status bar and black letterbox. No scaling |

The binding GIF shows the real list-to-menu transition; the receipt establishes
the copied slot contents. The keyboard pauses the game and is shown as a still.

[Back to the walkthrough](../named-macros.md)
