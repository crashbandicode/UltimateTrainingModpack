# Image provenance

## Current walkthrough

The [walkthrough](README.md) embeds seven verified game captures added on
2026-10-14 (commits `d3f2ae1` and `6fd5b3a`): `match.gif`, `replay-list.png`,
`fighter.gif`, `crop.gif`, `playback.gif`, `playback-p2.gif` and `naming.gif`.
They show one saved Kirby (P1) versus Hero (P2) match, its capture, and the
**In 0 / Out 449** crop played back on the Training CPU for each player. Their
capture identities are not recorded in a repository manifest; the commit
messages are the in-repository record.

## Earlier screenshots

The sixteen numbered PNGs are real native game captures from the October 10–12,
2026 acceptance tests. None is embedded in the current walkthrough;
`08-placement-controls.png` illustrates the [macro reference](../named-macros.md#placement).
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

[Back to the walkthrough](README.md)
