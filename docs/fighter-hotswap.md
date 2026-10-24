# Hot-swap Player 1 and the CPU

Change the fighter you control, or the CPU, without leaving Training and without
returning to the character select screen. Each role has its own ordered roster.
The game's normal loader replaces the fighter that is already in the match.

The controls below describe the enhancements development build, not a new
published release.

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

Player 1 cycles through the numbered choices, then returns to the original
character-select fighter at **slot 0**, then to slot 1 again. Slot 0 restores
the originally selected starting form. The CPU cycles through its own list;
a CPU outside that list starts at its first choice.

Default lists:

- Player 1: Mr. Game & Watch, Palutena, Pikachu
- CPU: Pikachu, Kirby, Bowser, Bayonetta

## Edit the shared character-cycle grid

Open **Characters → Character Cycles**. Both roles' numbers stay visible:
**red, top-left** is P1 order; **blue, top-right** is CPU order. The original
P1 fighter's whole group normally appears as grey **0**. In the current development
source, Ice Climbers can also have a numbered P1 slot when its red S setting differs
from Character Extras → Solo Popo. Matching configurations remain grey. The CPU's
original fighter remains selectable in its list.

| Control | Action | GameCube equivalent |
| --- | --- | --- |
| ZR | Switch the role being edited | R |
| A | Add the highlighted group, or update its starting form | A |
| X | Preview a grouped starting form | X |
| R | Remove from the active role's list | Z |
| ZL | Select All for the active role | L |
| Y, on Ice Climbers | Toggle the active role's Solo Popo preference | Y |
| B | Return to the menu | B |

Remove and add a group to move it to the end. Close the menu to apply changes.
Previewing a form alone does not change a list or the fighter in play.

**Pyra/Mythra (Aegis)** occupies one slot, and **Pokémon Trainer** occupies one
slot. Preview Pyra, Mythra, Squirtle, Ivysaur or Charizard with X and press A to
set that group's starting form. Updating it keeps its existing order. Normal
in-match transformations still work; the next hot-swap shortcut advances to the
next group rather than another form. Returning to the group starts in its chosen
form again. For example, Mythra can transform to Pyra, then the shortcut advances
to a configured Charizard slot; Charizard can transform to Squirtle, then the
shortcut advances to the next group or P1's original slot 0.

**All** (the Smash Ball tile or ZL) uses character-select row order, left to
right and top to bottom, with each transforming group once. It preserves
configured starting forms (defaulting to Pyra and Squirtle), excludes P1's
original group from the numbered P1 list (except the opposite Solo Popo configuration),
and includes the CPU's original fighter. Each role's All selection is independent.
The layout matches the tested all-unlocked character-select screen; it does not
read arbitrary save layouts. Saved-Mii cycling is available in this alpha; the native Mii editor and managed Mii-helper gates remain disabled. See the [release limits](experimental-release.md).

**Ice Climbers is supported**, as the pair or Solo Popo. The three Solo settings
save independently:

- **Character Extras → Solo Popo** controls only P1's original character-select
  Ice Climbers.
- **Red S, bottom-left** controls P1 who cycles into Ice Climbers.
- **Blue S, bottom-right** controls all CPU Ice Climbers, original or cycled.

Y can edit the red S even on P1's grey slot-0 Ice Climbers portrait. That changes
the future cycle preference without changing the current original P1 or Character
Extras. Each S appears only when enabled; both can appear together. The applicable
setting switches between the pair and Popo alone when the menu closes, and Solo
Popo remains alone after respawn. See the [Solo Popo guide](solo-popo.md).

![Shared cycle grid with red and blue orders, independent Solo Popo toggles, and All.](fighter-hotswap/character-cycles.gif)

*This earlier demonstration shows the original always-grey behavior. The current
development source allows opposite Solo configurations as described above; refreshed
footage will follow validation. The clip switches roles, toggles each S and enables All.*

## Save states and demonstration

A swap waits while a save, a recording, playback, the menu, or another load is
in progress. Save states restore position and damage onto the fighter who is
standing there now. They do not bring back the saved fighter's kind.
Within Pokémon Trainer's group, loading a save also restores the saved Pokémon.
For Pyra/Mythra, loading currently restores position without switching back to
the saved form.

![CPU and Player 1 after cycling to the next roster fighters in Training.](fighter-hotswap/hotswap.gif)

*Recorded in Training with the enhancements development build in Linux Eden. P1 starts as native Mario
and cycles through Solo Popo to Mythra; the CPU changes from Solo Popo to Kirby.
The completed fighters are visible at the end. See the
[development checks](character-cycles-validation.md) for evidence and limits.*
