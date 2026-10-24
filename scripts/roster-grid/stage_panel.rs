//! Independent full-screen stage selector; runtime owns paging and pane binding.
use crate::*;
use nx_layout_toolbox::bntx::BntxFile;

pub struct Asset {
    id: u32,
    width: f32,
    height: f32,
}

pub fn import_textures(bntx: &mut BntxFile, roster_assets: &Path) -> Result<Vec<Asset>> {
    let assets = roster_assets
        .parent()
        .and_then(Path::parent)
        .ok_or("roster assets parent")?
        .join("stage-grid/assets");
    let metadata: Value = serde_json::from_slice(&fs::read(assets.join("sources.json"))?)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut result = vec![];
    for icon in metadata["icons"].as_array().ok_or("stage icons")? {
        let id = u32::try_from(icon["id"].as_u64().ok_or("stage id")?)?;
        if id == 311 || !seen.insert(id) {
            return Err("duplicate or text-only stage asset".into());
        }
        let filename = icon["filename"].as_str().ok_or("stage filename")?;
        if Path::new(filename).components().count() != 1 {
            return Err("stage filename must be local".into());
        }
        let name = format!("stage_{id:03}");
        import_png_file(
            bntx,
            &name,
            &assets.join(filename),
            &ImportOptions {
                // Stage photographs use the game's compact GPU texture format.
                // RGBA8 for every stage exceeds the waiting archive's size cap.
                texture_format: ImportTextureFormat::Bc7,
                ..Default::default()
            },
        )?;
        let texture = &bntx.textures[bntx.texture_index_by_name(&name).ok_or("imported stage")?];
        let scale = (340. / texture.width as f32).min(130. / texture.height as f32);
        result.push(Asset {
            id,
            width: texture.width as f32 * scale,
            height: texture.height as f32 * scale,
        });
    }
    Ok(result)
}

fn solid(layout: &mut BFLYT, name: &str) -> Result<u16> {
    let mut material = layout
        .materials
        .iter()
        .find(|m| m.texture_maps.is_empty() && m.tev_stages.is_empty())
        .cloned()
        .ok_or("native stage solid material")?;
    material.name = name.into();
    material.texture_maps.clear();
    material.texture_transforms.clear();
    material.tex_coord_gens.clear();
    material.tev_stages.clear();
    material.white_color = color(255, 255, 255, 255);
    material.black_color = color(0, 0, 0, 0);
    material.flags_untrusted = false;
    material.original_section_size = None;
    let index = u16::try_from(layout.materials.len())?;
    layout.materials.push(material);
    Ok(index)
}

fn uv_material(layout: &mut BFLYT, name: &str, texture: &str) -> Result<u16> {
    let mut material = layout
        .materials
        .iter()
        .find(|m| m.name == "pic_icn_speed")
        .cloned()
        .ok_or("native stage UV material")?;
    if material.texture_maps.len() != 1 {
        return Err("stage UV texture map count".into());
    }
    let texture_index = match layout.textures.iter().position(|t| t == texture) {
        Some(index) => index,
        None => {
            layout.textures.push(texture.into());
            layout.textures.len() - 1
        }
    };
    material.name = name.into();
    material.texture_maps[0].index = i16::try_from(texture_index)?;
    material.white_color = color(255, 255, 255, 255);
    material.flags_untrusted = false;
    material.original_section_size = None;
    let index = u16::try_from(layout.materials.len())?;
    layout.materials.push(material);
    Ok(index)
}

pub fn append(layout: &mut BFLYT, template: &BasePane, assets: &[Asset]) -> Result<Value> {
    let bg_mat = solid(layout, "stage_background_mat")?;
    let tile_mat = solid(layout, "stage_tile_mat")?;
    let cursor_mat = solid(layout, "stage_cursor_mat")?;
    let mut panel = base("stage_grid", 0., 0., 1920., 1080.);
    panel.set_visible(false);
    let mut background = picture(
        "stage_background",
        0.,
        0.,
        1920.,
        1080.,
        bg_mat,
        color(12, 16, 24, 255),
    );
    background.picture.as_mut().unwrap().tex_coords.clear();
    panel.children.push(background);
    let mut cursor = picture(
        "stage_cursor",
        -690.,
        300.,
        443.,
        195.,
        cursor_mat,
        color(255, 65, 65, 255),
    );
    cursor.picture.as_mut().unwrap().tex_coords.clear();
    panel.children.push(cursor);
    let mut coordinates = vec![];
    for cell in 0..16 {
        let x = (cell % 4) as f32 * 460. - 690.;
        let y = 300. - (cell / 4) as f32 * 200.;
        let mut tile = picture(
            &format!("stage_tile_{cell:02}"),
            x,
            y,
            420.,
            180.,
            tile_mat,
            color(25, 32, 44, 255),
        );
        tile.picture.as_mut().unwrap().tex_coords.clear();
        panel.children.push(tile);
        coordinates.push(json!({"cell":cell,"x":x,"y":y}));
    }
    // Draw thumbnails after all tile backgrounds. Every native ID is independent.
    for asset in assets {
        let name = format!("stage_icon_{:03}", asset.id);
        let material = uv_material(layout, &name, &format!("stage_{:03}", asset.id))?;
        let mut icon = picture(
            &name,
            0.,
            15.,
            asset.width,
            asset.height,
            material,
            color(255, 255, 255, 255),
        );
        icon.set_visible(false);
        panel.children.push(icon);
    }
    let all_mat = uv_material(layout, "stage_all_icon", "roster_5e")?;
    let mut all_icon = picture(
        "stage_all_icon",
        0.,
        15.,
        130.,
        130.,
        all_mat,
        color(255, 255, 255, 255),
    );
    all_icon.set_visible(false);
    panel.children.push(all_icon);
    for cell in 0..16 {
        let x = (cell % 4) as f32 * 460. - 690.;
        let y = 300. - (cell / 4) as f32 * 200.;
        panel.children.push(text(
            template,
            &format!("stage_label_{cell:02}"),
            "",
            x,
            y - 65.,
            420.,
            26.,
            color(255, 255, 255, 255),
        ));
        panel.children.push(text(
            template,
            &format!("stage_rank_{cell:02}"),
            "",
            x - 175.,
            y + 75.,
            70.,
            28.,
            color(255, 85, 85, 255),
        ));
    }
    for (name, value, y, size) in [
        ("stage_title", "Stage Cycle", 455., 44.),
        ("stage_name", "", -405., 36.),
        (
            "stage_help",
            "A Add   R Remove   ZL All   B Back",
            -465.,
            28.,
        ),
    ] {
        panel.children.push(text(
            template,
            name,
            value,
            0.,
            y,
            1700.,
            size,
            color(255, 255, 255, 255),
        ));
    }
    let count = panel.children.len();
    layout
        .find_pane_mut("TrModMenu")
        .ok_or("TrModMenu")?
        .children
        .push(panel);
    Ok(
        json!({"panel":"stage_grid","parent":"TrModMenu","hidden":true,
        "columns":4,"rows":4,"children":count,"cursor":[443,195],"tile":[420,180],
        "thumbnail_max":[340,130],"labels":16,"ranks":16,"coordinates":coordinates,
        "icons":assets.iter().map(|a| json!({"id":a.id,"pane":format!("stage_icon_{:03}",a.id),"size":[a.width,a.height]})).collect::<Vec<_>>(),
        "all_texture":"roster_5e","text_only_stage":311}),
    )
}

