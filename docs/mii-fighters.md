# Mii Fighters

Choose which saved Mii Fighter Player 1 and the CPU use, make a new one with Smash's own editor, and practice against Miis from your saved replays. This is part of the [unstable enhancements alpha](experimental-release.md); physical-console acceptance is pending.

## Choose a Mii for each role

Open **Character Extras → Mii Brawler**, **Mii Swordfighter** or **Mii Gunner**. Each page has **Player**, **CPU** and **Add New**. The lists show each Mii's special moves, such as `[2111]`. Choosing one never edits the saved Mii.

![Selecting Replay Brawler [2111], cycling to that Mii, and performing Flashing Mach Punch.](feature-walkthroughs/mii-selection.gif)

*Replay Brawler is chosen for Player, then loaded by the cycle shortcut. Neutral B uses its punching flurry.*

## Use it in a cycle

Add the class portrait in **Characters → Character Cycles**. When the [cycle](fighter-hotswap.md) reaches it, that role loads the Mii you chose. If that class is on screen now, it reloads when you close the menu; other choices apply on a later cycle. Player 1's own character-select Mii stays at slot 0, so it is greyed in the Player list.

## Make a new Mii

**Add New** opens Smash's Mii Fighter editor with a copied starting configuration; **Test** works as usual. The editor's **Save** creates a normal Smash Mii. Leaving without saving creates nothing, and the copied Mii never changes.

![Smash's Mii Fighter editor, opened from Add New.](feature-walkthroughs/mii-editor.png)

In an online (Quickplay) waiting room, the pages offer only Add New. Headgear, Outfit, Color, Voice and Random stay available. If a match interrupts the editor, that same unfinished screen, value, and cursor reopen when you return to waiting practice. Explicit Cancel still leaves without saving. Local Wireless waiting practice opens Add New and Test on its own parent and returns to that room, including after a match interrupt. Headgear, Outfit, Color, Voice, and Random stay on their native Local Wireless rows.

## Opponents from your replays

With [replay capture](named-macros.md#turn-on-capture) on, **Save Replay → enter Training** adds an unfamiliar opponent's Mii to the modpack library, or reuses an identical saved Mii. No replay playback is needed. [Watch both cases.](mii-replay-discovery/README.md)

## Matching portraits

In Training, Player and CPU HUD portraits follow the selected Mii's face, outfit and colors, rendered by Smash itself. Local Wireless waiting practice updates the CPU icon too; the Ready card keeps the character-select fighter. Quickplay portraits are unchanged.

![Saved Brawler and Swordfighter in Training, with matching custom portraits beside their damage percentages.](feature-walkthroughs/mii-hud-portraits.png)

For storage and matching rules, see [How character changes work](character-changes.md#miis).
