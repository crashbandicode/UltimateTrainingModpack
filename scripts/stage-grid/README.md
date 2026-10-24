# Stage panel assets

`../roster-grid/stage_panel.rs` reads `assets/sources.json` relative to the
roster assets argument (`../../stage-grid/assets`), imports every listed PNG,
and generates one independently hidden picture and UV material per native stage
ID. IDs are decimal and zero-padded to three digits: `stage_icon_000`,
`stage_icon_003`; texture names are `stage_000`, `stage_003`.

The 115 PNGs are decoded directly from the owned game's `stage_1` textures.
Each is 108×90, with its virtual source path, source hash, PNG hash and pixel hash
recorded in `sources.json`. The two initial `stage_0` samples contained series
logos; they are not used. Missing or changed assets fail reproduction.

`catalog.json` joins the owned `ui/param/database/ui_stage_db.prc` entries with
`can_select=true`, sorted by `disp_order`, to the native form-0 stage table and
`msg_name+us_en.msbt` labels using `nam_stg1_` plus `name_id`. The mod adds the
default Waiting Room as a separate text tile. Boss and internal stages are
excluded by the database's selection flag. The image file name uses `name_id`,
which differs from the native package name for some stages.

Decode reproduction (private helper path):

```text
python3 scripts/stage-grid/decode_owned.py /path/to/decode-stage /path/to/owned/stock-assets scripts/stage-grid/catalog.json /path/to/output
```

The helper uses toolbox `bntx::decode::decode_texture_image(file, 0, 0, 0, true)`;
Pillow only packages the decoded RGBA pixels into PNG.

## Runtime pane contract

- `stage_grid`: hidden 1920×1080 child of `TrModMenu`, opaque native-solid backdrop.
- 4 columns × 4 rows, centers `x=(column-1.5)*460`, `y=300-row*200`.
- `stage_tile_00..15`: 420×180; `stage_cursor`: 443×195.
- `stage_label_00..15`: `(x,y-65)`, width 420, font 26.
- `stage_rank_00..15`: `(x-175,y+75)`, width 70, font 28.
- `stage_title`: y455; `stage_name`: y−405; `stage_help`: y−465.
- `stage_icon_{native_id:03}`: initially hidden, aspect-preserved within 340×130.
  Runtime sets visibility and positions each at the selected cell's `(x,y+15)`.
- `stage_all_icon`: initially hidden 130×130, reuses existing `roster_5e` Smashball.
- WaitingRoom311 has no image pane; runtime uses the normal text label tile.

The builder writes a `.stage.json` report beside each roster report. Validation
checks serialized pane/material/texture bindings, native identity UV transforms,
coordinates and fonts, then removes the stage additions to prove that the prior
roster layout is byte-identical. Existing validation subsequently removes both
panels and truncates additions to prove original layout/material preservation;
all original BNTX texture metadata/pixel checks remain in place.
