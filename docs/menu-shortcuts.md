# Opening the Training Modpack menu

**B + D-pad Up is always available.** Hold either button and press the other,
or press both together. This permanent shortcut cannot be disabled by configuration.

Two additional shortcuts are configured in:

```text
sd:/ultimate/TrainingModpack/button_config.ini
```

The file is created at startup if absent, with these defaults:

```ini
[menu]
shortcut_1=Minus
shortcut_2=disabled
```

For example, to replace Minus with a chord and enable a second shortcut:

```ini
[menu]
shortcut_1=ZL + X
shortcut_2=RStick
```

Use `disabled` or `none` to disable either additional slot. Empty values are
invalid, rather than another spelling of disabled. The permanent shortcut still
works when both slots are disabled. Additional shortcuts do not depend on the
legacy **Menu Open Start Press** setting.

## Supported buttons

Names are case-insensitive. Use one or more names joined by `+`:

| Names | Meaning |
| --- | --- |
| `A`, `B`, `X`, `Y` | Face buttons |
| `L`, `R`, `ZL`, `ZR` | Existing controller-aware Training Modpack trigger mapping |
| `DPad_Up`, `DPad_Down`, `DPad_Left`, `DPad_Right` | D-pad directions; `DPad Up`, etc. also work |
| `Plus`, `Minus` | Plus/Start and Minus |
| `LStick`, `RStick` | Stick clicks; `Left Stick Press` and `Right Stick Press` also work |

The existing trigger display labels also work (for example, `Pro ZL` or `GCC L`).
On a GameCube controller, logical `R` means Z, `ZL` means L (including its digital
click), and `ZR` means R (including its digital click). Logical `L` and Minus have
no GameCube mapping; the permanent shortcut remains available.

All buttons in a menu shortcut must be down, and at least one must have just been
pressed. Holding a shortcut does not repeatedly reopen the menu. Extra unrelated
buttons do not disqualify a chord. Menu opening is blocked while the vanilla pause
menu, mod menu, named-macro panel or replay-loading state owns input.

## Plus and other shortcuts

Plus/Start goes directly to normal game pause. Holding it no longer implicitly
opens the mod menu, and releasing it no longer synthesizes a delayed pause press.
An explicit `Plus` INI shortcut can intentionally assign it to the mod menu;
an accepted menu-opening request consumes that pause input.
Modal macro/replay screens retain their existing pause-input suppression.

Save/load state, input record/playback, and character-cycle button settings keep
their existing configuration, dispatch and request-consumption behavior.

## Loading and invalid input

Configuration is loaded once during initialization; **restart the game after an
edit**. The input hook uses cached fixed-size button data and never reads the file
or constructs button vectors. The entire file is limited to 8 KiB. Missing or
unreadable files, invalid UTF-8 and oversized files use the defaults. Existing
files are never rewritten to repair invalid settings.

Unknown keys and sections are ignored. `#` and `;` start comments. A malformed
recognized value (including an unknown button, repeated button, empty value or
empty `+` component) falls back to that slot's default. Duplicate recognized keys
also fall back to that slot's default; another valid slot is retained. Thus an
invalid first slot restores Minus, while an invalid second slot stays disabled.

## Verification

Passing host tests in `training_mod_consts/src/menu_shortcuts.rs` cover defaults,
supported names, disabled and malformed slots, the permanent fallback, press
edges, and explicit versus implicit Plus. Native controller tests in
`src/common/button_config.rs` were compile checked, not executed on the host.
Native all-target Clippy, no-default-features build and release build passed.
Eden verified missing-file creation, loading an edited X shortcut after a normal
game restart, removal of Minus as an opener, and the permanent B + D-pad Up
fallback. The default Minus opener and normal Plus pause also passed. Physical
console acceptance remains pending.
