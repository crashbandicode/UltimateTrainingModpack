# Named CPU macros

Save a match, open it in Training, crop the part you want, and play those inputs on the CPU.

Before the match, turn on capture in `sd:/ultimate/TrainingModpack/replay_capture.ini`:

```ini
[capture]
enabled=on
```

## The match

Kirby and Hero, from the saved match.

![Kirby and Hero fighting.](images/match.gif)

## Where it was saved

After the game's **Save Replay**, open **Input Recording → Replay Window** in Training to see **Replay Captures**. This one is **Replay · 8299f**: the replay name and its length in frames. Press **A** to open it.

![Replay Captures, with Replay · 8299f selected.](images/replay-list.png)

The companion is written only after Save Replay succeeds. Later Training sessions can open it even with capture turned off.

## Choose the fighter

**D-pad Up/Down** switch between the captured players. The CPU changes to match their fighter.

![The editor switching the CPU from Kirby to Hero.](images/fighter.gif)

## Move the crop

Press **L** for the start or **R** for the end, then hold **Left** or **Right**. The step grows the longer that direction is held. Hold **ZL** or **ZR** to start already fast. A diagonal press keeps moving the selected end. Sticks do not change the fighter.

If the editor says **Needs ZL+Y lead-in**, press **ZL+Y** to include the preceding clean start.

![The In point moving along the capture.](images/crop.gif)

## Play it back

Wait until the captured fighter has finished loading, close the editor, and save a Training setup with **ZL+Down**. A setup saved for a different CPU will not preview. Reopen the capture, set the crop, and press **X**. The CPU runs those inputs from your saved setup; **Y** restarts the preview.

![Kirby previewing the cropped match inputs from a saved Training setup.](images/playback.gif)

## Name and keep it

**A** opens the keyboard. The suggested name is the replay name plus the captured player, such as **Replay P1**. Enter a name, then press **Plus** to save it.

![Naming the cropped inputs Kirby opening, saving, and finding them in the named library.](images/naming.gif)

Find it under **Input Recording → Named Library**. **A** loads it. Close the menu before using your configured playback shortcut; the default is **ZR + D-pad Up**. Playback runs the inputs on the CPU in your current Training setup, so the result can differ from the original match.

The file format and the rest of the controls are in [Named CPU macros](../named-macros.md).
