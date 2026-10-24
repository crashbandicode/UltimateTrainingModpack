# Named CPU macros

Save a match, open it in Training, pick either captured player, crop the part
you want, and let the Training CPU play those inputs. Pick your opponent to
practice against their inputs.

Kirby is our player (P1) and Hero is the opponent (P2) in the saved match used
throughout this page:

![The opening exchange between Kirby P1 and Hero P2.](named-macros/images/match.gif)

Macro preview/playback runs in Training. Physical-console acceptance of the
current alpha is pending. Full controls, physics, files and test evidence are in
the [reference](reference/named-macros.md).

## Turn on capture

Do this before starting Smash. The modpack creates
`sd:/ultimate/TrainingModpack/replay_capture.ini` on its first run, with capture
off. With the game closed, set:

```ini
[capture]
enabled=on
```

## Save the match

Play a match, then use the game's **Save Replay** on the results screen. The
capture is kept only if Save Replay succeeds. You do not need to play the
replay in the Vault.

## Open the capture

In Training, open **Input Recording → Replay Window → Replay Captures**. Each
entry is the replay name and its length in frames, here **Replay · 8299f**.
Press **A** to open it. Saved captures stay available after a restart, even with
capture turned off.

![Replay Captures, with Replay · 8299f selected.](named-macros/images/replay-list.png)

## Choose the fighter

**D-pad Up/Down** switch between the captured players, and the CPU reloads as
that fighter. P1 and P2 are players in the saved match, not your current
controllers.

![The editor switching the CPU from Kirby to Hero.](named-macros/images/fighter.gif)

## Crop

Press **L** for the start or **R** for the end, then hold **Left** or **Right**;
the step speeds up the longer you hold. Hold **ZL** or **ZR** to start fast. If
the editor shows **Needs ZL+Y lead-in**, press **ZL+Y**.

![The In point moving along the capture.](named-macros/images/crop.gif)

## Preview

Wait for the fighter to finish loading, close the editor, and save a Training
setup with **ZL + D-pad Down**. Reopen the capture and press **X** to play the
crop from that setup; **Y** restarts. A setup saved with a different CPU will
not preview.

Both clips use **In 0 / Out 449**, the match opening above. They start from a
Training setup, so positions, hits and timing differ from the match.

**P1 / Kirby's** inputs on the Training CPU, with King Dedede as the Training
player:

![The Training CPU playing captured P1 Kirby inputs, with King Dedede as the current Training player.](named-macros/images/playback.gif)

**P2 / Hero's** inputs: select P2, wait for Hero to load, and save a new setup
before previewing.

![The Training CPU playing the opponent's captured P2 Hero inputs.](named-macros/images/playback-p2.gif)

## Name and keep it

Press **A**. The keyboard suggests the replay name plus the player, such as
**Replay P1**; enter a name and press **Plus**. This clip exports the P1 crop as
**Kirby opening** and finds it in the named library.

![Naming the cropped P1 inputs Kirby opening, saving, and finding them in the named library.](named-macros/images/naming.gif)

## Record and load

1. Set **Input Recording → Macro Source** to **Named Library**.
2. In **Input Recording → Named Library**, press **A** on a recording. The CPU
   changes to its fighter; close the menu and let the change finish.
3. Save the Training setup you want, then press **Input Playback** (default
   **ZR + D-pad Up**). The setup is restored, then the inputs play on the CPU.

To record by hand instead, keep **Named Library** as the source and press
**Input Record** (default **ZR + D-pad Down**) to start and again to finish, then
name it. The clips on this page show replay exports, not hand recording.

## Placement

**Input Recording → Edit Placement** moves where a loaded macro starts,
relative to the CPU's restored position, and can keep or clear its recorded
momentum. See the [placement controls](reference/named-macros.md#placement).

## Vault fallback

Optional. Only for older or imported replays, matches saved with capture off,
or a replay without a capture: play it in the Vault, then open the Replay Window
in the same game session. See the [Vault steps](reference/named-macros.md#vault-fallback).

## More

- [Reference](reference/named-macros.md): all controls, placement and files
- <a name="capture-a-match-optional"></a>[Capture and file format](reference/named-macros.md#capture-a-match-optional)
- <a name="replay-window-controls"></a>[Replay Window controls](reference/named-macros.md#replay-window-controls)
- <a name="direct-file-import"></a>[Direct file import](reference/named-macros.md#direct-file-import)
- <a name="test-record"></a>[Test record](reference/named-macros.md#test-record)
- [Build inputs](named-macros-build.md)
- [Image provenance](named-macros/SCREENSHOTS.md)
