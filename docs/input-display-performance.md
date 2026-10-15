# Input display performance — 7.1.2

This patch reduces the work the input display performs during Training and lobby
practice. It keeps the input history, icons, colours, frame counts and fade timing.

Previously, every visible update searched the entire layout for all 15 rows,
hid all 185 icons in the newest row, rebuilt its icon list, and rewrote both text
boxes. This also happened beneath the modpack and vanilla menus. With status
labels enabled, it resolved the fighter's status name again on every update.

The renderer now remembers values written to each physical row. It searches
within the input-display subtree, changes only affected icons and text, and
skips content work while hidden. Closing a menu refreshes the retained history
using the selected display mode. Layout registration clears the cache, including
when the game reuses an address. No game-owned pane pointers are retained.

## Validation

The host comparison compiles the original and modified renderer against the same
instrumented pane tree. It measures lookup calls/visits, writes and allocations,
and checks rendered state. These are renderer measurements, not Switch FPS.

| Update | Pane lookups before → after | Text writes before → after | Rust allocations before → after |
| --- | ---: | ---: | ---: |
| Unchanged input | 212 → 1 | 2 → 0 | 25 → 0 |
| Held input, frame count advancing | 210 → 3 | 2 → 1 | 24 → 1 |
| Rapid changing inputs, populated ring | 214 → 19–24 | 2 → 0–1 | 26 → 1–2 |
| Hidden by the mod menu | 210 → 1 | 2 → 0 | 26 → 0 |

The 2,897-pane host fixture contains the input display but omits unrelated game
layouts. Rust allocation counts exclude the native lookup wrapper's temporary
C strings and use fixture status names. The benchmark checks repeated frames,
32 consecutive input changes crossing the ring twice, all display modes, status
toggles, icon colour changes/removal, the 99-frame cap, menus, fading, clearing
history and same-address layout recreation. Ordinary visible state matches the
original; recreation now restores retained history rather than leaving default
historical panes. The missing-invalidation control fails as expected.

Restoring all 15 retained rows after layout recreation costs more on that one
update (2,912 lookups versus 212 in the old renderer, which only rebuilt the
newest row). It removes that work from subsequent unchanged frames.

See the [recorded comparison](input-display-benchmark.md) and
[reproduction instructions](../scripts/input-display-bench/README.md).

The release also passed strict Skyline Clippy with all targets and warnings
denied, a build with default features disabled, and an optimised release build
with `waiting-roster-reload,deluxe-sdk`. Both compared renderers use the same
compiler optimisation settings; the release keeps the existing size profile.

In muted Eden Training, live checks covered Smash inputs, Raw inputs, Status-only,
Off, status labels with Smash and Raw inputs, menu hiding/reopening, and expiry of
the history. These checks used the same renderer code and build features as the
release; the package version was corrected from 7.2.0 to 7.1.2 afterward.

The reported LessLag drops motivated this change. The redundant work is verified;
an improvement in physical-Switch FPS with LessLag has not been measured.
