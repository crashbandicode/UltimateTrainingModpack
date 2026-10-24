# Solo Popo

Practice as Popo without Nana, or give the CPU the same setup. This feature is
in the enhancements development build; it is not in the published 7.1.2 package.

If you chose Ice Climbers on the game's character-select screen, open
**Character Extras → Solo Popo → On**, then close the modpack menu. The change
applies immediately. Choose Off to bring Nana back.

![Enabling Solo Popo for the original Player 1 Ice Climbers in Training.](fighter-hotswap/solo-popo.gif)

*Recorded in Linux Eden Training. Kirby is the CPU.*

For Ice Climbers in a character cycle, open **Characters → Character Cycles**,
highlight Ice Climbers, and press **Y**. **ZR** switches which role you edit.

| Setting | Controls |
| --- | --- |
| Character Extras → Solo Popo | P1 who originally selected Ice Climbers |
| Red S, bottom-left of the portrait | P1 who cycles into Ice Climbers |
| Blue S, bottom-right of the portrait | CPU Ice Climbers, originally selected or cycled in |

These settings save independently. An S appears only while its setting is on.
The original P1 portrait stays grey as slot 0, but you can still toggle its red
S for a future session. That red S does not change the current original P1 or
the Character Extras setting.

The applicable setting switches between Popo alone and the pair when the menu
closes. Player cycling returns through the original slot 0 after the last
numbered fighter. See the [hot-swap guide](fighter-hotswap.md) for the controls
and the [development checks](character-cycles-validation.md) for test coverage.
