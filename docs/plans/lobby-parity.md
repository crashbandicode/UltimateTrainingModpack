# Lobby parity implementation plan

Authorized 2026-10-25: implement the original fourteen source-audited gaps on a
separate branch. The operator later appended items 15–22. Branch `lobby-parity`,
based on full HUD48 commit `76b6ff70783038c798d2408d538a61e473f71ecc`. The
physical-Switch HUD48 review is separate and unchanged by this campaign.

**Status: all 22 gameplay items accepted in the scope below.** Acceptance means
emulator observation (muted Eden, Smash Ultimate 13.0.5) or documented shared-code
coverage, as each row states. There is no physical-Switch acceptance and no
online-arena coverage. Builds, diagnostics and cleanup are not gameplay items.

## Required behavior

| ID | Feature | Scope | State |
|---|---|---|---|
| 1 | Save/load positions, damage, held items and restore settings | Quickplay and Local Wireless (LDN) waiting | **Accepted.** Quickplay restores positions, the exact item variant and nonzero damage, including in a fresh process. LDN restores the exact held item and both positions after a throw and movement (run116). An actual LDN match start retires the active operation (run140). Other LDN paths are shared-code coverage; individual timing of every queued save callback is not claimed |
| 2 | Record/play inputs, CPU mash and ledge playback slots | Both waiting modes | **Accepted.** LDN numbered recording and direct playback (run114); automatic ledge playback produces a getup attack, then idle (run116). An active 600-frame recording stops when the real match starts. After return, nothing resumes unprompted and a fresh recording plays back (run140). Mash-slot and save-state playback triggers are covered by shared-code composition, not separate runs |
| 3 | Named macros and replay-clip editor/preview | Both waiting modes | **Accepted.** Quickplay naming, interrupted-naming return and 500-frame preview. Exact-Mii LDN 500-input preview (run115). An actual LDN match interrupts naming. Returning to the same lobby restores the unsaved text and caret, and explicit Cancel saves nothing (run140) |
| 4 | L+R+A quick reset | Both waiting modes | **Accepted** (Quickplay reset; LDN return to spawn) |
| 5 | Hitbox and grab-box visualization | Both waiting modes | **Accepted.** Observed in Quickplay; LDN through the same waiting-role dispatch |
| 6 | Hidden CPU tech animations | Both waiting modes | **Accepted.** Observed in Quickplay; LDN through the same CPU-role predicate |
| 7 | Practice-stage hazards setting | Both waiting modes | **Accepted.** Quickplay WarioWare On/Off. In LDN, native rules had hazards On and waiting practice had them Off. The real match still ran microgames on both peers, so the practice setting did not leak into the match (run140) |
| 8 | Immediate stage change and per-entry rotation | Quickplay (LDN already implemented) | **Accepted** for Normal forms |
| 9 | Managed Kirby hat selection | Quickplay | **Accepted** |
| 10 | Full character-item selection | Quickplay | **Accepted** |
| 11 | Saved-Mii selection and Player/CPU swaps | Quickplay | **Accepted.** The Ready card keeps the real-match pick |
| 12 | Matching custom Mii practice HUD portraits | Quickplay | **Accepted** |
| 13 | Native Mii cosmetics interruption/resume | Quickplay | **Accepted.** Headgear, Outfit, Voice and Random confirmation resume exactly (run111). Color resumes its preview while the equipped color stays equipped, and Cancel keeps the equipped color (`8a05178`, run112) |
| 14 | Native Mii Add New/Test with exact interrupted return | LDN waiting | **Accepted.** An actual match interrupts an unfinished Specials draft. Return restores the exact picker and highlighted option. Cancel keeps only committed values, and Exit Without Saving restores the original fighter and Ready card with no new Mii (run140) |
| 15 | Normal, Ω / Final Destination and Battlefield forms; Training rotation | Training, both waiting modes | **Accepted in emulator scope.** With Training rotation On, entries went 1 → 4 → 1; Off restored ordinary Training 308, and direct Battlefield Normal gave 0 (run135). Waiting direct forms 1/3/4/104 (run140). Waiting Ω, then a normal-Battlefield real match, then waiting FD-Battlefield on return (run141). A rotated entry with a native preload already holding Training completed with the requested form (run141). No allocator, performance or physical-Switch claim |
| 16 | Green Greens Training hazards On/Off | Ordinary Training | **Checked.** On shows block stacks; Off shows none after a native reload on the same stage and fighters (run137). The reported failure did not reproduce, so no fix is claimed. Dynamic bomb/apple/wind timing was not established |
| 17 | One-time flicker when switching to a Mii | Ordinary Training | **Fixed and accepted.** Earlier runs captured a white, then purple portrait frame (run138, run141). With the targeted portrait helper, Ness ↔ Spook Gunner changes directly in both directions with no captured flash (run143). Captures are sampled at 30 FPS, not lossless. The headgear Mii shows the generic fallback portrait |
| 18 | No stray zero markers on Player/CPU badges | Training cycle chooser | **Accepted.** Correct zero/numbered badges; no stray Swordfighter zero (run130) |
| 19 | CPU original fighter as slot 0 (grey) and between cycle loops | Training CPU cycle | **Accepted.** The cycle returns to the original fighter, and the next loop starts after it (run130) |
| 20 | Load a saved replay Mii with exact headgear/equipment | CPU, ordinary Training | **Accepted for Gunner.** Exact Spook outfit, hat, color, voice and specials, finite native preview and original-fighter restore (runs130/133/134). Waiting/LDN, Player and helper headgear are not claimed |
| 21 | CPU-at-ledge / player-under-platform save/load placement | Save states (reproduced in Training) | **Fixed and accepted, C72/run145.** Native ground-contact synchronization restores the exact console fixture below Battlefield's platform; CPU ledge roll continues normally. A nearby ground save also restores both fighters correctly. Air/cliff restores and the existing wait handshake are unchanged. The earlier posture-only candidate failed and was replaced |
| 22 | Changed-headgear replay Mii loading for Brawler and Swordfighter | CPU, ordinary Training | **Accepted, run144.** Native saved Brawler Devil Horns/Fighter Uniform and Swordfighter crown/Pirate Outfit loaded with their selected equipment, P1 unchanged, and original Ness restored after each. This checks the same loading path as the replay Gunner; it is not fresh online replay capture for both classes or a Player/helper/waiting equipment claim |

