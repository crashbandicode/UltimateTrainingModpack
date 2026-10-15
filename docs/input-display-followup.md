# Input display follow-up

7.1.2 removed repeated full-tree work on a held input. A Smash-input change still advanced the ring, and each of the 15 rows was found with `find_pane_by_name_recursive` from `TrModInputLog`. On the host fixture that restarts a depth-first walk, so one change examined about 20,295 row panes before icon work.

The benchmark confirms this repeated work. Its contribution to the reported LessLag frame drops remains a hypothesis: host pane counts are not Switch or emulator FPS measurements.

## This change

`draw` walks `Pane::children_list` once when any row needs a pane. Pinned nnsdk `d23f131` (`Pane` in `skyline` `6b52d32`) stores that sentinel and each child's `link`. It has no child iterator. `skyline_smash` `01bf583` also has `find_pane_by_name(..., recursive)`, which is still one native search per name.

The 15 pointers stay in a local array and die when `draw` returns. `RenderCache` still stores values and the root address, not pane pointers. A missing row still panics with `Could not find pane {name}`. Rows are direct children of `TrModInputLog`; a row nested deeper would now miss.

Unchanged rows do not walk. Icon, frame, and status lookups inside a dirty row are unchanged.

## Host bench

`scripts/input-display-bench/run.ps1` compares the frozen 7.1.2 renderer with the worktree. It adds `ttl_decay` (three real ttl decrements), `ring_rotate` (three ring advances), and `late_head` (a frame tick on `TrModInputLog14`).

The completed host run used the same 2,897-pane fixture for both renderers:

| Update | Pane visits before | Pane visits after |
| --- | ---: | ---: |
| History rotation | 20,314 | 34 |
| Rapid input change | 20,382–20,469 | 102–189 |
| Three successive fade frames | 11,602 / 11,988 / 9,284 | 19 / 19 / 18 |
| Frame count on the last row | 2,708 | 19 |
| Unchanged held input | 2 | 2 |

Visible pane states matched for input changes, Smash/Raw/Status modes, status toggles, hide/reopen, reset, and fades. Both renderers restored history after layout re-registration; deliberately omitting invalidation failed the control. The rapid-input workload of 120 frames took about 27.0 ms before and 0.216 ms after on the host. That timing covers the synthetic workload, not game frame time; tiny unchanged-state timings varied between runs.

The fixture does not include all surrounding game HUD panes. Root lookup and icon construction still perform work, and native allocations are not counted. No physical-Switch FPS improvement is established by these measurements.

The candidate passed all-target Clippy with warnings denied, the release build, and the no-default-features build. In a fresh Eden Training session, Smash Inputs with status text filled and scrolled the history, then faded and expired. Raw Inputs, Status Only, and None were selected through the live menu and visually checked. This is runtime rendering evidence, not a controlled LessLag FPS comparison.

```powershell
/usr/bin/pwsh -NoLogo -NoProfile -NonInteractive -Command '& ./scripts/input-display-bench/run.ps1'
```

Run that from the repository root. Output is `/tmp/input-display-bench/report.md`. The numbers are host visits and CPU time, not FPS.
