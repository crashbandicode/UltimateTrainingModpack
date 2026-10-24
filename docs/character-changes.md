# How character changes work

**Unstable enhancements alpha.** This page explains, in plain terms, what the
modpack does when you swap fighters, give Kirby a hat, hand Player 1 another
fighter's item, or use a Mii. It describes the current alpha only; see
[Release status](experimental-release.md) for what has been tested and where.

## Swapping a fighter

Every fighter in a match sits in a player "entry" that the game fills by loading
that fighter's files. A swap does not add anything to character select. It asks
the game's own loader to refill the entry that is already there with a different
fighter. Your controller, team and costume settings stay as they were.

Loading takes a moment. While another action owns practice, the swap is refused
with **Character swap unavailable during this action**: for example while the
menu is still closing, a save state is loading, a recording is playing, another
fighter is loading, or a lobby dialog is open. Press the shortcut again when
practice is idle.

A swap starts the new fighter fresh. Practice extras that belonged to the old
fighter, such as a pending hat or item request, are cancelled. Save states keep
position and damage for whoever is standing there now; they do not bring back
the fighter you saved with.

In a lobby, swaps only change your **practice** fighter. The real match always
uses the fighter you picked on character select.

## Slot 0: your original pick

Player 1's numbered cycle (1, 2, 3 …) is followed by **slot 0**: the fighter you
picked on character select. After the last numbered fighter the shortcut returns
to slot 0, then starts again at 1. Going back to slot 0 reloads your original
pick exactly as you chose it: its starting form, its Solo Popo setting and, for a
Mii, the same Mii.

The CPU has no slot 0. It cycles only through its own list. If the CPU is
currently a fighter outside that list, the next press starts at the CPU's first
choice.

Pyra/Mythra and Pokémon Trainer each take one slot and start in the form you
chose. Normal Down+B changes still work; the next swap moves on to the next
slot rather than to another form.

## Ice Climbers and the three Solo Popo settings

Solo Popo plays Popo without Nana. There are three separate settings, so each
situation can be different:

| Setting | Applies to |
| --- | --- |
| **Character Extras → Solo Popo** | Player 1's original Ice Climbers (slot 0) |
| Red **S** on the Ice Climbers portrait | Player 1's numbered Ice Climbers slot |
| Blue **S** on the Ice Climbers portrait | The CPU's Ice Climbers, original or cycled in |

Because slot 0 and a numbered slot have separate settings, Player 1 can practice
both versions in one cycle: for example original Solo Popo at slot 0 and the
pair in a numbered slot. The original portrait is grey (not addable) only when
both Player settings ask for the same version. Changing a setting switches
between Popo alone and the pair when you close the menu.

## Kirby hats and character items: a borrowed fighter

Kirby's hat and copied neutral special come from another fighter's data. Some
character items, such as Link's Remote Bomb or Pac-Man's fruit, also need data
that belongs to the fighter who normally uses them. In the normal game that
fighter has to be in the match.

The modpack borrows the data instead. The game keeps spare player entries that
practice does not use: two in Training and two in a local-wireless waiting room.
The modpack loads the needed fighter into one of those spare entries **without
bringing it into the match**, so you never see or fight it. This temporary,
parked fighter is the hat's or item's *source*.

- **Kirby hat:** once the source has loaded, the modpack asks the game's own
  copy-ability setup to give Kirby that fighter's hat and neutral special, as if
  Kirby had swallowed it.
- **Character item:** the game's own item code then creates the item in Player
  1's hand, replacing any item Player 1 was holding.

A source stays loaded while something uses it: a hat Kirby is wearing, or an
item that still exists, even after it has been thrown. When nothing uses it, it
can be reused for the next request or replaced by another fighter. Leaving
Training or the lobby removes it. Bob-omb, Mr. Saturn and the Banana Peel are
ordinary items and need no source.

