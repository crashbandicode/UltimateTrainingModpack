# Revision 29: the Training Modpack without a `Layout::draw` hook

Revision 29 removes the hook on the function the modpack called `nn::ui2d::Layout::draw`
(`OFFSET_DRAW`, main+0x4b620). It has no fallback mode and no feature gate. The modpack now
reaches its layouts through two inline anchors inside Smash's own UI manager. Both are
needle-located and pinned to five words, and a mismatch at either one panics with the existing
version report. The layout-archive hook at `OFFSET_LAYOUT_ARC_MALLOC` (main+0x3774704) is
unchanged.

All addresses are offsets from the 13.0.5 main module's text base. They come from the operator's
own dump (`research/game-13.0.5/main-13.0.5.elf`, which is not redistributed), read with
`llvm-objdump` and a small read-only scanner for BL/ADRP/vtable/relocation references. The 13.0.5
symbol table has no `ui2d` names, so class identities come from RTTI strings and from the
vtables that the dynamic `R_AARCH64_RELATIVE` relocations fill in.

## 1. What the removed hook was

- `0x4b620` is entry `+0x98` in the vtable of `nn::ui2d::Layout`. The vtable's address point is
  `0x4f3f280`, and its RTTI name `N2nn4ui2d6LayoutE` is at `0x4473c90`. The same entry also
  sits in `nn::ui2d::LayoutEx` (`0x4f3f568`, `N2nn4ui2d8LayoutExE`) and in Smash's layout
  subclass, whose vtable is `0x523f6f0` (no RTTI). That subclass overrides the destructors and
  `+0x10`, `+0x88` and `+0x90`, and adds its own entries after `+0xa8`. It does not override
  `+0x98`.
- On 13.0.5 the function is the layout's **Calculate**, not its draw. It builds a calculate
  context (`bl 0x59f90`), stores `this` at `DrawInfo+0xc0`, calls the root pane's `+0x98` (the
  pane calculate; `nn::ui2d::Parts` inherits it from `Pane` at `0x58700`), then calls its own
  `+0xa8`. Its arguments are `(Layout*, DrawInfo&, bool force)`; the old hook's third
  "cmd_buffer" parameter was that bool.
- No code calls it directly; the BL/B scan finds no site. Smash calls it virtually from exactly
  two places, both on a UI holder (`Layout*` loaded from `holder+0x458`):
  - `0x375efbc`, inside the UI manager's per-frame pass `0x375e260`;
  - `0x3784ae8`, inside the manager's insert `0x37849c0`, when the insert asks for an initial
    calculate.

  A third holder-calculate site, `0x378eecc`, calls `0x4ad50` non-virtually. The old hook never
  saw it, and revision 29 does not either.
- Parts layouts never pass through `+0x98`. A Parts pane is calculated as part of its parent's
  pane tree.

## 2. The game's layout registry (plan §2, option 1)

Smash wraps every UI layout in a *holder*. All holders are created by `0x3773770`, which has
hundreds of scene call sites. The same function contains the existing arc-malloc site at
`0x3774704` and builds the Smash `Layout` subclass at `0x37748c4`. Holder fields that revision 29
relies on:

