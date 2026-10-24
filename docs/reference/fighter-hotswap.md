# Hot-swap and Solo Popo reference

Details behind the [hot-swap guide](../fighter-hotswap.md) and the
[Solo Popo guide](../solo-popo.md). They describe the
[unstable enhancements alpha](../experimental-release.md). How loading,
slot 0 and source fighters work is in [how character changes work](../character-changes.md).
Waiting-room behavior is in the [Lobby Practice reference](lobby-practice.md).

## Shortcuts and cycle order

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
the originally selected starting form. A fresh Training visit clears the
previous cycle position, including after a Mii swap, so original Ice Climbers
follows its own Solo setting. The CPU cycles through its own list; a CPU outside
that list starts at its first choice.

Default lists:

- Player 1: Mr. Game & Watch, Palutena, Pikachu
- CPU: Pikachu, Kirby, Bowser, Bayonetta

## Shared character-cycle grid

Open **Characters → Character Cycles**. Both roles' numbers stay visible:
**red, top-left** is P1 order; **blue, top-right** is CPU order. The original
P1 fighter's whole group normally appears as grey **0**. In the enhancements
alpha, Ice Climbers can also have a numbered P1 slot when its red S setting
differs from Character Extras → Solo Popo. Matching configurations remain grey.
The CPU's original fighter remains selectable in its list.

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
original group from the numbered P1 list (except the opposite Solo Popo
configuration), and includes the CPU's original fighter. Each role's All
selection is independent. The layout matches the tested all-unlocked
character-select screen; it does not read arbitrary save layouts. Saved-Mii
cycling, the native Mii editor and Mii hats are included in the full alpha; see
the [Mii guide](../mii-fighters.md).

## Choose a fighter directly

Open **Characters → Player Character** or **CPU Character**. Choose a portrait
and press **A** to apply it; the portrait briefly flashes as feedback. **ZR**
switches roles and **X** previews a starting form. This changes the current
practice fighter without changing either numbered cycle.

## Solo Popo settings

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
Popo remains alone after respawn.

The original Ice Climbers portrait is grey only when Character Extras and the
red S request the same configuration. When they differ, add Ice Climbers to the
cycle to switch between the pair and Solo Popo. Slot 0 uses Character Extras;
the numbered slot uses the red S. Editing one preference never overwrites the
other. Eden verified original Solo Popo → numbered paired Ice Climbers →
original Solo Popo. Physical-console acceptance of this refinement remains
pending. Later corrections are recorded in the [release notes](../experimental-release.md).

## Save states and conflicting actions

A swap waits while a save, a recording, playback, the menu, or another load is
in progress. Save states restore position and damage onto the fighter who is
standing there now. They do not bring back the saved fighter's kind.
Within Pokémon Trainer's group, loading a save also restores the saved Pokémon.
For Pyra/Mythra, loading currently restores position without switching back to
the saved form.

## Recordings

The guides embed these Linux Eden Training clips. Identities and review notes
are in [media-provenance.json](../media-provenance.json).

- `fighter-hotswap/hotswap.gif`: recorded with the enhancements development
  build of 2026-10-16. P1 starts as native Mario and cycles through Solo Popo to
  Mythra; the CPU changes from Solo Popo to Kirby. The completed fighters are
  visible at the end.
- `fighter-hotswap/character-cycles.gif`: normal Ice Climbers is added as Player
  slot 2 while Character Extras keeps original slot 0 as Solo Popo. The shortcut
  then cycles through Kirby, the pair, and original Solo Popo. CPU Young Link
  stays unchanged.
- `fighter-hotswap/manual-hotswap.gif`: Player Ice Climbers changes to Bowser.
  CPU Young Link remains unchanged.
- `fighter-hotswap/solo-popo.gif`: Character Extras → Solo Popo turns the
  original Player Ice Climbers into Popo alone. Young Link is the CPU.

See the [development checks](../character-cycles-validation.md) for evidence
and limits.
