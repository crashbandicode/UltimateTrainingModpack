# Independent source audit — revision27 naming-generation fence

2026-10-12 ~06:22UTC. MEMENTO-CTRL-SOURCE-AUDIT-ONLY replacement for OpenCode
`ses_ed151b2f1ffe2KMLGuLIHcC0iT` (initial and exact-session resume both failed
`OpenCode exited with code 1`; no report from it). Parent: `ses_eda4c5e1affe9opq1xJdL8HYIt`.

Native Claude Code session `8dbb82fb-2068-4d96-8e81-c249078ec655`, owner PID 3568838.
Verified live pair from the exact session transcript (assistant `message.model`/`effort`)
and `CLAUDE_CODE_EFFORT_LEVEL`: `claude-opus-5-5` / `xhigh` (matches launch pair).
Watcher PID 3570074 (owner-bound, model/effort only, no WARN); reaped before return.

Scope: read-only source inspection only. No product edits, builds, tests, ELF jobs,
game/profile/device changes, or subdelegation. Runtime/artifact verification is parent-owned
and was not duplicated.

## Scope and identity checks

- `git diff --no-index --stat revision26-builds/source revision27-builds/source`: only
  `src/training/named_macros.rs` (+6/−2). Three hunks: the new static plus comment (r27 L26–28),
  `request_native_name` capture (L154), `service_keyboard` comparison (L175), and
  `clear_session` increment (L514).
- Frozen r27 `*.rs`/`*.toml` (114 files) hash-identical to
  `UltimateTrainingModpack-single-file`; no product `src/**/*.rs` is missing from the frozen tree.
  The only tree differences are product-only static assets (`src/static/img/*.svg`, `layout.arc`,
  `waiting_ui.patch`), which are not in the frozen source copy.

## Verdict

The fix is correct and minimal for the stated root cause. I found no blocking defect.
It keeps real scene and character cancellation. It no longer stales a valid same-session export
across the preview stop/restore. It does not let an obsolete name apply to a different draft.
Line numbers are revision27 `src/training/named_macros.rs` unless another file is named.

## Proof chain

### 1. Root cause (r26) is exactly reproduced by source

- `replay_capture.rs:345-349`: `ClipEffect::Export` calls `replay_preview::request_stop()` and then
  `offer_replay_recording` → `request_native_name` (L776). r26 stored
  `NameJob.generation = GENERATION`.
- `replay_preview.rs:157-165`: the CPU `before_frame` takes `Request::Stop` while the preview is
  occupied. It calls `save_states::request_preview_restore()` and prints `[ClipPreview] restore_stop`.
- `save_states.rs:553-562`: CPU `KillPlayer`→`WaitForAlive` calls `named_macros::begin_restore()`.
  That is the only caller. `begin_restore` advances `GENERATION` (L528).
- `native_keyboard.rs:174-187` / `ui/mod.rs:63-66`: the applet runs synchronously in the draw hook.
  `service_keyboard` then polls in the same call (L165-166) and logs `consume` (L174) *before*
  the generation guard. In r26 the guard compared the advanced `GENERATION` and returned silently,
  so no `applied` line appeared. This matches the r26 native log: `restore_stop` → `returned 0x0` →
  `consume id=1 replay=true` → no `applied`, and the draft was retained.

### 2. Valid same-session export is no longer falsely staled

- `SESSION_GENERATION` has exactly three references: L28 (decl), L154 (capture), L175 (compare).
  It has exactly one writer: `clear_session` (L514). `begin_restore` (L524-541) does not touch it.
- Preview teardown after export uses `replay_preview::clear_session` (`replay_preview.rs:67-72`,
  called at L85/116/124/133/181/210). That is a different function and it does not touch
  `NAME_JOB` or either named-macro epoch. This is essential: `restored_idle` (L209-211) runs
  during/after the export stop.
- Therefore the preview-stop restore can no longer invalidate the job. The same
  `id` / `is_native_training_mode()` guards (L169-177) still apply.

### 3. Real scene/character cancellation is preserved

`named_macros::clear_session` (L512-522) still advances both `GENERATION` and
`SESSION_GENERATION`, **takes `NAME_JOB`** (L515), and closes the Save screen
(`close_transient`, `macro_panel.rs:504-513`). Every path into it is unchanged:

- `character_roster.rs:622`: the native character install calls `training::clear_session_state()`
  (`training/mod.rs:61-84`, L79).
- `lobby.rs:143-146`: a lobby-practice active toggle calls `clear_session_state()`.
- `lobby.rs:116-124`: a scene-key change with the preview occupied calls
  `named_macros::clear_session()` directly.

After `take()`, a late completion finds no matching job (L169-173) and is discarded without
retiring a newer job. `NAME_SERIAL` is a monotonic u64 (L153), and `native_keyboard::finish`
only accepts the running id (`native_keyboard.rs:150-157`).
A mode departure without `clear_session` is still rejected by `is_native_training_mode()` (L176).

