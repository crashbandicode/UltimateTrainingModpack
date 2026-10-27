# How character changes work

**Unstable enhancements alpha.** This page explains, in plain terms, what the
modpack does when you swap fighters, give Kirby a hat, hand Player 1 another
fighter's item, or use a Mii. See [Release status](experimental-release.md) for
what has been tested and where.

## Swapping a fighter

Every fighter in a match sits in a player "entry" that the game fills by loading
that fighter's files. A swap asks the game's own loader to refill the existing
entry with a different fighter. Nothing is added to character select, and your
controller, team and costume settings stay as they were.

The new fighter starts fresh: practice extras that belonged to the old fighter,
such as a pending hat or item request, are cancelled. Save states keep position
and damage for whoever is standing there now; they do not bring back the fighter
you saved with. In a lobby, swaps change only your **practice** fighter; the real
match always uses your character-select pick.

While another action owns practice, a swap is refused with **Character swap
unavailable during this action**: for example while the menu is still closing, a
save state is loading, a recording is playing, another fighter is loading, or a
lobby dialog is open. Press the shortcut again when practice is idle.

## Slot 0: your original pick

Player 1's numbered cycle (1, 2, 3 …) is followed by **slot 0**, the fighter you
picked on character select, and then starts again at 1. The CPU uses the same
shape for the fighter it actually started as: its numbered choices, then its own
grey slot 0, then slot 1 again. Slot 0 reloads that role's pick exactly as
chosen: its starting form, its Solo Popo setting and, for a Mii, the same Mii.

A fresh session has no slot 0 until that character-select fighter is captured.
A missing identity is not Mii Swordfighter, or any other default. Pyra/Mythra
and Pokémon Trainer take one slot each and start in the form you chose. Down+B
still changes form; the next swap moves to the next slot rather than to another
form.

## Ice Climbers and the three Solo Popo settings

Solo Popo plays Popo without Nana. Three separate settings cover each situation:

| Setting | Applies to |
| --- | --- |
| **Character Extras → Solo Popo** | Player 1's original Ice Climbers (slot 0) |
| Red **S** on the Ice Climbers portrait | Player 1's numbered Ice Climbers slot |
| Blue **S** on the Ice Climbers portrait | The CPU's Ice Climbers, original or cycled in |

Because slot 0 and a numbered slot have separate settings, one Player cycle can
hold both versions, such as original Solo Popo at slot 0 and the pair in a
numbered slot. The original portrait is grey (not addable) only when both Player
settings ask for the same version. The CPU has one Solo Popo setting, so an
Ice Climbers CPU original stays slot 0. Changes apply when you close the menu.
See the [Solo Popo guide](solo-popo.md).

## Kirby hats and character items: a borrowed fighter

Kirby's hat and copied neutral special come from another fighter's data. Some
character items, such as Link's Remote Bomb or Pac-Man's fruit, also need data
that belongs to the fighter who normally uses them. In the normal game that
fighter has to be in the match.

The modpack borrows the data instead. The game keeps spare player entries that
practice does not use: two in Training and two in a waiting room.
The modpack loads the needed fighter into one of them **without bringing it into
the match**, so you never see or fight it. This parked fighter is the hat's or
item's *source*.

![Kirby equips Mario's hat and throws a fireball, then Link's hat and fires an arrow.](feature-walkthroughs/kirby-hats.gif)

*Neither Mario nor Link is in the match: each hat comes from a source fighter loaded out of sight.*

- **Kirby hat:** once the source has loaded, the game's own copy-ability setup
  gives Kirby that fighter's hat and neutral special, as if Kirby had swallowed it.
- **Character item:** the game's own item code creates the item in Player 1's
  hand, replacing anything Player 1 was holding.

A source stays loaded while something uses it: a hat Kirby is wearing, or an
item that still exists, even after it has been thrown. Otherwise it can be reused
for the next request or replaced by another fighter. Leaving Training or the
lobby removes it. Bob-omb, Mr. Saturn and the Banana Peel are ordinary items and
need no source.

Sources load in ordinary Training with exactly one CPU and in Local Wireless
waiting practice. The full experimental build also supports Quickplay
waiting practice. Lobby Practice must be On. Actual matches and unqualified
scenes, including arenas, cannot admit these temporary practice sources.

