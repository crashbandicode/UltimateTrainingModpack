# Hot-swap Player 1 and the CPU

Change the fighter you control, or the CPU, without leaving Training and without
returning to the character select screen. Each role has its own ordered roster.
The game's normal loader replaces the fighter that is already in the match.

This page is the Training control. Waiting-room practice, the roster editor, and
what an actual match keeps from the character select screen are in
[Lobby Practice and shared character rosters](lobby-practice.md).

## Swap during a match

1. Enter ordinary Training and wait until both fighters are standing in the match.
2. **ZL + D-pad Left** cycles the CPU.
3. **ZL + D-pad Right** cycles Player 1. Player 1 swapping is off until you enable
   it: open the Modpack menu with **Minus** (a short **Plus** press still opens
   the game's pause menu), turn on **Characters → Player Swapping**, and
   close the menu with **B**.

The chord advances one fighter per press. Holding it does not keep cycling.
Change the chords under **Button Config → Cycle Player Character / Cycle CPU
Character**.

A fighter that is already in that role's list moves to the next one. A fighter
that is not in the list starts the list from the beginning, then wraps.

Default lists:

- Player 1: Mr. Game & Watch, Palutena, Pikachu
- CPU: Pikachu, Kirby, Bowser, Bayonetta

The development build has one **Characters → Character Cycles** portrait grid
for both lists. Player order is red and CPU order is blue. **Y** switches the
list being edited; **A** adds the highlighted fighter, **R** removes it, and
**ZL** selects All. Remove and add a fighter to move it to the end. **B** returns
to the menu; close the menu to apply your changes.

At Pyra or Pokémon Trainer's slot, **X** previews the next individual form.
Press **A** to add that form: Pyra, Mythra, Squirtle, Ivysaur, or Charizard.
Normal in-match transformations still work. Previewing an icon does not change
either list or the fighter in play. Ice Climbers and Miis keep their familiar
positions but are dimmed and unavailable for swapping.

The grid matches the all-unlocked character-select layout used for these demos.
All cycles through the stable 85-entry catalog; existing explicit lists keep
their saved order. On a GameCube controller, **Z** removes and **L** selects All.

![Switching Pyra/Mythra previews and adding forms to the separate red player and blue CPU orders.](fighter-hotswap/character-cycles.gif)

A swap waits while a save, a recording, playback, the menu, or another load is
in progress. Save states restore position and damage onto the fighter who is
standing there now. They do not bring back the saved fighter's kind.
Within Pokémon Trainer's group, loading a save also restores the saved Pokémon.
For Pyra/Mythra, loading currently restores position without switching back to
the saved form.

![CPU and Player 1 after cycling to the next roster fighters in Training.](fighter-hotswap/hotswap.gif)

*Recorded in Training with the development build in Linux Eden. ZL+Right cycles
Player 1 from Mythra to Pyra; then ZL+Left cycles the CPU from Charizard to
Mythra. Both replacements finish loading in the clip.*
