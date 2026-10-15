Native roster panel builder. Uses the external `nx-layout-toolbox` library at
`../../../../eden-gnw/toolbox-cli` (relative to this Cargo manifest); its own lockfile
pins dependencies. No game archive or executable is downloaded by the builder.
The asset manifest records each of the 89 PNG sources and SHA256 digests.

Build with `cargo build --manifest-path scripts/roster-grid/Cargo.toml --no-default-features -j1`.
Check with `cargo clippy --manifest-path scripts/roster-grid/Cargo.toml --all-targets -j1 -- -D warnings`.
Use a bounded systemd user unit: MemoryMax=3G, MemorySwapMax=0, CPUQuota=200%, RuntimeMaxSec=600.
Keep Cargo target and cache on `/home`, not `/tmp`.

Run `/usr/bin/python3 scripts/roster-grid/reproduce.py BUILDER ORIGINAL_DONOR.arc ORIGINAL_WAITING.arc STOCK.arc OUTPUT_DIR`.
The wrapper verifies the three baseline digests and every PNG digest, builds both
archives, then calls the repository's `make_waiting_patch.make`. It independently
decodes and checks the complete patch and all source/target fingerprints.
Outputs remain in OUTPUT_DIR for review; copy `layout-roster.arc` to
`src/static/layout.arc`, `waiting_ui.patch` to `src/static/waiting_ui.patch`, and
the manifest to `scripts/waiting_patch_manifest.json` after verification.

`roster_grid` is appended directly under `TrModMenu` in `info_training.bflyt`
and `trmod_waiting.bflyt`. It starts invisible. All descendants are flat:
89 `roster_icon_XX` pictures (uppercase hex kind), 87 pairs `roster_p_00..86`
and `roster_c_00..86` (decimal), `roster_cursor`, `roster_background`, and
`roster_title`, `roster_name`, `roster_help`, `roster_role` textboxes.
The icons use private materials and new RGBA8 textures. The cursor/background
each have their own material cloned from an existing native solid material.
Portraits clone the existing `pic_icn_speed` material: identity texture transforms,
UV source 0, no projection payload, existing native shader variant `0x19`.
Do not use `FooterBackground`: it is a screen-projected stripe material, with
UV source 4, translation −27.2, rotation 155 degrees and flag `0x8019`.
The builder checks every serialized portrait's material/texture-list/BNTX name
binding, ordinary UV source and identity transforms. `BUILDER audit INPUT.arc`
dumps the material registrations without modifying the archive.

Native canvas 1920x1080 maps to 1280x720. For each physical column and row,
cell center is `x=(column-6)*132, y=345-row*105`. Six complete rows have 13 cells;
the last 9 occupy columns 2..10. Portrait 96x96, cursor 120x102. Cursor position is
the cell center; change its four vertex colors for the active role. Badges are
52 wide, font 28, at x±30/y−35; title y465/font44, name y−390/font36,
help y−465/font28. Role text is at (528,−285), the final cell 86.
Badge buffers hold 128 UTF16 code units and start empty. Existing game text font
and material are cloned from `TrModDisp0Txt`; badges are red and blue.

`grid.json` is the fixed physical grid, independent of serialized catalog order.
Trainer kinds 24/25/26 and Aegis 5A/5B (hex) share coordinates; only 24 and 5A initially
show. Unsupported 4B/48/49/4A are dimmed. This matches the observed all-unlocked
save, and does not discover live save ordering.

The builder checks BFLYT parse/write stability, unique descendant names and
counts, SARC entry roundtrip, original texture metadata/pixels, original mip
offset arrays, and reversal of the panel insertion to the original layout.
`mips.rs` is adapted from research/ui-port's proven native pointer-array repair:
existing textures use original offset arrays, while new textures require one mip.
The original pointer arrays and 4096-byte GPU pool alignment are preserved.
Host structural checks do not replace native Training/waiting rendering validation.