### Mii hats

For a Mii hat, the source is a Mii loaded with the Mii you selected for that role
in **Character Extras → Mii Brawler / Mii Swordfighter / Mii Gunner**. Kirby then
performs that class's standard copied neutral special (Shot Put, Gale Strike or
Charge Blast), following the game's normal copy rules.

## Hats, items and save states

Loading a save state gives Kirby back the saved hat, using the same source
loading as the shortcut, and gives Player 1 the exact item held when the state
was saved, including a turnip's face.

## Returning to the same lobby after a match

A match tears down practice. When you come back to the **same** local-wireless
lobby, the modpack reloads each role's last practice fighter (Player 1's needs
Player Swapping to stay on) and puts back the hat each Kirby was actually wearing
when the match started. A hat knocked off before the match stays lost. Sources
are loaded again from scratch. A new lobby, or a new Training visit, starts fresh.

## Miis

A Mii fighter is a class (Brawler, Swordfighter or Gunner) plus a configuration:
its look, outfit, hat, colour, voice and four special moves. The modpack treats
two Miis as the same only when class, appearance and moves all match; names do
not matter.

### Your saved Miis

The modpack reads the Mii Fighters in Smash's own Mii list and never edits or
renames them. Each role chooses its own Mii per class, and a cycle loads that Mii
when it reaches the class portrait (steps are in the [Mii guide](mii-fighters.md)).

- Each role's original character-select Mii stays at that role's slot 0 and is
  greyed in its own list. The other role does not show 0 for that class unless
  it is also that role's original. A different saved configuration of the class
  can still be a numbered choice.
- Changing the choice for the class on screen reloads it when you close the
  menu; other choices apply on the next cycle.
- The saved-Mii list loads automatically the first time it is needed, so the
  first shortcut works without opening the menu.

### Add New: making a Mii with the game's editor

**Add New** opens Smash's own Mii Fighter editor. It starts from a copy of your
Player choice for that class, or your first saved Mii of that class, or the
game's default when you have none. The copied Mii is not changed, and **Test**
works as usual.

- The editor's own Save creates a normal Smash Mii. It appears in Smash's Mii
  list outside Training and in both modpack lists.
- Saving an identical Mii again reuses the existing one instead of adding a copy.
- Leaving without saving discards the draft and creates nothing.

The full experimental build enables Player/CPU selections and Add New
in Quickplay and Local Wireless waiting practice. Name, Special Moves, Settings,
Test and cosmetic pages remain available. Its interruption contract preserves
the unfinished screen, value and cursor for the same-lobby return; exact cosmetic
and Local Wireless match-return validation is [still pending](plans/lobby-parity.md).
Explicit Cancel discards the edit. Only the editor's Save keeps a new Mii.

### Opponent Miis from your matches

With replay capture on, saving a replay on the results screen also stores the
fighters' Mii data. Entering Training in the same game session imports it
automatically; watching the replay is unnecessary. The [short walkthrough](mii-replay-discovery/README.md)
shows a new opponent and an already-owned Mii.

- If an identical Mii is already in your Smash Mii list, that one is used.
- Otherwise it is added once to the modpack's own library
  (`sd:/ultimate/TrainingModpack/mii-presets.json`) as, for example,
  **Replay Brawler**. It then appears beside your saved Miis in the modpack
  lists, including after a restart. Choosing the same capture again adds no copy.
- Discovered opponents are never added to Smash's own Mii list.

If the Mii does not appear, selecting its track in **Input Recording
→ Replay Window** retries from the saved capture. Choosing that track also loads
the exact Mii on the Training CPU for input playback.

### Matching HUD portraits

Smash renders a portrait from the selected Mii's face and outfit using the
temporary source fighter. The mod attaches that native image to the HUD, then
releases its temporary rendering resources before leaving waiting practice.
Returning to the lobby rebuilds the portrait with the restored fighter.

This covers both Training fighters and the Local Wireless waiting CPU. The
**full experimental build** includes the Quickplay waiting CPU. The lobby
Ready card keeps the character-select fighter for the real match.