pub fn validate(layout: &BFLYT, bntx: &BntxFile, assets: &[Asset]) -> Result<()> {
    let panel = layout
        .find_pane("stage_grid")
        .ok_or("stage roundtrip panel")?;
    assert!(!panel.visible());
    assert_eq!((panel.width, panel.height), (1920., 1080.));
    assert_eq!(panel.children.len(), 54 + assets.len());
    let unique: std::collections::BTreeSet<_> = panel.children.iter().map(|p| &p.name).collect();
    assert_eq!(unique.len(), panel.children.len());
    let background = panel.find("stage_background").ok_or("stage background")?;
    assert_eq!((background.width, background.height), (1920., 1080.));
    for pane in panel.children.iter().filter(|p| {
        p.name == "stage_background"
            || p.name == "stage_cursor"
            || p.name.starts_with("stage_tile_")
    }) {
        let picture = pane.picture.as_ref().ok_or("stage solid picture")?;
        let material = &layout.materials[picture.material_index as usize];
        assert!(material.texture_maps.is_empty());
        assert!(material.tev_stages.is_empty());
        assert!(picture.tex_coords.is_empty());
        assert!(picture.vertex_colors.iter().all(|c| c.a == 255));
    }
    for cell in 0..16 {
        let x = (cell % 4) as f32 * 460. - 690.;
        let y = 300. - (cell / 4) as f32 * 200.;
        for (prefix, px, py, width, font) in [
            ("stage_label", x, y - 65., 420., 26.),
            ("stage_rank", x - 175., y + 75., 70., 28.),
        ] {
            let pane = panel
                .find(&format!("{prefix}_{cell:02}"))
                .ok_or("stage cell text")?;
            assert_eq!(
                (pane.translate.x, pane.translate.y, pane.width),
                (px, py, width)
            );
            assert_eq!(pane.text.as_ref().ok_or("stage text")?.font_size.y, font);
        }
        let tile = panel
            .find(&format!("stage_tile_{cell:02}"))
            .ok_or("stage tile")?;
        assert_eq!(
            (tile.translate.x, tile.translate.y, tile.width, tile.height),
            (x, y, 420., 180.)
        );
    }
    let cursor = panel.find("stage_cursor").ok_or("stage cursor")?;
    assert_eq!((cursor.width, cursor.height), (443., 195.));
    for (name, texture) in assets
        .iter()
        .map(|a| {
            (
                format!("stage_icon_{:03}", a.id),
                format!("stage_{:03}", a.id),
            )
        })
        .chain(std::iter::once((
            "stage_all_icon".into(),
            "roster_5e".into(),
        )))
    {
        let pane = panel.find(&name).ok_or("stage icon pane")?;
        assert!(!pane.visible());
        assert!(pane.width <= 340. && pane.height <= 130.);
        let material = &layout.materials
            [pane.picture.as_ref().ok_or("stage picture")?.material_index as usize];
        assert_eq!(material.name, name);
        assert_eq!(material.texture_maps.len(), 1);
        assert_eq!(
            layout.textures[material.texture_maps[0].index as usize],
            texture
        );
        assert!(bntx.texture_index_by_name(&texture).is_some());
        assert!(material.proj_tex_gen_params.is_empty());
        assert_eq!(material.tex_coord_gens.len(), 1);
        assert_eq!(material.tex_coord_gens[0].source, 0);
        assert_eq!(material.tex_coord_gens[0].raw[1], 0);
        for transform in &material.texture_transforms {
            assert_eq!(transform.translate, Vec2 { x: 0., y: 0. });
            assert_eq!(transform.rotate, 0.);
            assert_eq!(transform.scale, Vec2 { x: 1., y: 1. });
        }
    }
    assert!(panel.find("stage_icon_311").is_none());
    for (name, y) in [
        ("stage_title", 455.),
        ("stage_name", -405.),
        ("stage_help", -465.),
    ] {
        assert_eq!(
            panel.find(name).ok_or("stage header/footer")?.translate.y,
            y
        );
    }
    Ok(())
}
