mod mips;
mod menu_pictures;
mod stage_panel;
mod validation;
use nx_layout_toolbox::{
    bflyt::*,
    bntx::{
        pipeline::{import_png_file, ImportOptions, ImportTextureFormat},
        read_bntx, write_bntx,
    },
    sarc::{read_arc, write_arc},
};
use serde_json::{json, Value};
use std::{fs, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn color(r: u8, g: u8, b: u8, a: u8) -> Color8 {
    Color8 { r, g, b, a }
}
fn base(name: &str, x: f32, y: f32, w: f32, h: f32) -> BasePane {
    BasePane {
        kind: PaneKind::Pane,
        flag: 3,
        base_position: 0,
        alpha: 255,
        flag_ex: 0,
        name: name.into(),
        user_data_field: [0; 8],
        translate: Vec3 { x, y, z: 0. },
        rotate: Vec3::default(),
        scale: Vec2 { x: 1., y: 1. },
        width: w,
        height: h,
        picture: None,
        text: None,
        window: None,
        parts: None,
        user_data: None,
        children: vec![],
        trailing: vec![],
        opaque: None,
    }
}
#[allow(clippy::too_many_arguments)]
fn picture(name: &str, x: f32, y: f32, w: f32, h: f32, material: u16, tint: Color8) -> BasePane {
    let mut p = base(name, x, y, w, h);
    p.kind = PaneKind::Picture;
    p.picture = Some(PicturePane {
        material_index: material,
        vertex_colors: [tint; 4],
        tex_coords: vec![PaneTexCoord {
            top_left: Vec2 { x: 0., y: 0. },
            top_right: Vec2 { x: 1., y: 0. },
            bottom_left: Vec2 { x: 0., y: 1. },
            bottom_right: Vec2 { x: 1., y: 1. },
        }],
        flags: 0,
    });
    p
}
#[allow(clippy::too_many_arguments)]
fn text(
    template: &BasePane,
    name: &str,
    s: &str,
    x: f32,
    y: f32,
    w: f32,
    size: f32,
    tint: Color8,
) -> BasePane {
    let mut p = template.clone();
    p.name = name.into();
    p.children.clear();
    p.user_data = None;
    p.translate = Vec3 { x, y, z: 0. };
    p.rotate = Vec3::default();
    p.scale = Vec2 { x: 1., y: 1. };
    p.base_position = 0;
    p.width = w;
    p.height = size * 1.4;
    p.flag = 3;
    p.alpha = 255;
    let t = p.text.as_mut().unwrap();
    t.text_position = 4;
    t.text_alignment = 1;
    t.text_cols = [tint; 2];
    t.font_size = Vec2 {
        x: size * 0.72,
        y: size,
    };
    t.text_buf_bytes = 256;
    t.text_str_bytes = ((s.encode_utf16().count() + 1) * 2) as u16;
    t.text_str_offset = 0xa8;
    t.text_id_offset = 0;
    t.line_width_offset_offset = 0;
    t.per_character_transform_offset = 0;
    t.trailing = s
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    p
}
fn run() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() == 3 && a[1] == "audit" {
        let arc = read_arc(&fs::read(&a[2])?)?;
        let i = arc
            .position("blyt/info_training.bflyt")
            .or_else(|| arc.position("blyt/trmod_waiting.bflyt"))
            .ok_or("layout")?;
        let l = read_bflyt(&arc.files[i].data)?;
        println!("textures: {:?}", l.textures);
        for (i, m) in l.materials.iter().enumerate() {
            println!("MATERIAL {i}: {m:#?}");
        }
        return Ok(());
    }
    if a.len() != 5 {
        return Err("usage: roster-grid-builder INPUT.arc OUTPUT.arc ASSETS REPORT.json".into());
    }
    let original = fs::read(&a[1])?;
    let mut arc = read_arc(&original)?;
    let bi = arc.position("timg/__Combined.bntx").ok_or("BNTX missing")?;
    let old_bntx = arc.files[bi].data.clone();
    let before = read_bntx(&old_bntx)?;
    let mut bntx = before.clone();
    let icons: Value = serde_json::from_slice(&fs::read(Path::new(&a[3]).join("sources.json"))?)?;
    for icon in icons["icons"].as_array().ok_or("icons")? {
        let kind = icon["kind"].as_u64().ok_or("kind")?;
        import_png_file(
            &mut bntx,
            &format!("roster_{kind:02x}"),
            &Path::new(&a[3]).join(icon["filename"].as_str().ok_or("filename")?),
            &ImportOptions {
                texture_format: ImportTextureFormat::Rgba8,
                ..Default::default()
            },
        )?;
    }
    let stage_assets = stage_panel::import_textures(&mut bntx, Path::new(&a[3]))?;
    let item_assets = menu_pictures::import_textures(&mut bntx, Path::new(&a[3]))?;
    let raw = write_bntx(&bntx)?;
    let repaired = mips::repair(&raw, &old_bntx)?;
    let after = read_bntx(&repaired)?;
    validation::validate_texture_dictionary(&after)?;
    for t in &before.textures {
        let n = t.name(&before);
        let t2 = &after.textures[after.texture_index_by_name(n).ok_or("lost texture")?];
        let mut normalized_before = t.clone();
        let mut normalized_after = t2.clone();
        normalized_before.name_string_index = 0;
        normalized_after.name_string_index = 0;
        normalized_before.data_offset_in_brtd = 0;
        normalized_after.data_offset_in_brtd = 0;
        if format!("{normalized_before:?}") != format!("{normalized_after:?}")
            || t.pixel_data(&before.brtd) != t2.pixel_data(&after.brtd)
        {
            return Err(format!("changed pixels {n}").into());
        }
    }
    arc.files[bi].data = repaired;
    let mut menu_picture_reports = vec![];
    for entry in &mut arc.files {
        if !entry.data.starts_with(b"FLYT") { continue; }
        let mut button = read_bflyt(&entry.data)?;
        if button.pane_exists("MenuButton") && button.pane_exists("Icon")
            && button.pane_exists("character_item") {
            let report = menu_pictures::append(&mut button, &item_assets, &after)?;
            entry.data = write_bflyt(&button)?;
            menu_picture_reports.push(json!({"path":entry.name,"report":report}));
        }
    }
    if menu_picture_reports.len() != 1 { return Err("expected one shared menu button layout".into()); }
    let li = arc
        .position("blyt/info_training.bflyt")
        .or_else(|| arc.position("blyt/trmod_waiting.bflyt"))
        .ok_or("menu layout missing")?;
    let mut layout = read_bflyt(&arc.files[li].data)?;
    let original_layout = layout.clone();
    let old_texture_refs = layout.textures.len();
    if layout.pane_exists("roster_grid") || layout.pane_exists("stage_grid") {
        return Err("already built".into());
    }
    let template = layout
        .find_pane("TrModDisp0Txt")
        .ok_or("FooterTxt")?
        .clone();
    let pic_template = layout
        .find_pane("FooterBackground")
        .ok_or("FooterBackground")?
        .picture
        .as_ref()
        .ok_or("background picture")?
        .material_index;
    // FooterBackground is a screen-projected stripe: source4, rotate155,
    // translateX=-27.2 and a projection parameter. Its shader ignores the
    // ordinary picture UV path. Use the native speed icon's existing shader
    // variant and identity UV transforms instead.
    let mut material = layout
        .materials
        .iter()
        .find(|m| m.name == "pic_icn_speed")
        .ok_or("native UV icon material missing")?
        .clone();
    material.white_color = color(255, 255, 255, 255);
    material.flags_untrusted = false;
    material.original_section_size = None;
    if material.texture_maps.len() != 1 {
        return Err("background material must have one texture".into());
    }
    let old_materials = layout.materials.len();
    let grid: Vec<Vec<u32>> = serde_json::from_str(include_str!("grid.json"))?;
    assert_eq!(grid.len(), 87);
    assert_eq!(grid.iter().map(Vec::len).sum::<usize>(), 90);
    let mut panel = base("roster_grid", 0., 0., 1920., 1080.);
    panel.set_visible(false);
    // Opaque panel and cursor precede portraits and badge text in native draw order.
    let mut bg = picture(
        "roster_background",
        0.,
        0.,
        1830.,
        1000.,
        pic_template,
        color(12, 16, 24, 250),
    );
    bg.picture.as_mut().unwrap().tex_coords = vec![];
    panel.children.push(bg);
    let cursor_material = layout.materials.len() as u16;
    let mut cm = layout
        .materials
        .iter()
        .find(|m| m.texture_maps.is_empty() && m.tev_stages.is_empty())
        .cloned()
        .ok_or("native solid material missing")?;
    cm.name = "roster_cursor_mat".into();
    cm.texture_maps.clear();
    cm.texture_transforms.clear();
    cm.tex_coord_gens.clear();
    cm.tev_stages.clear();
    cm.flags_untrusted = false;
    cm.original_section_size = None;
    layout.materials.push(cm.clone());
    cm.name = "roster_background_mat".into();
    let bg_material = layout.materials.len() as u16;
    layout.materials.push(cm.clone());
    cm.name = "roster_all_mat".into();
    cm.white_color = color(255, 255, 255, 255);
    cm.black_color = color(0, 0, 0, 0);
    let all_material = layout.materials.len() as u16;
    layout.materials.push(cm);
    panel.children[0].picture.as_mut().unwrap().material_index = bg_material;
    let mut cursor = picture(
        "roster_cursor",
        -792.,
        345.,
        120.,
        102.,
        cursor_material,
        color(255, 65, 65, 255),
    );
    cursor.picture.as_mut().unwrap().tex_coords.clear();
    panel.children.push(cursor);
    let mut coordinates = vec![];
    for (cell, kinds) in grid.iter().enumerate() {
        let row = cell / 13;
        let col = if row == 6 { cell % 13 + 2 } else { cell % 13 };
        let x = (col as f32 - 6.) * 132.;
        let y = 345. - row as f32 * 105.;
        coordinates.push(json!({"cell":cell,"row":row,"column":col,"x":x,"y":y,"kinds":kinds}));
        for &kind in kinds {
            let texture = layout.textures.len() as u16;
            layout.textures.push(format!("roster_{kind:02x}"));
            material.name = format!("roster_mat_{kind:02x}");
            material.texture_maps[0].index = texture as i16;
            let mi = layout.materials.len() as u16;
            layout.materials.push(material.clone());
            let unsupported = [0x48, 0x49, 0x4a].contains(&kind);
            let mut p = picture(
                &format!("roster_icon_{kind:02X}"),
                x,
                y,
                80.,
                80.,
                mi,
                if unsupported {
                    color(110, 110, 110, 150)
                } else {
                    color(255, 255, 255, 255)
                },
            );
            p.set_visible(![0x25, 0x26, 0x5b].contains(&kind));
            panel.children.push(p);
        }
        panel.children.push(text(
            &template,
            &format!("roster_p_{cell:02}"),
            "",
            x - 36.,
            y + 35.,
            52.,
            28.,
            color(255, 85, 85, 255),
        ));
        panel.children.push(text(
            &template,
            &format!("roster_c_{cell:02}"),
            "",
            x + 36.,
            y + 35.,
            52.,
            28.,
            color(85, 165, 255, 255),
        ));
        for (prefix, side, tint) in [
            ("p", -56., color(255, 85, 85, 255)),
            ("c", 56., color(85, 165, 255, 255)),
        ] {
            let mut stripe = picture(
                &format!("roster_all_{prefix}_{cell:02}"),
                x + side,
                y,
                4.,
                86.,
                all_material,
                tint,
            );
            stripe.picture.as_mut().unwrap().tex_coords.clear();
            stripe.set_visible(false);
            panel.children.push(stripe);
        }
        if kinds == &[0x4b] {
            for (prefix, side, tint) in [
                ("p", -36., color(255, 85, 85, 255)),
                ("c", 36., color(85, 165, 255, 255)),
            ] {
                panel.children.push(text(
                    &template,
                    &format!("roster_solo_{prefix}"),
                    "",
                    x + side,
                    y - 35.,
                    40.,
                    28.,
                    tint,
                ));
            }
        }
    }
    for (name, s, x, y, w, size) in [
        ("roster_title", "Character Cycle", 0., 465., 1500., 44.),
        ("roster_name", "Mario", 0., -390., 1500., 36.),
        (
            "roster_help",
            "A Add   R Remove   X Form   Y Solo Popo   ZR Player/CPU   ZL All   B Back",
            0.,
            -465.,
            1700.,
            28.,
        ),
        ("roster_role", "CPU", 528., -285., 120., 26.),
    ] {
        panel.children.push(text(
            &template,
            name,
            s,
            x,
            y,
            w,
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
    let roster_layout = layout.clone();
    let stage_report = stage_panel::append(&mut layout, &template, &stage_assets)?;
    let bytes = write_bflyt(&layout)?;
    let parsed = read_bflyt(&bytes)?;
    assert_eq!(write_bflyt(&parsed)?, bytes);
    assert_eq!(
        parsed
            .pane_text("roster_title")
            .as_deref()
            .ok_or("missing title")?,
        "Character Cycle"
    );
    assert_eq!(
        parsed
            .pane_text("roster_role")
            .as_deref()
            .ok_or("missing role")?,
        "CPU"
    );
    assert_eq!(
        parsed
            .pane_text("roster_p_00")
            .as_deref()
            .ok_or("missing badge")?,
        ""
    );
    let p = parsed.find_pane("roster_grid").ok_or("roundtrip panel")?;
    assert_eq!(p.children.len(), count);
    assert!(!p.visible());
    for i in 0..87 {
        assert!(p.find(&format!("roster_p_{i:02}")).is_some());
        assert!(p.find(&format!("roster_c_{i:02}")).is_some());
    }
    assert_eq!(
        p.children
            .iter()
            .filter(|p| p.kind == PaneKind::Picture)
            .count(),
        266
    );
    assert_eq!(
        p.children
            .iter()
            .filter(|p| p.kind == PaneKind::Text)
            .count(),
        180
    );
    let unique: std::collections::BTreeSet<_> = p.children.iter().map(|p| &p.name).collect();
    assert_eq!(unique.len(), p.children.len());
    // Validate the serialized picture -> material -> texture-list -> BNTX
    // binding, and reject projected or transformed template regressions.
    for icon in &p.children {
        if let Some(hex) = icon.name.strip_prefix("roster_icon_") {
            let kind = u32::from_str_radix(hex, 16)?;
            let m = &parsed.materials[icon.picture.as_ref().unwrap().material_index as usize];
            assert_eq!(m.texture_maps.len(), 1);
            let name = &parsed.textures[m.texture_maps[0].index as usize];
            assert_eq!(name, &format!("roster_{kind:02x}"));
            assert!(after.texture_index_by_name(name).is_some());
            assert!(m.proj_tex_gen_params.is_empty());
            assert_eq!(m.tex_coord_gens.len(), 1);
            assert_eq!(m.tex_coord_gens[0].source, 0);
            assert_eq!(m.tex_coord_gens[0].raw[1], 0);
            for t in &m.texture_transforms {
                assert_eq!(t.translate, Vec2 { x: 0., y: 0. });
                assert_eq!(t.rotate, 0.);
                assert_eq!(t.scale, Vec2 { x: 1., y: 1. });
            }
        }
    }
    stage_panel::validate(&parsed, &after, &stage_assets)?;
    let mut without_stage = parsed.clone();
    without_stage
        .find_pane_mut("TrModMenu")
        .unwrap()
        .children
        .retain(|p| p.name != "stage_grid");
    without_stage
        .materials
        .truncate(roster_layout.materials.len());
    without_stage
        .textures
        .truncate(roster_layout.textures.len());
    assert_eq!(write_bflyt(&without_stage)?, write_bflyt(&roster_layout)?);
    let mut restored = parsed.clone();
    restored
        .find_pane_mut("TrModMenu")
        .unwrap()
        .children
        .retain(|p| p.name != "roster_grid" && p.name != "stage_grid");
    restored.materials.truncate(old_materials);
    restored.textures.truncate(old_texture_refs);
    assert_eq!(write_bflyt(&restored)?, write_bflyt(&original_layout)?);
    arc.files[li].data = bytes;
    let output = write_arc(&arc)?;
    let round = read_arc(&output)?;
    for f in &arc.files {
        let idx = round
            .position(f.name.as_ref().ok_or("unnamed")?)
            .ok_or("lost entry")?;
        assert_eq!(f.data, round.files[idx].data);
    }
    fs::write(&a[2], output)?;
    fs::write(
        Path::new(&a[4]).with_extension("menu-pictures.json"),
        serde_json::to_vec_pretty(&menu_picture_reports)?,
    )?;
    fs::write(
        Path::new(&a[4]).with_extension("stage.json"),
        serde_json::to_vec_pretty(&stage_report)?,
    )?;
    fs::write(
        &a[4],
        serde_json::to_vec_pretty(
            &json!({"parent":"TrModMenu","panel":"roster_grid","size":[1920,1080],"portrait":[80,80],"cursor":[120,102],"children":count,"icons":90,"badge_textboxes":174,"solo_textboxes":2,"all_stripes":174,"textboxes":180,"old_textures_preserved":before.textures.len(),"new_textures":after.textures.len(),"old_materials_preserved":old_materials,"coordinates":coordinates}),
        )?,
    )?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