### 4. No obsolete naming is accepted

- The name applies only to the current `Screen::Save` draft. `confirm_native_name` returns
  `Effect::None` otherwise (`macro_panel.rs:199-203`). `finish_save` (`macro_panel.rs:690-711`)
  reads only draft fields (name/version/start/frames/placement). It has no anchor, motion,
  `GENERATION`, or restore dependency, so the dropped motion fence protected no naming input.
- A restore inside the pending window cannot replace the draft:
  - The preview restore forces `RESTORE_ACTION=0` while occupied (L530-531), and
    `service_restore` returns early (L590-595). No `start_named_lockout`/`begin_capture` follows.
  - User/auto save-state loads are gated on `!panel_visible() && !replay_preview::occupied()`
    (`save_states.rs:521-525`). The Save screen makes `is_open()` true.
  - `request_playback` needs `playback_allowed()`, which requires `!is_open()`
    (`macro_panel.rs:155-157`).
  - A new draft needs `finish_capture`→`offer_save` (L397-410), which needs a completed capture.
    That cannot happen while the synchronous applet blocks the game thread.
- `GENERATION`'s only other consumer, `restored_target` (L743-754 → `macro_panel.rs:385-397`), is
  unaffected: both of its increments (L513, L528) are byte-identical to r26.

## Limitations and observations (none blocking)

1. **Plausible extra preview restore, newly reachable for export (low; source cannot decide).**
   A successful export runs `save_new` → `replay_capture::close_editor()` (L932) →
   `replay_preview::request_stop()` (`replay_capture.rs:290`). If the preview session is still
   `Stopping`, this queues a second `Request::Stop` (`replay_preview.rs:28-32`).
   `save_states(...)` runs between each fighter's `before_frame` and `after_frame`
   (`training/mod.rs:163-171,191-197`). If the player's state is the last to reach `NoAction`,
   the next CPU `before_frame` takes that Stop before `after_frame`'s `clear_session` can drop it.
   It then issues a second `request_preview_restore` (another `restore_stop`). If the CPU finishes
   last, `after_frame` clears it (`replay_preview.rs:209-211`, `REQUEST.take()` at L68).
   This does not affect persistence: the macro is already saved and selected,
   `RESTORE_ACTION` is forced 0, and `service_restore` early-returns while occupied.
   The same pattern already exists for any editor close during `Stopping`. r26 export never
   reached it. Parent can check the r27 console for more than one `[ClipPreview] restore_stop`
   (or a second `restored_idle`) after `[NativeKeyboard] applied id=1`.
2. **The epoch is defense-in-depth.** Real cancellation is `NAME_JOB.take()` (L515) plus the id
   match (L169-173). The `SESSION_GENERATION` comparison only rejects an interleaving where a
   request loads the epoch before `clear_session` and stores its job after the take. That
   matters only if those calls run on different threads; the known callers appear to be on the
   game thread. This does not weaken r26 behavior apart from intentionally removing
   `begin_restore`. `job.id != id` at L175 is redundant after L171 (harmless).
3. **Pre-existing, unchanged:** `clear_session` does not abort an already-queued native keyboard
   request (`native_keyboard.rs:8-12`). A keyboard can still appear after a character/scene
   cleanup in that ≤1-frame queue window. Its result is discarded, not applied.
4. **Pre-existing, unchanged:** `lobby.rs:119-124` calls `named_macros::clear_session` on a
   scene-key change only when the preview is occupied. Other native Training departures rely on
   the L176 mode guard and the short queue-to-applet window.
5. **Newly reachable arms are unverified at runtime.** After a preview restore, `Cancelled`
   (L188-189: replay jobs send `Pad::Back` to the Save screen) and `Failed(code)` (L191-192) no
   longer return silently. Source looks intended. Only the Confirm path has native r27 evidence
   (parent-reported, not re-checked here).
6. **No unit test.** `named_macros.rs` has no `cfg(test)`. The fence rests on this source chain
   plus the parent-owned native run.
7. **Wording nit:** The comment at L26-27 says the restore advances the generation "while its
   export keyboard is open". More precisely, it happens between the queued request and the
   applet's synchronous start/return, because the game loop does not advance while the applet
   is shown. No code impact.

## Commands and errors

All local commands ran through `/usr/bin/pwsh -NoLogo -NoProfile -NonInteractive -Command`
(the watcher launch used `-File`). One raw error was a path mistake, retried with `/snap/bin/rg`:
`/usr/bin/rg: The term '/usr/bin/rg' is not recognized as a name of a cmdlet, function, script file, or executable program.`
`git diff --no-index` exit 1 is its normal "differences found" status. No project program was executed.