Item 21 acceptance: the exact console fixture under the platform, a nearby
ground variant not under a platform, and normal cliff/ledge continuation. Traces
disproved an earlier theory that the player never reached the position-move
step, so do not add forced waits, timeouts or ground correction based on it.

## Behavior contract

- Real-match fighter selection, matchmaking, results and replay saving stay
  native. The Player Ready card shows the fighter picked for the real match;
  practice changes affect only the local practice fighters.
- A match starting takes precedence over every editor, recording, save-state
  operation and load. Pending work retires with its scene and never dereferences
  departed native objects.
- Unsaved editor state resumes in the same lobby without implicitly saving.
  Explicit cancellation still cancels. A new lobby starts fresh.
- Lobby practice rotates stages once per entry. Ordinary Training rotates only
  with **Training Stage Rotation** On (default Off), once per entry from
  character select; resets, direct changes and the Mii editor return never
  advance it. **Y Form** selects a named stage's form. See
  [Practice stage forms](../practice-stage-forms.md).

## Implementation constraints

Use the existing centralized native access, ownership and lifecycle helpers.
Broaden a backend only after its native entry/configuration shape and owner
lifetime are established; removing mode checks alone is not implementation. Use
one common context/role mapping only where it expresses a demonstrated shared
invariant. Preserve small idle costs, bounded work and teardown. Workers use
isolated worktrees and return exact changes and evidence; the coordinator owns
integration, resource limits, runtime slots and acceptance. No unrelated source
or console changes.

## Acceptance

- Retain existing passing Training/LDN results unless a changed dependency
  invalidates them; do not repeat accepted matrices.
- For each new path, name the unresolved behavior, the smallest observation and
  the stopping condition. Test the actual user action, a relevant refusal or
  interruption, and continuation after the match/return.
- Bind exact source and NRO identities. All-target Clippy has zero warnings and
  the no-default-features build passes; run focused policy/regression tests and
  the full native build with the bounded runner.
- LDN runs use a native guest request **approved on the host**, then the host's
  **L+R** start. Confirm both windows visually at the actual main menu or scene.
  A frontend room count or input receipt is not proof.
- Quickplay is confirmed against an unmodified peer, including actual contact,
  results and return.
- The resource pass covers idle CPU, repeated allocation, ownership/teardown,
  bounded queues and disk writes. Record measured before/after evidence and
  do not credit host noise as an improvement.
- Keep source implementation, emulator evidence, review and physical-Switch
  acceptance separate. Publish mode-specific docs and short visual walkthroughs
  only from verified footage.

## Known limits

- No physical-Switch acceptance or online-arena coverage.
- Shared-code coverage, not direct LDN observation: nonzero-damage restore,
  non-interrupted named saves, hit/grab boxes and hidden tech, plus the
  mash-slot/save-state playback trigger combinations.
- Native `system_data.bin` changed in several runs, including after real
  results; its writer and meaning are unknown. Dedicated Mii files and presets were unchanged in the interruption checks; run143 explicitly saved the two new equipment fixtures.
  No claim that every native save stays identical.
- Short Quickplay idle samples stayed near 60 FPS and 0.94 CPU cores with zero
  swap. They establish neither a memory leak nor an improvement.

## History in brief

- Two-peer joins failed during setup in runs113–116. Run136 omitted host
  acceptance of the guest request, a setup error rather than a defect. Successful later runs used the native host-approval sequence.
- Run121's guest editor ignored a host start. Run140 passes the actual-match interruption case with the later integrated handling; the older failure is retained as history.
- Exact-Mii preview was first refused because a CPU rank byte differed in the
  fighter descriptor; validated recapture fixed it in build60.
- Color resume needed the checkpoint-copy correction `8a05178`.

## Live handoff

Private operational ledger: `artifacts/enhancements/console-practice91/lobby-parity167/`
beside this repository. It holds per-run results, receipts, worker sessions,
blockers and exact next commands, and `TASK-STATUS.md` is the current count.
Source audit: `console-practice91/lobby-parity166/AUDIT.md`.
