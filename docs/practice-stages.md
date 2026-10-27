# Practice Stages

Change the stage you practice on without going back to character select, and choose which stages the waiting room rotates through. This is part of the [unstable enhancements alpha](experimental-release.md); physical-console acceptance is pending.

## Change the stage now

Open **Misc Settings → Practice Stages**. It opens at **Change Stage Now**: highlight a stage and press **A**. The menu closes and the stage changes. **L/R** turn the page, and the list also includes **Training** and **Waiting Room**.

![Applying Peach’s Castle in Battlefield form and returning to practice.](practice-stages/forms.gif)

*Peach’s Castle, Battlefield form. The full loading transition is kept. [Capture details](practice-stages/SCREENSHOTS.md).*

This works in Training and in lobby practice. In Training the new stage lasts for that session only. In a lobby, **A** also keeps the stage for your later lobby entries.

### Pick a form

**New in [C89](lobby-parity-release.md):** press **Y** on a named stage to choose its form. Normal, Omega and Battlefield are separate choices, and **A** applies the form you chose. The help line names the current form, then the forms Y moves to next:

| Stage has | Help line starts with |
| --- | --- |
| All three forms | `Normal \| Y Form: Omega, Battlefield` |
| Normal and Omega (Battlefield) | `Normal \| Y Form: Omega` |
| Normal and Battlefield (Final Destination) | `Normal \| Y Form: Battlefield` |
| Normal only | `Normal \| Y Form` (Y does nothing) |

Y never applies a stage or changes the rotation order. Change Stage Now and Lobby Stage Rotation keep their own forms.

## Rotate stages between lobby visits

Press **X** to open **Lobby Stage Rotation**, then press **A** on stages in the order you want to visit them. The rotation advances each time you enter lobby practice, including after a match. Ordinary Training follows it only while **Misc Settings → Training Stage Rotation** is On (default Off): each Training entry from character select takes the next stage. Press **X** again to return to Change Stage Now.

![Waiting Room, Battlefield and Small Battlefield selected as lobby stages 1, 2 and 3.](feature-walkthroughs/stage-rotation.png)

*Numbers show the order for future lobby entries. Editing this list does not change the current Training stage.*

If a lobby is keeping a stage you chose with **A**, open Lobby Stage Rotation before you close the menu to go back to rotating.

Lobby use needs **Misc Settings → Lobby Practice** turned on; see [Lobby Practice](lobby-practice.md). The online legal-stage pool and loser counterpick are in a separate plugin, [Stages + Counterpick](elite-counterpick.md).
