# Revision 33 — startup memory (2026-10-13)

This revision removes the upstream analytics and auto-update threads, gives the
config writer an explicit 64 KiB stack, and inflates the embedded layout archives
per load, releasing them about five seconds after the game copies them.

> **Update (2026-10-24):** the `0xA5` overwrite before freeing described below
> now runs only with the development feature `archive-poison-probe`; ordinary
> builds release without it (`src/training/ui/mod.rs`).

Revision 32 made the plugin image small (2,174,976 B). This revision goes after what the plugin takes from
the game heap *after* it is loaded. The Switch standard library gives a thread spawned without an explicit
size a 2 MiB stack (`library/std/src/sys/thread/switch.rs`, `DEFAULT_MIN_STACK_SIZE`), and Skyline carves
thread stacks out of the game heap, so two permanent threads cost twice the image.

## What changes

| item | before | after |
|---|---|---|
| upstream analytics thread (`events_loop`, Firebase POST every 10 s, device/user ids) | permanent, 2 MiB stack | removed with `common/events.rs` |
| upstream version check (`release.rs`, GitHub releases of `jugeeya/UltimateTrainingModpack` at every console boot, update dialog, zip install) | 512 KiB thread + JSON at boot, could offer to install upstream over this fork | removed with `common/release.rs`, `common/dialog.rs`, the `Auto-Update` menu row and the `update_policy` field |
| config writer thread (`common/filesystem.rs`) | permanent, 2 MiB default stack | permanent, explicit 64 KiB stack |
| embedded Training layout archive | inflated once on the first Training load, 4,213,268 B resident for the session | inflated per load into a parked buffer, released after the game copied it |
| embedded waiting-room archive | a second ~4.2 MB resident copy after the first waiting-room use | built per load from the inflated Training archive and the patch; both inputs dropped at once, the result parked and released |
| dependencies | `minreq`, `native-tls` (+ patch), `zip`, `byte-unit`, `skyline-web` | gone |

`SMASH_VERSION` moved to `common/mod.rs` (the panic report and macro metadata still use it). Menu JSON files
that still carry `update_policy` load fine: unknown fields are ignored.

## The archive lifetime

The layout hook sits at the game's own allocation of the archive buffer: the game allocates the size the
hook reports and then copies from the pointer the hook reports, so the plugin's inflated archive is only
read during that copy. `training::ui` now parks each inflated archive with a timestamp (`hand_out`) and the
heartbeat thread releases parked archives older than five seconds (`release_stale_archives`). Before a
buffer is freed it is overwritten with `0xA5`, so if anything still read it the HUD would break visibly
instead of silently depending on freed memory. Lines: `[LayoutArchive] handed out info_training bytes=…`,
`[LayoutArchive] released info_training bytes=… after …ms`. Inflating the 4.2 MB archive costs a few tens
of milliseconds on a loading screen; the archives stay DEFLATE-compressed in the image (about 195 KB).

## Verification

- Host: `training_mod_consts` tests/clippy/no-default build (the menu lost a field), tooling tests.
- Pinned gate: `check_nro.py clippy33` / `build33 --features deluxe-sdk`; frozen `revision33-builds/normal.*`.
- Eden Training-mode run before the card: enter Training (HUD drawn, heartbeat cadence unchanged), wait
  past the release line, keep playing, leave and re-enter Training (a second `handed out` / `released`
  pair), then the waiting room if reachable. Results are recorded in the research handoff and the smoke notes.

## What to watch on the console

- Boot: no `Performing version check` dialog, no network activity from the plugin.
- `[LayoutArchive] released …` about five seconds after a Training load, with the HUD still intact.
- Rollback: revision 32 (`65ed74ab…`, 2,174,976 B) in the card's `handoff-r33-20261013/backup/` and in
  `artifacts/switch-before-r33/`.
