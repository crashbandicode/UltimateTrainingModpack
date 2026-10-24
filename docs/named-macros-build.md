# Revision27 capture source and build inputs

This development source is on main. The illustrated walkthrough is
[named-macros.md](named-macros.md). The revision 27 qualification
archive remains in commit
[`86a1426`](https://github.com/crashbandicode/UltimateTrainingModpack/commit/86a1426597246a2d0b36e9e652878053efea38a0).

## Qualification

The Rust sources match the 118 product-input hashes in the frozen revision27
manifest. Normal and trace builds passed strict all-target Clippy and release
builds with default features disabled. Native replay-save association, cancellation,
cold Training access, CPU reload, eight-input preview and named export passed the
scoped Eden checks; current Deluxe LessLag coexistence also passed.

Matched capture-OFF/ON performance comparisons and the bounded slowdown trial are
still incomplete. Publishing this source is not a binary release or physical
Switch qualification. See [usage and limits](named-macros.md).

## External source layout

The qualified source retains its existing path dependency on `smash-match-data`.
To reproduce that layout, place the projects as follows:

```text
workspace/
  smash-training-lobby/
    UltimateTrainingModpack-single-file/  # this tree
    research/named-macros/                # local qualification archive
  smash-match-data-design/                # pinned shared-format source
  smash-replay-export/replay-core/        # existing optional decoder crate
```

That archive commit contains `revision27-builds/normal.json`, `trace.json`, the
exact shared-format source snapshot under `revision27-builds/external/smash-match-data/`,
and the four qualified Cargo lockfiles under `revision27-builds/source/`.
Restore those lockfiles at their matching product-relative paths and use that
shared-format snapshot rather than assuming a newer sibling checkout is identical.
The product's existing ignore rules exclude Cargo lockfiles, so they are preserved
in the research branch instead.

Training sets `default-features = false` for `smash-match-data`; the replay-import
decoder is not compiled into this integration. Cargo still resolves its optional
path dependency, so an existing `smash-replay-export/replay-core` source checkout
must be present. Its manifest hash is recorded under
`external_sources.smash-replay-core-manifest`. The manifest-only snapshot is
provenance, not a complete decoder crate. This publication does not replace that
separate project's checkout or publish its application.

The shared snapshot supplies observation types, archive I/O and validation.
Training's native hooks and streaming archive/cache projection remain in this
repository; the shared module is not a renderer, simulator or AI application.

## Build recipe

Use the project's Skyline toolchain. The qualified host used the isolated Skyline
installation documented in the archived build receipts, with one Cargo job:

```text
cargo skyline clippy --no-default-features --features waiting-roster-reload -- --all-targets -- -D warnings
cargo skyline build --release --no-default-features --features waiting-roster-reload
```

The trace build additionally enables `named-input-trace`; performance comparisons
must use the normal build. Frozen artifact identifiers are:

| Build | Bytes | SHA-256 |
| --- | ---: | --- |
| Normal27 | 3346432 | `4d6cab005296ea7e95082289f3825e4c6ad8e9cc3aefad3d37e391657c7daac7` |
| Trace27 | 3342336 | `7f8863618d0c6dc0fd0d34f94cd2a40851b59332383abb005e64a20510f000eb` |

Build receipts and hashes identify the previously tested artifacts; they do not
promise byte-identical output from an arbitrary newer compiler or dependency set.
