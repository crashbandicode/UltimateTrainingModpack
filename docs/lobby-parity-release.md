# Lobby parity candidate C89 — unstable

**C90 update:** stage-form labels now show only the selected Normal, Omega or Battlefield form. The full build retains C89’s features; this update does not add mid-animation replay starts.

The menu tests pass (176 tests across seven harnesses), along with warning-free Clippy and the no-default-features build. Yoga verified all three labels and a live Peach’s Castle Battlefield-form change with the current companion plugins. Console review is pending.

C89 builds on C72's waiting-room practice (save/load, recordings, macro previews
and interrupted Mii editing in Quickplay and Local Wireless waiting) and adds the
changes below. **Training and waiting-room emulator checks passed with the
current console plugins. Console acceptance is pending.** C89 is not yet a
published download; the earlier [enhancements alpha](experimental-release.md)
does not include these changes.

## New in C89

- **Miis:** Player, CPU and HUD portraits share one rule for which saved Mii
  loads. Miis with changed headgear or outfits load for the Player too, and
  switching between two Miis of the same class no longer reports
  *already active*. [Mii guide](mii-fighters.md)
- **Waiting-room CPU Apply:** a Mii chosen for the waiting-room CPU and applied
  after a pause is no longer refused just because the game changed the CPU's
  rank value meanwhile. Other protected configuration changes still refuse safely.
- **HUD portraits:** equipped Miis get their own portrait instead of the generic
  class one, and the stage no longer blacks out while the portrait renders.
  [Portraits](mii-fighters.md#matching-portraits)
- **Setup Library:** name a save-state slot, share the file, and bind it to any
  of the five slots. The list reads names only; a file loads when you pick it.
  [Setup Library](setup-library.md)
- **Record Slots:** copy a named recording into a numbered slot, or save a slot
  to the named library. [How](named-macros.md#share-a-numbered-record-slot)
- **Replay preview:** a refused preview explains why in the wide help strip, and
  **ZL+Y** moves a mid-action crop to a clean lead-in. [Preview](named-macros.md#preview)
- **Stage forms:** Normal, Omega and Battlefield are separate **Y** choices, and
  the help line names them. [Pick a form](practice-stages.md#pick-a-form)
- **Item icons:** each of the 38 character items shows an icon taken from its
  own game assets, using 35 shared textures. [Character items](character-items.md)

## Build identity

| Field | Value |
| --- | --- |
| Smash version | 13.0.5 |
| Source commit | `c61fdb0` |
| Full NRO size | 4,231,168 bytes |
| NRO SHA-256 | `def8782fae866857fb174c19104cadc83086b270e3b828a9f2bc2b91caeafcbb` |
| Compiled source content SHA-256 | `7e50f98b52897bf1298b3842ca89461984f99b4312206b2bd979179fc3be8f32` |

## Validation

- **Build and tests:** 16 build/validation jobs passed, including full and base
  Clippy with zero warnings, the no-default-features build and the full release.
  All 707 host tests ran fresh across 21 harnesses (263 Mii, 113 macro, 176 menu,
  155 constants).
- **Emulator, current console plugins:** all 12 companion plugins from the
  current Switch plus C89; all 13 initialized.
  - Training: Player 1 went Mr. Game & Watch → armored replay Gunner → default
    Gunner → armored replay Gunner against Duck Hunt. Body and HUD portrait
    matched each time; no black stage in 887 frames.
  - Local Wireless waiting: delayed CPU Mii selection in both directions,
    matching portraits, no black stage in 581 frames.
  - Normal exits from both modes.
- **Earlier candidates:** replay preview and Peach's Castle forms (C77), Setup
  Library and Record Slot sharing (a C82-based build), all item icons in Training
  and Local Wireless waiting (C84), and an equipped waiting-room CPU Mii with
  original-fighter restore (C86).
- **Limits:** emulator checks only, not an Atmosphère/sysmodule or console boot
  qualification. Captures are sampled at 30 FPS, not lossless; the final five
  recordings dropped 43 frames.

## Console review

Back up the installed NRO, then replace only `libtraining_modpack.nro` in
`atmosphere/contents/01006A800016E000/romfs/skyline/plugins/`. Keep dependencies,
other mods, settings and saves. Do not install a second Training Modpack NRO.

1. **Mii HUD:** enter Training as Mr. Game & Watch against Duck Hunt. Switch
   Player 1 to the default Mii Gunner, then the equipped replay Gunner, then
   back. Each time check the body and HUD portrait, no *already active* notice,
   and no flicker or black stage.
2. **CPU Mii:** give the CPU the equipped replay Gunner, then restore the
   original fighter.
3. **Setup:** [save a setup](setup-library.md) to the library, check its file
   in `setups/`, press **Y** to bind it to another slot, move, then load that slot.
4. **CPU recording:** record a short CPU action,
   [save the slot](named-macros.md#share-a-numbered-record-slot) to the library,
   bind it to another Record Slot and play it.
5. **Stage forms:** on Peach's Castle, press **Y** through Normal, Omega and
   Battlefield and apply each with **A**.
6. **Replay crop:** open the crop you reported and press **X**. If it shows
   *Needs ZL+Y lead-in*, press **ZL+Y**, then **X**
   ([Preview](named-macros.md#preview)).
7. **Waiting room:** choose a Mii for the CPU, wait with the menu open, then
   close it so the change applies; change back the same way. Finish a match and
   return: practice resumes and your real-match fighter is unchanged.

Kirby hats, character items, Elite Stages and counterpicks need no repeat; they
passed earlier reported console tests on C72.

Installed on the console SD card on October 27, 2026, with checksum readback;
the other installed plugins, sysmodules and inventoried settings were unchanged.
Physical console review of C89 is still pending.

Known limits:
Online arenas are unverified. Earlier intermittent startup faults remain
unresolved. Broad tracing is off; keep the small practice logs and
Atmosphère report if a console fault occurs.
