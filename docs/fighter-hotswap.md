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
   the game's pause menu), turn on **Player Characters → Player Swapping**, and
   close the menu with **B**.

The chord advances one fighter per press. Holding it does not keep cycling.
Change the chords under **Button Config → Cycle Player Character / Cycle CPU
Character**.

A fighter that is already in that role's list moves to the next one. A fighter
that is not in the list starts the list from the beginning, then wraps.

Default lists:

- Player 1: Mr. Game & Watch, Palutena, Pikachu
- CPU: Pikachu, Kirby, Bowser, Bayonetta

Edit either list under **Cycle Characters** in that role's tab. **All (menu
order)** walks the whole supported chooser. Pokémon Trainer, Pyra/Mythra, Ice
Climbers, Miis, and non-playable forms are not in the chooser.

A swap waits while a save, a recording, playback, the menu, or another load is
in progress. Save states restore position and damage onto the fighter who is
standing there now. They do not bring back the saved fighter's kind.

![CPU and Player 1 after cycling to the next roster fighters in Training.](fighter-hotswap/hotswap.gif)

*Recorded in Training with the Linux Eden build `9a5d704c9c`. The match starts
on the character-select fighters. ZL+Left moves the CPU onto the CPU roster, and
ZL+Right moves Player 1 onto the Player 1 roster.*
