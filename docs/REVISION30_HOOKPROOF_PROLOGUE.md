# Revision 30 — hook-proof UI-manager locator (2026-10-13)

Deluxe 0.3.1 function-hooks the UI pass entry, which broke
revision 29's prologue needle at boot on the Switch. The locator now starts
0x14 bytes into the function, past any 16-byte hook stub, and the entry words
are reported rather than required. The two anchor pins are unchanged.

Revision 29 (`docs/REVISION29_DRAW_DECOUPLING.md`) drives the modpack UI from two inline anchors inside
Smash 13.0.5's UI manager and reads the manager global from the `adrp`/`ldr` pair at main+0x375e280. It
found that pair by searching for the ten-word prologue of the per-frame UI pass `0x375e260`, a needle
anchored at the function's FIRST instruction.

## What failed on the Switch

The operator's first hardware smoke of revision 29 panicked at boot with
`Failed to find offset for UI_MANAGER_PROLOGUE` (`src/common/offsets.rs:168`) while reporting Smash
13.0.5. The console's `practice-diagnostics.log` showed the game pinned (`main 21450c64…`,
`verified_13_0_5=true`) and both anchor sites `result=installed` with the Eden words; only the prologue
search failed.

Cause: the Deluxe build on the console (`libssbu_online_deluxe_rewrite.nro` 0.3.1,
`0.3.1-00660a3d63cc-1791859184`, SHA-256 `9511e8c0…e840`) function-hooks `0x375e260` for its dispatcher
idle backoff (its `install.log` reads the five prologue words and acts on the site; the binary carries
six references to the address). Skyline runs Deluxe's `main` before ours and its hooks are 16-byte
stubs (`ldr x17, #8; br x17; .quad target`), so the first four words of the prologue were gone before
the modpack searched for them. The Deluxe v0.3.0 host used in the Eden qualification has no reference
to that address (its backoff hook was at `0x354ab48`), which is why revision 29 passed there. The
revision 29 design's "uncontested sites" statement is therefore wrong for Deluxe ≥ 0.3.1 at the pass
entry; the two anchor sites themselves (`0x375efa8`, `0x3784ad4`) are still untouched.

## Change

- `training_mod_macros::ui_anchor::UI_MANAGER_LOCATOR` (host-tested): six words starting
  `UI_MANAGER_LOCATOR_DELTA = 0x14` bytes into the prologue — `stp x20,x19 | stp x29,x30 | add x29,sp,#0x90 |
  adrp x28,0x593b000 | ldr x8,[x28,#0xa90] | ldr x9,[x8]`. It starts past the 16 bytes a function hook
  rewrites and has exactly one hit in the 13.0.5 text (owned-dump test `elf_pins_match_owned_13_0_5_dump`).
- `src/common/offsets.rs`: `OFFSET_UI_MANAGER_LOCATOR` is searched; `OFFSET_UI_MANAGER_PROLOGUE` is
  derived (locator − 0x14). The ten-word `UI_MANAGER_PROLOGUE` remains for the pin report only.
- `src/training/ui/anchors.rs::install`: logs
  `[UiAnchor] ui_manager_prologue site=main+0x375e260 words=… result=installed|site_owned_by_another_plugin|site_words_differ`
  and continues; the modpack needs only the `adrp`/`ldr` pair, which still must decode (panic otherwise).
  On the operator's console the expected report is `site_words_differ` (Deluxe's four-word stub).

Nothing else changes: the two anchor pins still panic on any mismatch, there is still no draw hook and
no fallback.

## Qualification

Host tests (`training_mod_macros`, including the owned-dump test), pinned strict NRO Clippy and release
(`check_nro.py clippy30|build30 --features deluxe-sdk --offline`), freeze (`revision30-builds/`), an Eden
boot check with the console's Deluxe 0.3.1 staged beside the candidate, then the Switch install over
USB MTP. Records: `research/named-macros/revision30-*.md`, `artifacts/switch-r29-smoke-1/DIAGNOSIS.md`.