| Holder field | Meaning | Evidence |
| --- | --- | --- |
| `+0x10` `char[64]`, `+0xc` length, `+0x8` FNV-1a | Holder name (copied from the string the resource accessor's `+0x50` lookup returns for tag `0x696e666f`) | `0x3774744`–`0x3774848` |
| `+0x450` | Archive resource accessor | `0x37745c4` |
| `+0x458` | The holder's `nn::ui2d::Layout*` (Smash subclass, 0xb8 bytes) | `0x37748f8`, `0x375efa8` |
| `+0x648` / `+0x64c` | Layer (< 10) / priority | `0x37849d4`, `0x3784a34` |

The UI manager is a singleton. Every manager method loads it as
`adrp x?, 0x593b000; ldr x8, [x?, #0xa90]; ldr x?, [x8]`; the global slot is main+0x593ba90 in
`.bss`. Its holder registry is a `std::vector<Holder*>` at `manager+0x0 / +0x8`, kept sorted by
`holder+0x64c` (insertion at `0x37849dc`–`0x3784a2c` through `0x3784b30`). Requests are queued
at `+0x128` for adds and `+0x160` for removals, and are applied at the start of the next manager
update. The `ssbu-dispatcher-backoff` analysis already identified the same per-frame pass
independently: "callback `0x35487f0` on worker 1 … calls vtable `0x523d418` slot `+0x10`, which
resolves to `0x375e260`; its owner at `+0x458` is an `nn::ui2d::Layout` derivative".

The per-frame pass `0x375e260` is the `run` entry of the job whose vtable is `0x523d418`. The
job is created at `0x375d418`, aggregate `0x5333150`. The main loop waits for it at `0x374b898`,
before its fighter-record step. After applying the queued removals, it walks the registry:

```
375eeec  ldp  x20, x21, [x19]          ; registry begin/end (x19 = manager)
375eefc  ldr  x23, [x20]               ; holder
375ef00  ldr  x0, [x23, #0x458]        ; layout->+0x58 (0x4b810): animate + advance frame
...      holder anim lists +0x460 (vfunc +0x40/+0x20) and +0x540 (vfunc +0x30)
375ef98  add  x8, x8, #0x2d0           ; draw-info ring slot (holder+0x638, 4 entries)
375efa4  bl   0x44cb0                  ; draw-info setup (x0 = holder+0x50)
375efa8  ldr  x0, [x23, #0x458]        ; <- revision 29 pass anchor
375efac  ldr  x8, [x0]
375efb0  ldr  x8, [x8, #0x98]
375efb4  mov  x1, x19                  ; DrawInfo = holder+0x50
375efb8  mov  w2, wzr
375efbc  blr  x8                       ; Layout::Calculate (the old hook ran here)
375efc0  ... bl 0x44d00; bl 0x44d40; ring index += 1; next holder
```

**Revision 29 reads this registry instead of hooking `Layout::draw`.** The manager-global slot is
decoded at run time from the `adrp`/`ldr` pair at main+0x375e280 (`b0010efc f9454b88` →
main+0x593ba90), which is located with a needle on the pass function's prologue; nothing assumes
a data offset. `ui::layout_registry` lists the live holders and their `Layout*` and finds them by
`Layout::layout_name`, the same field the old hook compared. Both anchors receive their holder
directly, so pane work never searches by name. The registry's lookup is used and checked at
registration: when `info_training` or `info_waiting` is registered, it logs that the
just-registered layout is found by name (`registry_hit`) and how many holders the registry
holds.

## 3. Anchors (plan §2: anchors the modpack owns)

| Anchor | Site | Thread | Holder register | Five-word pin | Locator needle (unique) |
| --- | --- | --- | --- | --- | --- |
| `ui_pass_calculate` | main+0x375efa8 in pass `0x375e260` | worker 1 (UI job), once per holder per frame | `x23` | `f9422ee0 f9400008 f9404d08 aa1303e1 2a1f03e2` | 7 words at site+4: `f9400008 f9404d08 aa1303e1 2a1f03e2 d63f0100 aa1303e0 9723974f` (one hit, 0x375efac) |
| `ui_register_calculate` | main+0x3784ad4 in insert `0x37849c0` | main thread (`0x375f020` from main loop `0x374c89c`/`0x374cabc`), once per inserted holder that asks for an initial calculate | `x21` | `f9422ea0 f9400008 f9404d08 320003e2 aa1303e1` | 7 words at site+4: `f9400008 f9404d08 320003e2 aa1303e1 d63f0100 aa1303e0 97230084` (one hit, 0x3784ad8) |
| manager global | `adrp`/`ldr` at main+0x375e280 | — | — | `b0010efc f9454b88` decode to main+0x593ba90 | 10-word prologue of `0x375e260` (one hit) |

Install sequence (`ui::anchors::install`, before the arc hook is installed):

1. Locate each anchor by its locator, which starts one word after the site. Locator-not-found
   panics through `impl_offset!` ("Failed to find offset …").
2. Read the five words at the site and classify them with the host-tested
   `training_mod_macros::ui_anchor::verify_site`. The result is `installed`,
   `site_owned_by_another_plugin` (first word differs, the other four match: another plugin
   already patched it), or `site_words_differ`. Log
   `[UiAnchor] site=main+0x… words=… result=…`, following the `install.log` pattern of
   `ssbu-dispatcher-backoff/src/runtime.rs:326–363`. Any result other than `installed` panics,
   and the panic hook appends the Smash/Atmosphere version report. There is no skip and no
   fallback.
3. Decode and log the manager-global slot (`[UiAnchor] manager_slot=main+0x593ba90`). A decode
   failure also panics.
4. `skyline::install_hooks!` both anchors. Their exported callback names carry a
   `training_modpack_` prefix: Deluxe's
   `IMGUI_DEPENDENCY.md` records a cross-NRO collision of identically named exported hook symbols.

Both sites sit immediately after a `bl` returns, and the displaced instruction is a plain,
PC-independent `ldr`. No caller-saved general or SIMD register is live there, and Skyline's
`InlineCtx` saves x0–x30 and q0–q31 anyway. The pin words are the instructions that load and
call the vtable slot, so a different game version cannot pass them silently.

**Uncontested sites.** No other known plugin touches anything in `0x375e260`–`0x375f01f` or
`0x37849c0`–`0x3784b2c` (as surveyed for revision 29; **superseded 2026-10-13**: Deluxe 0.3.1 function-hooks
the pass entry `0x375e260` for its dispatcher idle backoff, which broke the ten-word prologue needle on the
Switch — revision 30 locates the pass past that stub, see `docs/REVISION30_HOOKPROOF_PROLOGUE.md`; the two
anchor sites themselves remain untouched):

- Deluxe (`/tmp/deluxe-pin-…/repo`): sequence hooks `0x374bb04`, `0x374c600`, `0x374cd64`;
  model update `0x3ab590`; renderer `0x386b0fc`, `0x387029c`, `0x384fa10`, `0x38701e0`,
  `0x3749b78`; resolution `0x37cc130`, `0x37cc17c`, `0x37cc1b0`; native banner `0x37a28a0`.
- ssbu-dispatcher-backoff: `0x354ab48`.
- HDR (fresh read-only clone): inline `0x1b6cc08`, plus calls into `0x3776910`.
- The overlay plugin, per Deluxe's `IMGUI_DEPENDENCY.md`: ui2d panes, `0x374812c`, `0x1b6cc08`,
  `0x1345558`, `0x1d68b94`.

The Training Modpack's other hooks are fighter/module sites, plus `0x3774704`.

## 4. Why the anchors run per layout, not once per frame

The plan describes a single tick per frame that walks the registry before the game draws. The
disassembly shows that this would change behaviour. Inside the pass, each holder runs
**animate (`+0x58`) → animation lists → calculate (`+0x98`)** before the next holder starts. The
old hook ran *between* that holder's animate and its calculate, so its pane writes overrode
the animated values for that frame. Several of the writes target animated native panes:

- `damage::draw` sets `info_melee` `p1`/`p2` digit alpha to 255 and the `set_fxui_dead*` explosion
  alpha to 0, and resets the `set_dmg_num_*` animation frames;
- the HUD hide sets root-pane visibility and the `InfluencedAlpha` flag;
- `menu::draw` sets native `info_training` panes.

A tick before the loop would let the holder's own animate overwrite these writes, and a tick
after the loop would apply them one frame late. Revision 29 therefore runs the modpack's UI work
at the exact instruction before the game's virtual calculate call, once per holder. That is
the same position and the same cadence as the removed hook, and it includes the registration
calculate that the hook also saw. `ui::tick(layout)` is the former `handle_draw` body with the
`original!()` call removed. `replay_capture::after_draw()` now runs at the end of `tick`, right
after `menu::draw`, where before it ran right after `info_training`'s calculate. Nothing between
those two points reads or writes the clip editor's hold state (the calculate is game code), so
the held range-edit repeat keeps its cadence of once per `info_training` calculate.

| Former `handle_draw` work | Revision 29 |
| --- | --- |
| `lobby::update`, `named_macros::service_keyboard` at the start of every layout calculate | same, at the start of every `tick` |
| `replay_capture::service_reload` for `info_training` (native Training) or for every layout (elsewhere) | same predicate |
| waiting host: `TrModPort` readiness, `set_ui_ready`, second `lobby::update`, show/hide | same, on `info_waiting` |
| HUD hide (`info_playercursor*`, `info_melee`, `info_radar_*`) | same predicate, same writes |
| `damage::draw` | same |
| `frame_counter::tick_real`, `input_log`/`display`/`menu::draw` on `info_training` or the waiting host | same |
| `replay_capture::after_draw` after `info_training`'s calculate | end of `tick` for `info_training`, before the calculate (equivalent, see above) |

## 5. Verification and evidence

- Host (`training_mod_macros`): pins, locators and the manager prologue round-trip to the
  documented words. Each locator begins with pin words 1–4. `verify_site` covers all three
  verdicts, and the ADRP/LDR decode reproduces main+0x593ba90 and rejects non-ADRP/LDR words.
  Name lookup returns the first match in priority order and skips null names.
  - `elf_pins_match_owned_13_0_5_dump` (`#[ignore]`) reads the dump from
    `TRAINING_MOD_MAIN_ELF`. It checks that every locator occurs exactly once at the documented
    offset and that every pin matches.
- Product: `cargo skyline` clippy/builds through the research helper (§7).
- Live (primary): an F3-style reopen exercising the menu, slider, input log, damage panes, HUD
  toggle, notifications and the waiting-room host. Then the toggle row (tq6 procedure), and a
  conflict rehearsal with a test NRO that replace-hooks `0x4b620` and calls the original. The
  modpack must be unaffected, because it no longer touches that function. Expect
  `[UiAnchor] … result=installed` twice and `[UiAnchor] registered layout=info_training … registry_hit=true`.

## 6. Risks

1. **Pane timing.** The anchor sits immediately before the same virtual call, so pane writes land
   in the same frame and the same animate/calculate interval as before; no one-frame shift is
   expected. One ordering change: if another NRO replace-hooks `0x4b620`, the modpack now always
   runs *before* that hook, inside the caller. Before, the order depended on install order.
   After an animate, the other NRO sees the modpack's pane state exactly as the original calculate
   did.
2. **Coverage.** Calculate calls that do not go through a Smash holder are no longer observed. The
   static scan found none on Smash layouts (parts layouts are pane-tree calculated, and
   `0x378eecc` was never hooked), and the old work on such calls was name-gated and could not
   match. The live HUD-toggle check must still confirm that `info_playercursor*`, `info_melee`
   and `info_radar_*` are holders calculated in the pass.
3. **Waiting-room host.** `info_waiting` readiness is decided in the same place: at its
   registration calculate and on every pass. The order is unchanged: `lobby::update` →
   `set_ui_ready` → `lobby::update`. The live check must still show `[LobbyPractice] active=true`
   after the arc injection, and the host hidden when lobby practice is off.
4. **Version.** A non-13.0.5 binary or a pre-patched site panics at boot with the version report,
   where previously only the draw needle panicked. The revision 28 offsets are unchanged.
5. **Registry use.** The registry is read only from the two anchors, on the threads that own the
   vector at those moments (the pass loop does not lock it either). A holder freed while still
   listed would be the game's own bug as well.
6. **Stack.** An inline trampoline puts a full `InlineCtx` on the stack, about 0x310 bytes, before
   `tick`. `tick` therefore runs roughly 0.8 KiB deeper on worker 1 and on the main thread than
   it did as a replace hook. This has not been measured live.
7. **Text patches seen by other plugins.** The anchors patch the word at main+0x375efa8 and at
   main+0x3784ad4. A plugin that loads after the modpack and byte-searches a needle covering one
   of those words would fail its own search. None of the known plugins searches those
   functions. The old hook patched main+0x4b620 the same way.
8. **Boot cost.** Three more needle searches over the text segment, each `memmem` first and
   last (about 57 MiB), replace the old single draw-needle search: net two more.

## 7. Build record and helper lines

Source: branch `r29-draw-decoupled`, commit `31c112e` (on top of revision 28 `a4ee5d5`). The
pinned isolated toolchain is the one `check_nro.py` uses: `RUSTUP_HOME`/`CARGO_HOME` under
`/home/intpa/eden-gnw/tools`, nightly-2026-02-14, and `cargo-skyline` from
`tools/skyline-install/bin`. `CARGO_NET_OFFLINE=true`. Every command ran in its own bounded
unit: `systemd-run --user --wait --pipe --collect -p MemoryMax=3G -p MemorySwapMax=0
-p CPUQuota=200% -p RuntimeMaxSec=900 -p WorkingDirectory=<dir>`.

| Directory | Command | Result (last lines) |
| --- | --- | --- |
| `training_mod_macros` | `cargo test --offline -- --test-threads=1` | `test result: ok. 103 passed; 0 failed; 1 ignored`; doc-test `1 passed` |
| `training_mod_macros` | `TRAINING_MOD_MAIN_ELF=…/main-13.0.5.elf cargo test --offline --lib ui_anchor -- --ignored` | `test ui_anchor::tests::elf_pins_match_owned_13_0_5_dump ... ok` |
| `training_mod_macros` | `cargo clippy --offline --all-targets -- -D warnings` | `Finished dev profile`, no warnings |
| `training_mod_macros` | `cargo build --offline --no-default-features` | `Finished dev profile` |
| `training_mod_tui` | `cargo test --offline -- --test-threads=1` | 27 + 12 + 10 + 31 + 17 + 3 passed, 0 failed (unchanged crate) |
| `training_mod_consts` | `cargo test --offline -- --test-threads=1` | 16 passed, 0 failed (unchanged crate) |
| product | `cargo-skyline skyline clippy --no-default-features --features waiting-roster-reload,deluxe-sdk -- --all-targets -- -D warnings` | `Finished dev profile`, no warnings (the first attempt failed only on `non_fmt_panics` in `anchors.rs`, which the root crate's edition 2018 enforces; fixed before the commit) |
| product | same with `--features waiting-roster-reload` | `Finished dev profile`, no warnings |
| product | same with `--features waiting-roster-reload,deluxe-sdk,named-input-trace` | `Finished dev profile`, no warnings |
| product | `cargo-skyline skyline build --no-default-features` | `Finished dev profile` |
| product | `cargo-skyline skyline build --release --no-default-features --features waiting-roster-reload,deluxe-sdk` | `Finished release profile [optimized]` in 48.65 s |

The normal (deluxe-sdk) NRO is
`target/aarch64-skyline-switch/release/libtraining_modpack.nro`: **3,424,256 bytes**, SHA-256
`5561e630e8d527b525b29574ff490fa1c2a44d5a4764e5ba18b08a3beb938636`. Until the primary freezes
it, a copy with the same hash is kept outside the shared target directory, in the worker's
session scratchpad: `/tmp/claude-1000/-home-intpa-smash-training-lobby-UltimateTrainingModpack-single-file/73f855d7-8259-41ec-a053-77d7aadeb202/scratchpad/r29-builds/normal-deluxe.nro`.
The matching `.so` exports
`training_modpack_ui_pass_calculate`, `training_modpack_ui_register_calculate` and
`handle_layout_arc_malloc`, and no `handle_draw`. No trace build was made; freezing
normal/trace is left to the primary.

Lines for the research helper (`check_nro.py` was not modified). They follow `OPERATIONS28`
exactly, with the revision number changed:

```python
OPERATIONS29 = {'clippy29': 'clippy --no-default-features --features {features} -- --all-targets -- -D warnings',
                'build29': 'build --release --no-default-features --features {features}'}
# features29(trace, extra) == features28(trace, extra): waiting-roster-reload[,deluxe-sdk][,named-input-trace]
# unit: f'named-macros-nro-{operation}-' + ('trace-' if trace else '') + ('' if 'deluxe-sdk' in features else 'nodeluxe-') + str(epoch)
# frozen guard: ROOT / 'revision29-builds' / f'{mode}.nro'; receipt 'revision': 29
```

The cargo arguments those produce for the normal build and gate are:

```
build --release --no-default-features --features waiting-roster-reload,deluxe-sdk
clippy --no-default-features --features waiting-roster-reload,deluxe-sdk -- --all-targets -- -D warnings
```