Sources can be loaded in ordinary Training with exactly one CPU, and in a
local-wireless waiting room with Lobby Practice on. Elsewhere, including online
lobbies, hats and items that need a source report that loading is not available
in this scene.

### Mii hats

For a Mii hat, the parked source is a Mii loaded with the Mii you selected for
that role in **Character Extras → Mii Brawler / Mii Swordfighter / Mii Gunner**.
Kirby then performs that class's standard copied neutral special (Shot Put,
Gale Strike or Charge Blast), following the game's normal copy rules.

## Hats, items and save states

Loading a save state gives Kirby back the hat that was saved, using the same
source loading as the shortcut. Loading a save state also gives Player 1 the
exact item that was held when the state was saved, including the turnip's face.

## Returning to the same lobby after a match

A match tears down practice. When you come back to the **same** local-wireless
lobby, the modpack reloads each role's last practice fighter (Player 1's needs
Player Swapping to stay on) and puts back the
hat that Kirby was actually wearing when the match started, for Player 1 and
the CPU separately. A hat that was knocked off before the match stays lost.
Nothing from the old practice session is reused: sources are loaded again from
scratch. A new lobby, or a new Training visit, starts fresh.

## Miis

A Mii fighter is a class (Brawler, Swordfighter or Gunner) plus a configuration:
its look, outfit, hat, colour, voice and four special moves. The modpack treats
two Miis as the same only when class, appearance and moves all match; names do
not matter.

### Your saved Miis

These are the Mii Fighters in Smash's own Mii list. The modpack reads them and
does not edit or rename them.

- Open **Character Extras → Mii Brawler** (or Swordfighter, Gunner) and choose
  a Mii separately for **Player** and **CPU**. The list shows each Mii's special
  moves, such as `[2111]`.
- Add the class's portrait to a cycle in **Characters → Character Cycles**. When
  the cycle reaches it, the fighter loads with the Mii you chose for that role.
- Player 1's original character-select Mii stays at slot 0 and is greyed in the
  Player list.
- If you change the choice for the Mii class that is currently on screen, it
  reloads when you close the menu. Other choices apply on the next cycle.
- The saved-Mii list is read automatically the first time it is needed, so the
  first shortcut works without opening the menu.

### Add New: making a Mii with the game's editor

**Add New** on a Mii class page opens Smash's own Mii Fighter editor. It starts
from a copy of your Player choice for that class (or your first saved Mii of
that class), or from the game's default when you have none. The saved Mii it
copies is not changed. **Test** works as usual.

- Saving with the editor's own Save creates a normal Smash Mii. It appears in
  Smash's Mii list outside Training and in both modpack lists.
- Saving an identical Mii again reuses the existing one instead of adding a copy.
- Leaving without saving discards the draft and creates nothing.

In an online (Quickplay) waiting room with Lobby Practice on, the class pages
offer only Add New. Name, Special Moves,
Settings and Test stay available; the Headgear, Outfit, Color, Voice and Random
choices are greyed and work in Training only. If a match
starts while you are editing, your unsaved changes are kept and the same editor
screen opens again when you return to waiting practice. Only the editor's Save
keeps a new Mii.

### Opponent Miis from your matches

With replay capture on, saving a replay on the results screen also stores the
fighters' Mii data with the capture. When you choose that opponent's capture in
Training, the CPU loads that exact Mii, with its look and moves.

- If an identical Mii is already in your Smash Mii list, that one is used.
- Otherwise it is added once to the modpack's own library
  (`sd:/ultimate/TrainingModpack/mii-presets.json`) as, for example,
  **Replay Brawler**. It then appears beside your saved Miis in the modpack
  lists, including after a restart. Choosing the same capture again does not add
  another copy.
- Discovered opponents are never added to Smash's own Mii list.

### Known limit

After a Mii swap, the small HUD portrait can keep showing the previous Mii.
The fighter's body and special moves are the swapped-in Mii.
