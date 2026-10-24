//! Existing menu chips plus root-sourced, numbered Character Item photographs.
use crate::*;
use nx_layout_toolbox::bntx::BntxFile;

// A semantic representative from the native modpack button sheet, not new art.
const ALIASES: &[(&str, &str)] = &[
    ("lobby_practice", "quick_menu"),
    ("lobby_stages", "stage_hazards"),
    ("trio_practice", "save_state_mirroring"),
    ("player_character", "save_damage_player"),
    ("cpu_character", "save_damage_cpu"),
    ("player_roster", "randomize_slots"),
    ("cpu_roster", "randomize_slots"),
    ("player_cycle", "playback_loop"),
    ("cpu_cycle", "playback_loop"),
    ("solo_popo_original", "save_damage_player"),
    ("player_kirby_hats", "buff_state"),
    ("cpu_kirby_hats", "buff_state"),
    ("character_item_grant", "character_item"),
    ("p1_cpu_control", "mash_state"),
    ("p1_cpu_level", "clatter_strength"),
    ("named_macro_library", "recording_slot"),
    ("named_macro_placement", "save_state_save"),
    ("named_macro_replay", "input_playback"),
    ("macro_source", "input_record"),
];

pub struct Asset { width: f32, height: f32 }

pub fn import_textures(bntx: &mut BntxFile, roster_assets: &Path) -> Result<Vec<Asset>> {
    let assets = roster_assets.parent().and_then(Path::parent)
        .ok_or("roster assets parent")?.join("item-icons/assets");
    let mut result = Vec::new();
    for index in 0..38 {
        let name = format!("character_item_{index:02}");
        import_png_file(bntx, &name, &assets.join(format!("icon-{index:02}.png")),
            &ImportOptions {
                texture_format: ImportTextureFormat::Bc7,
                ..Default::default()
            })?;
        let texture = &bntx.textures[bntx.texture_index_by_name(&name).ok_or("item texture")?];
        let scale = (64. / texture.width as f32).min(64. / texture.height as f32);
        result.push(Asset { width: texture.width as f32 * scale, height: texture.height as f32 * scale });
    }
    Ok(result)
}

/// Edit the shared button Parts layout, not each instance or the roster/stage
/// panel. The reverse check retains every original pane/material/texture byte.
pub fn append(layout: &mut BFLYT, assets: &[Asset], bntx: &BntxFile) -> Result<Value> {
    let original = layout.clone();
    let old_materials = layout.materials.len();
    let old_textures = layout.textures.len();
    let original_children = layout.find_pane("Icon").ok_or("button Icon")?.children.len();
    let native_item = layout.find_pane("character_item").ok_or("native item chip")?.clone();
    let mut additions = Vec::new();
    for &(alias, source) in ALIASES {
        if layout.pane_exists(alias) { return Err(format!("duplicate chip {alias}").into()); }
        let mut pane = layout.find_pane(source).ok_or_else(|| format!("missing chip source {source}"))?.clone();
        pane.name = alias.into();
        pane.set_visible(false);
        additions.push(pane);
    }
    let native_material = layout.materials[native_item.picture.as_ref()
        .ok_or("native item picture")?.material_index as usize].clone();
    if native_material.texture_maps.len() != 1 { return Err("item UV texture count".into()); }
    let mut pictures: Vec<_> = assets.iter().enumerate().map(|(index, asset)|
        (format!("item_icon_{index:02}"), format!("character_item_{index:02}"), asset.width, asset.height)
    ).collect();
    // These launchers are mounted dynamically, so the static submenu inventory
    // does not include them. Reuse the class portraits already in the archive.
    for (pane, texture) in [
        ("mii_brawler_config", "roster_48"),
        ("mii_sword_config", "roster_49"),
        ("mii_gunner_config", "roster_4a"),
    ] {
        let t = &bntx.textures[bntx.texture_index_by_name(texture).ok_or("Mii portrait")?];
        let scale = (64. / t.width as f32).min(64. / t.height as f32);
        pictures.push((pane.into(), texture.into(), t.width as f32 * scale, t.height as f32 * scale));
    }
    for (index, (name, texture, width, height)) in pictures.iter().enumerate() {
        let mut material = native_material.clone();
        material.name = format!("item_picture_{index:02}");
        material.texture_maps[0].index = i16::try_from(layout.textures.len())?;
        material.white_color = color(255, 255, 255, 255);
        material.black_color = color(0, 0, 0, 0);
        material.flags_untrusted = false;
        material.original_section_size = None;
        let material_index = u16::try_from(layout.materials.len())?;
        layout.materials.push(material);
        layout.textures.push(texture.clone());
        let mut pane = picture(name,
            native_item.translate.x, native_item.translate.y,
            *width, *height, material_index, color(255, 255, 255, 255));
        pane.base_position = native_item.base_position;
        pane.set_visible(false);
        additions.push(pane);
    }
    layout.find_pane_mut("Icon").unwrap().children.extend(additions);
    let encoded = write_bflyt(layout)?;
    let parsed = read_bflyt(&encoded)?;
    assert_eq!(write_bflyt(&parsed)?, encoded);
    for &(alias, source) in ALIASES {
        let mut expected = original.find_pane(source).unwrap().clone();
        expected.name = alias.into();
        expected.set_visible(false);
        assert_eq!(format!("{:?}", parsed.find_pane(alias).ok_or("serialized alias")?), format!("{expected:?}"));
    }
    for (pane_name, texture, width, height) in &pictures {
        let pane = parsed.find_pane(pane_name).ok_or("serialized menu picture")?;
        assert!(!pane.visible());
        assert_eq!((pane.width, pane.height), (*width, *height));
        let material = &parsed.materials[pane.picture.as_ref().ok_or("item picture")?.material_index as usize];
        let name = &parsed.textures[material.texture_maps[0].index as usize];
        assert_eq!(name, texture);
        assert!(bntx.texture_index_by_name(name).is_some());
        assert!(material.proj_tex_gen_params.is_empty());
        assert_eq!(material.tex_coord_gens.len(), 1);
        assert_eq!(material.tex_coord_gens[0].source, 0);
        for transform in &material.texture_transforms {
            assert_eq!(transform.translate, Vec2 { x: 0., y: 0. });
            assert_eq!(transform.rotate, 0.);
            assert_eq!(transform.scale, Vec2 { x: 1., y: 1. });
        }
    }
    let mut restored = parsed;
    restored.find_pane_mut("Icon").unwrap().children.truncate(original_children);
    restored.materials.truncate(old_materials);
    restored.textures.truncate(old_textures);
    assert_eq!(write_bflyt(&restored)?, write_bflyt(&original)?);
    Ok(json!({"item_pictures":assets.len(), "mii_portraits":3, "aliases":ALIASES,
        "original_button_restored_byte_exact":true}))
}
