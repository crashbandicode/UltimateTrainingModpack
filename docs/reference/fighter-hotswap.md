# Hot-swap and Solo Popo reference

Details behind the [hot-swap guide](../fighter-hotswap.md) and the
[Solo Popo guide](../solo-popo.md), for the
[enhancements alpha](../experimental-release.md). Loading, slot 0 and
source fighters are explained in [how character changes work](../character-changes.md);
waiting-room rules are in the [Lobby Practice reference](lobby-practice.md).

## Shortcuts

| Action | Default chord | Notes |
| --- | --- | --- |
| Cycle the CPU | **ZL + D-pad Left** | Wait until both fighters are standing in the match |
| Cycle Player 1 | **ZL + D-pad Right** | Off until **Characters → Player Swapping** is On |

Open the Modpack menu with **Minus** (a short **Plus** press still opens the
game's pause menu) and close it with **B**. Each press advances one fighter;
holding the chord does not repeat. Change the chords under **Button Config →
Cycle Player Character / Cycle CPU Character**. Fighters load without leaving
practice, with no matching character-select pick or re-entry.

## Cycle order

| Role | Order | Defaults |
| --- | --- | --- |
| Player 1 | Numbered fighters, then **slot 0** (your character-select fighter in its original starting form), then slot 1 again | Mr. Game & Watch, Palutena, Pikachu |
| CPU | Its own numbered fighters, then **slot 0** (the CPU's captured character-select fighter), then its first numbered choice again | Pikachu, Kirby, Bowser, Bayonetta |

A fresh Training visit clears the previous cycle position, including after a
Mii swap, so original Ice Climbers follows its own Solo setting.

## Character Cycles grid

Open **Characters → Character Cycles**. **Red, top-left** numbers are P1's
order; **blue, top-right** numbers are the CPU's. The button table is in the
[guide](../fighter-hotswap.md#set-the-order); directional inputs move the marker.

- Close the menu to apply changes. Previewing a form alone changes nothing.
- Remove and add a group to move it to the end. There is no eight-fighter limit.
- Each role's original group appears as grey **0** on that role's numbers and
  cannot be added, except Player 1's Ice Climbers with the opposite Solo Popo
  setting, or a Mii class with another saved Mii chosen (see the
  [Mii guide](../mii-fighters.md)). An unadmitted Mii class is not slot 0.
- These are plugin roster settings, not new game slots. Portrait icons are the
  SmashWiki stock icons listed in the
  [asset manifest](../../scripts/roster-grid/assets/sources.json).

**Grouped fighters.** Pyra/Mythra (Aegis) and Pokémon Trainer take one slot each.
Preview Pyra, Mythra, Squirtle, Ivysaur or Charizard with **X**, then press **A**
to set that group's starting form; its place in the order stays. Down+B still
transforms in the match, the next shortcut moves to the next group, and
returning to the group starts in its chosen form again. Example: Mythra → (Down+B)
Pyra → shortcut → Charizard → (Down+B) Squirtle → shortcut → next group or slot 0.

**All** (the Smash Ball tile, or **ZL**) follows the character-select rows, left
to right and top to bottom, with each group once in its configured starting form
(default Pyra and Squirtle). It excludes that role's original group, except
Player 1's opposite Solo Popo configuration. Each role's All is independent,
and picking a fighter while All is active starts a custom order.
The layout matches the tested all-unlocked character-select screen; other save
layouts are not read.

## Choose a fighter directly

Open **Characters → Player Character** or **CPU Character**, choose a portrait
and press **A**; the portrait flashes. **ZR** switches roles and **X** previews a
form. Neither numbered cycle changes.

## Solo Popo settings

The three independent settings (Character Extras for P1's original, red S for
cycled-in P1, blue S for every CPU Ice Climbers) are in the
[Solo Popo guide](../solo-popo.md). Beyond the guide:

- Each S shows only when enabled; both can show together. **Y** on P1's grey
  slot-0 portrait edits the red S without changing the current fighter or
  Character Extras, and editing one preference never overwrites another.
- Slot 0 always uses Character Extras; a numbered Ice Climbers slot uses the red S.

Tested: Eden verified original Solo Popo → numbered paired Ice Climbers →
original Solo Popo, and the Training and solo local-wireless checks in the
[development validation](../character-cycles-validation.md#enhancements-development-build--2026-10-16)
passed. Physical-console acceptance is pending; later corrections are in the
[release notes](../experimental-release.md).

## Save states and conflicting actions

A swap waits while a save, recording, playback, the menu or another load is in
progress. Save states restore position and damage onto the fighter standing
there now, not the saved fighter. Within Pokémon Trainer's group a load also
restores the saved Pokémon; Pyra/Mythra currently keeps the current form.

## Recordings

The guides' clips are real Linux Eden Training recordings. What each shows, its
date and its build are in the [development checks](../character-cycles-validation.md#refreshed-demonstrations);
hashes and review notes are in [media-provenance.json](../media-provenance.json).
