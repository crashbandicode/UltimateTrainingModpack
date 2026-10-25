# Lobby parity implementation plan

Authorized 2026-10-25: implement all fourteen source-audited gaps on a separate branch. Branch `lobby-parity`, based on full HUD48 commit `76b6ff70783038c798d2408d538a61e473f71ecc`. Current Switch HUD48 review remains separate from this campaign.

## Required behavior

| ID | Feature | Required new scope | State |
|---|---|---|---|
| 1 | Save/load practice states including positions, damage, held items and associated restore settings | Quickplay and Local Wireless waiting | Implemented; Quickplay positions/item restore observed; damage and fresh-process check pending |
| 2 | Record/play inputs, CPU mash and ledge playback slots | Both waiting modes | Implemented; Quickplay CPU recording/playback observed; trigger and LDN checks pending |
| 3 | Named macro library/editor and replay-clip editing/preview | Both waiting modes | Implemented; Quickplay named save and interrupted naming return observed; editor/clip preview pending |
| 4 | Training-style L+R+A quick reset | Both waiting modes | Implemented; Quickplay reset observed |
| 5 | Hitbox and grab-box visualization | Both waiting modes | Implemented; visual runtime check pending |
| 6 | Hidden CPU tech animations | Both waiting modes | Implemented; applicable-tech runtime check pending |
| 7 | Practice-stage hazards setting | Both waiting modes | Implemented; active hazards runtime check pending |
| 8 | Immediate stage change and per-entry stage rotation | Quickplay (LDN already implemented) | Implemented; direct Quickplay change observed; rotation/return qualification pending |
| 9 | Full managed Kirby hat selection | Quickplay | Implemented; Quickplay managed Mario/Link hats observed |
| 10 | Full character-item selection | Quickplay | Implemented; Quickplay absent-source Stitch-Face grant and save/load observed |
| 11 | Saved-Mii configuration selection and Player/CPU swaps | Quickplay | Implemented; Quickplay saved Brawler/Swordfighter swap observed; special validation pending |
| 12 | Matching custom Mii practice HUD portraits | Quickplay | Implemented; Quickplay CPU portrait observed with unchanged actual CSS Ready card |
| 13 | Native Mii cosmetics and exact interruption/resume | Quickplay; supersedes former Training-only restriction | Implemented including Voice eligibility and native Random coroutine follow-up; cosmetic runtime pending |
| 14 | Native Mii Add New/Test with return to practice | Local Wireless waiting | Implemented with measured LDN owner; runtime pending |

Real-match fighter selection, matchmaking, results and replay-save behavior must remain native. The Player Ready card continues to identify the fighter selected for the real match. Practice changes affect the local practice fighters. A match beginning takes precedence over every editor, recording, save-state operation and load; pending work must retire with its scene and never dereference departed native objects. Resume unsaved editor state in the same lobby without implicitly saving. Explicit cancellation still cancels. Ordinary Training retains its existing behavior. Stage rotation remains exclusive to lobby practice; Training supports direct changes only.

Use existing centralized native access, ownership and lifecycle helpers. Broaden a backend only after its native entry/configuration shape and owner lifetime are established; removing mode checks alone is not implementation. Use one common context/role mapping where it expresses a demonstrated shared invariant. Preserve small idle costs, bounded work and teardown. Online arenas are a separate qualification scope, not assumed covered by Quickplay.

## Work streams

1. Core practice tools (1–7): native Training dependencies, CPU role addressing, interruption-safe save/record/editor/reset paths, visualization and hazards.
2. Quickplay character resources (9–12): qualify native backend and resource/helper lifecycle, saved Mii selection and matching CPU portrait; preserve native Player Ready card.
3. Native Mii editor (13–14): full cosmetics interruption snapshots and LDN editor entry/return.
4. Coordinator (8 and integration): Quickplay stage preloading/live replacement, shared boundaries, review, builds and runtime qualification.

Workers use isolated worktrees and return exact changes/evidence. The coordinator owns integration, aggregate resource limits, runtime slots and acceptance. No worker may run unbounded compiler/emulator jobs. No unrelated source or console changes.

## Acceptance

- Retain existing passing Training/LDN matrices unless a changed dependency invalidates them.
- For each new path name the unresolved behavior, smallest observation and stopping condition. Test the actual user action, a relevant refusal/interruption, and continuation after match/return; bundle compatible actions in running sessions.
- Use exact compiled source and NRO identities. Required all-target Clippy has zero warnings, and the no-default-features build must pass. Run focused pure-policy/regression tests and the full native build using the existing bounded runner.
- Confirm Quickplay with an unmodified peer; include actual contact, results, rematch/return. Native editor drafts retain the exact unfinished screen and values. Saved practice state restoration and macro playback use the correct waiting CPU rather than assuming native Training entry 1.
- Resource pass covers idle CPU, repeated allocation, ownership/teardown, bounded queues and disk writes. Record measured before/after evidence without claiming improvements from unrelated host noise.
- Keep source implementation, emulator evidence, review and physical-Switch acceptance separate. Publish accurate mode-specific docs and concise visual walkthroughs after functionality is verified.

## Live handoff

Private operational ledger: `artifacts/enhancements/console-practice91/lobby-parity167/` beside this repository, including worker sessions, test receipts, live host claims, blockers and exact next commands. Source audit: `console-practice91/lobby-parity166/AUDIT.md`.
