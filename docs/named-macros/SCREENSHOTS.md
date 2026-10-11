# Screenshot provenance

All twelve images are real native game captures from the October 10–11, 2026
acceptance tests. Nine are used in the current walkthrough; the three superseded
numeric-editor/naming captures remain archived. They are assembled from multiple
test moments, rather than frames from one continuous recording.

## Processing

- Original PNG dimensions: **1280 × 800**.
- Game viewport crop: **left 0, top 39, right 1280, bottom 759**; right/bottom
  coordinates are exclusive.
- Published dimensions: **1280 × 720**.
- Lossless PNG encoding; no resizing, annotations, compositing or game-pixel edits.
- Crops were compared pixel-for-pixel with their original viewport regions.

The crop removes emulator menu/status chrome. Original captures and runtime
evidence remain in the local test archive. Only the walkthrough images and
documentation are published here.

## Sources

Source names below are relative to the local native-test runs archive.

| Published image | Source capture | Context |
| --- | --- | --- |
| [01-native-replay.png](images/01-native-replay.png) | `macro-6/replay-progress.png` | Native Vault replay: Lucario versus CPU Dr. Mario. |
| [02-input-recording.png](images/02-input-recording.png) | `macro-6/input-menu.png` | Input Recording menu in Training, P1 Steve. |
| [03-fighter-capture.png](images/03-fighter-capture.png) | `macro-6/previous-p2-full-window.png` | **Historical numeric editor.** Previous capture; P2 Dr. Mario; full 1,862-frame window. |
| [04-eight-frame-window.png](images/04-eight-frame-window.png) | `macro-6/replay-eight-frame-window.png` | **Historical numeric editor.** In 0, Out 8, Length 8f. |
| [05-name-export.png](images/05-name-export.png) | `macro-6/replay-export-naming.png` | **Historical custom keyboard**, superseded by the native system keyboard. Full 1,862-frame export. |
| [06-named-library.png](images/06-named-library.png) | `macro-6/library-before-failure.png` | Saved exports plus synthetic Acceptance A–F test fixtures. Names are truncated in the tiles. |
| [07-macro-playback.png](images/07-macro-playback.png) | `macro-6/exported-replay-playback.png` | Playback indicator in Training; the HUD does not display the macro name or length. |
| [08-placement-controls.png](images/08-placement-controls.png) | `macro-2/placement-editor.png` | Earlier build, P1 Link; zero-offset controls, not a visible displaced-target preview. |
| [09-visual-editor.png](images/09-visual-editor.png) | `acceptance-fable/runs/parent-17/editor-initial.png` | Current visual strip: P2 Dr. Mario, 1,862 frames, playhead 0. |
| [10-eight-frame-selection.png](images/10-eight-frame-selection.png) | `acceptance-fable/runs/parent-17/g2-selection-restart.png` | Current crop [0, 8), playhead 7 after successful restart and endpoint stop. |
| [11-system-keyboard.png](images/11-system-keyboard.png) | `macro-13/keyboard.png` | Native system keyboard before confirming the eight-frame export; separate from revision 17. |
| [12-preview-loading.png](images/12-preview-loading.png) | `acceptance-fable/runs/parent-17/r1-neutral-00.png` | Actual character-loading interval following Y/Plus test inputs; B closes the editor. |

The first seven images use revision 6 of the local test build. The eighth shows
the earlier placement UI; later momentum/restore verification is described in the
guide's test summary rather than inferred from this screenshot.

For 09/10/12, source paths are under the isolated `eden-replay/acceptance-fable`
archive rather than the older `eden-replay/runs` archive. The new crops use the
same viewport box and pixel equality check. Original eight images are preserved
byte-for-byte. Updating this local guide does not itself publish a new build.

[Back to the walkthrough](README.md)
