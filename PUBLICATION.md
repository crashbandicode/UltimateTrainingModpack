# Match capture: source, qualification and benchmark archive

Updated October 12, 2026. This is the `named-macros-research` branch of
[`intpa/UltimateTrainingModpack`](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack).

- [Capture implementation](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack/src/branch/named-macros-capture)
  — revision27 source, including the named macro library, native capture/save
  association, Training preview and shared-format integration.
- [Illustrated walkthrough](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack/src/branch/named-macros-walkthrough/docs/named-macros/README.md).
- [Build layout and external prerequisites](https://git.syntaxbyexample.com/intpa/UltimateTrainingModpack/src/branch/named-macros-capture/docs/named-macros-build.md).

## Current acceptance

[Revision27 qualification](revision27-qualification.md) records native replay save
and cancellation, full-precision canonical input retention, cold Training access
with capture disabled, CPU reload, eight-input preview and a persisted named macro.
[Deluxe qualification](live-deluxe2-qualification.md) covers the same scoped workflow
with current Deluxe active in manual LessLag mode. These are Eden results, not
physical-console or general network qualification.

**Performance and slowdown qualification remain incomplete.**
[The first OFF measurement](live-benchmark-b1off.md) contains a valid 40-second
combat window but did not finish the required native save; it is not a completed
OFF/ON pair. The automatic post-measurement save/stop helper passed focused tests
but has not been live-qualified. No overhead, equivalence or slowdown-preservation
conclusion is supported yet.

The prospective [campaign](live-performance-campaign.md) uses the first match in
a fresh process with identical prewarmed disk caches. It supersedes the unexecuted
warmup-then-measure lifecycle in the original [protocol](live-performance-protocol.md).
Rolling FPS and whole-session CSV composition intervals are separate measurements;
neither supplies a complete Smash simulation-tick clock.

## Archived material

- Nine `live_*.py` benchmark/staging/collection/control helpers and seven test modules.
- Qualification, fixture, pilot and incomplete-measurement reports, plus the shared
  module compatibility audit and naming-fence review. Dated statements in these
  reports describe their checkpoint; this page supplies the current overview.
- `revision27-builds/normal.json` and `trace.json`, four build/Clippy receipts,
  and all 118 frozen product source/lockfile inputs.
- The 12 hash-pinned external build inputs: shared-format source and format document,
  plus the optional replay-core manifest. The latter is provenance only, not a
  complete decoder crate. Training disables that optional feature.
- The Deluxe fixture's manifest, without its executable or packaged source.

The immutable build-manifest status `checked_candidate_runtime_pending` is
intentionally preserved: the staging helper checks that literal value. Subsequent
runtime outcomes are recorded in the qualification reports, not rewritten into
historical manifests. The published product sources were rechecked against all
118 input pins and the archived external inputs against all 12 pins before commit.
Existing normal/trace artifact hashes and successful build receipts were verified.

The existing strict all-target Clippy and no-default-feature build results apply
to that identical source. Publication added documentation and Git metadata only;
it did not rebuild or replace the qualified candidate. Earlier 41 integrated tests
and the later 16 focused finish/driver/evidence tests are separate runs, not a claim
that a combined suite was rerun for publication.

## Reusing the research helpers

This branch is a source/evidence archive, not a self-contained emulator test kit.
The original checkout is:

```text
/home/intpa/smash-training-lobby/research/named-macros/
```

`live_acceptance.py` derives the product directory from that location; other
helpers retain explicit paths for the owned Eden CLI profile, run archive,
`acceptance.py`, `edenctl.py`, launcher, systemd units and stopped archive inspector.
The stager also requires the exact locally retained fixture2 saves/cache, original
backup receipts, frozen NROs, hook plugins and optional Deluxe payload. These are
not downloaded or recreated by importing the scripts. An unrelated checkout must
not be treated as permission to operate those resources. Review paths, source pins,
ownership and a finite resource grant before executing live commands.

The six synthetic test modules use temporary fixtures/mocks:

```text
python3 -m unittest test_live_acceptance test_live_perf test_live_benchmark_drive test_live_benchmark_stage test_live_benchmark_evidence test_live_benchmark_pulse
```

`test_live_benchmark_finish` additionally needs Pillow and five original screenshots
under `/home/intpa/eden-replay/acceptance-fable/runs/`:

- `live-r27-bench-b1off/native-save-dialog.png` (SHA-256
  `2153896a0eec637b9d60abd3b35da6ab89fa95a025ac0aa35eb7a09712d399fc`);
- `live-r27-fixture2/fixture2-native-save.png`;
- `live-r27-bench-b1off/fixed-ready.png`;
- `live-r27-bench-b1off/measure-window-end.png`;
- `live-r27-bench-pilot3/pilot3-warmup-details.png`.

Those tests do not launch an emulator, but the image fixtures remain in the local
run archive. Missing fixtures must not be reported as a passing visual gate.

Raw profile backups, saves/replays, encryption keys, credentials, caches, game
assets, game-derived disassembly, run logs and test executables remain local.
The allowlist in `.gitignore` keeps those outside this publication. The walkthrough
branch contains the reviewed native screenshots used in its instructions.
