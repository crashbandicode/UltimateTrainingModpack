# Lobby Practice and shared character rosters

This independent experimental fork targets Smash Ultimate 13.0.5. It uses the native asynchronous fighter loader to replace one existing practice entry per role. It adds no CSS slots or custom fighter IDs. P1 and CPU use the same mechanism with separate settings. Physical Switch validation remains pending.

## Configure and swap characters

1. Enter ordinary Training. Press **Minus**, or hold Plus for about half a second, to open the Modpack Menu. A short Plus press keeps the native pause menu.
2. Open **Player Characters → Player Swapping** and enable P1 replacement.
3. Use **Player Character** or **CPU Characters → CPU Character** for a one-time replacement.
4. Use **Cycle Characters** in either tab to choose that role's independent ordered roster.
5. Close with **B** to commit. Fighters load without leaving practice; no matching first CSS choice or re-entry is needed.

Defaults: **P1 G&W → Palutena → Pikachu**; **CPU Pikachu → Kirby → Bowser → Bayonetta**. Existing explicit settings migrate. Hidden legacy trio fields remain for compatibility; the preloaded-three implementation is no longer the normal runtime.

Choose **All (menu order)** to cycle through the whole supported chooser catalog in displayed order. Manual selections show numbered order; deselect and reselect to move a fighter to the end. Selecting a fighter while All is active starts a custom sequence. Up/Down crosses pages. There is no eight-character limit, and at least one fighter remains selected.

Default mappings are **ZL + Right** for P1 and **ZL + Left** for CPU. Configure them under **Button Config → Cycle Player Character / Cycle CPU Character**. Holding a mapping advances only once. A fighter already in the sequence advances to its successor; otherwise cycling starts at the first choice, then wraps.

The chooser contains 80 supported stock fighter kinds. Pokémon Trainer, Pyra/Mythra, Ice Climbers, Miis and non-playable forms are excluded. These are plugin roster settings, not new game slots. Coverage is representative, not every character/costume/move combination.

## Lobby Practice and presentation

Enable **Misc Settings → Lobby Practice → On**, close the editor, and enter the waiting practice area. The editor appears as Lobby Practice. The waiting-enabled build uses the waiting-roster-reload feature; ordinary Training remains available without it.

Actual matches use the original character-select choice, regardless of P1's practice replacement. The lower waiting roster card intentionally shows that match selection. CPU replacements affect the local practice dummy. Practice shortcuts and hooks are disabled in an actual match.

The waiting CPU's small character icon is refreshed after replacement. Waiting HUD resource callbacks preload the supported stock icons before the game's normal load barrier. The refresh obtains the current generation-checked UI, maps its presentation ID to the visible partner, and calls the native texture setter. It retains no pane pointers and changes no partner or matchmaking descriptor. The icon uses the character's default-costume image.

Ordinary Training portrait and name update immediately. Native **L + R + A** reset retains the current fighter there. Save/load, recording/playback, native Training reset and some stage options remain ordinary-Training features; the waiting menu labels their scope. Native lobby dialogs take input priority.

## Save states, recording and lifecycle

Default save/load shortcuts are **ZL + Down / ZL + Up**. A save restores generic position/damage into the current fighter; it does not restore the saved fighter kind. Character-specific charge restores only for matching kinds.

Save loading, recording/playback, native UI, another character replacement and pending training actions block conflicting swaps. A replacement clears per-session training state. Requests are bound to the observed entry identity and scene epoch. Native destruction invalidates queued/active references before the original destructor runs. No manual loader pumping or forced cancellation is used.

## Installation and diagnostics

With the game stopped, replace the existing Training Modpack NRO; never load two copies. Install the required generated waiting editor archive as well:

    sd:/atmosphere/contents/01006A800016E000/romfs/skyline/plugins/libtraining_modpack.nro
    sd:/ultimate/TrainingModpack/info_waiting.arc

Preserve the existing training_modpack_menu.conf, saves, other plugins and emulator settings. The waiting archive derives from the operator's owned assets and is supplied only in the private handoff, not a source release. Native version fingerprints and the expected archive must match before activation.

Persistent logs are sd:/ultimate/TrainingModpack/practice-diagnostics.log and practice-diagnostics.previous.log. Rotation keeps two files of at most 128 KiB each. Records include build/version, scene, request/entry identity, loader phase, completion, HUD/icon refresh, destruction, save/record events and Rust panics. Preserve both files and Atmosphère crash reports after a fault, along with exact NRO hashes, game version, fighter/costume choices and last action. A native fault can prevent the final record.

## Validation

Candidate26w passed ordinary P1/CPU reload, ordered/All menus, representative moves, save/record conflicts, reset and re-entry. Both native LDN host roles versus a vanilla peer produced matching combat and results. A separate test destroyed the waiting CPU during an active Bayonetta load; references were invalidated and the actual match retained original CSS G&W.

Candidate29w adds the waiting CPU icon correction. See the versioned handoff's acceptance evidence for its runtime tests and hashes. Physical Switch repeated swaps, transitions, restarts and vanilla-peer desync checks remain required. Emulator/LDN passes do not establish Nintendo online-service or ban safety. One controller per peer is the exercised scope.

## Candidate21 hardware hardening

The native Training-menu object is no longer cached for later HUD access. Version-checked hooks copy the game's combo-display setting into an atomic boolean at the native read sites; HUD drawing consumes only that scalar. The stale-moves setting write now uses the game's byte width. This removes an inherited lifetime hazard without claiming it caused any particular earlier crash. The rest of the fighter/module/scene lifetime assumptions still require physical Switch testing.

The corrected separate G&W plugin is v0.1.1-experimental. Its earlier 0.1.0 binary has a known hardware SVC-clobber defect and must not be deployed. See the separate plugin's validation notes; the corrected release still needs physical confirmation.

## Source cleanup

The superseded lobby/trio investigation plugins remain in commit abd6ca9 and the local research archive. The normal NRO retains the five entry lifecycle and portrait hooks it needs in character_lifecycle.rs, along with persistent crash diagnostics. Scene, entry identity, destruction, ABI/version and conflicting-action checks remain in place.
