//! Pointer-free native draft values. The native saved ID is deliberately zero.
use crate::{
    inventory::{self, MiiConfig}, lifetime::Configuration, saved,
};

pub type Row = [u8; saved::HEADER_LEN];

pub fn from_seed(seed: &MiiConfig) -> Result<Row, &'static str> {
    Configuration::from_saved(seed).map_err(|_| "Invalid editor appearance")?;
    let mut row = [0; saved::HEADER_LEN];
    let mut units = Vec::new();
    for ch in seed.name.chars() {
        let mut encoded = [0; 2];
        let next = ch.encode_utf16(&mut encoded);
        if units.len() + next.len() > 10 { break; }
        units.extend_from_slice(next);
    }
    if units.is_empty() { units.extend("New Mii".encode_utf16()); }
    row[0x114..0x118].copy_from_slice(&(units.len() as u32).to_le_bytes());
    for (i, unit) in units.into_iter().enumerate() {
        row[0x118 + i * 2..0x11a + i * 2].copy_from_slice(&unit.to_le_bytes());
    }
    for (offset, tag) in [0x130, 0x138, 0x1a0].into_iter().zip(seed.raw_tags) {
        row[offset..offset + 8].copy_from_slice(&tag.0.to_le_bytes());
    }
    let appearance = seed.canonical.appearance;
    row[0x140] = appearance.colour;
    row[0x141] = appearance.voice;
    row[0x142..0x146].copy_from_slice(&appearance.specials);
    row[0x148..0x1a0].copy_from_slice(&seed.charinfo);
    let (_, checked) = parse(&row)?;
    if checked.canonical() != seed.canonical { return Err("Editor seed differs from selected Mii"); }
    Ok(row)
}

pub fn parse(row: &Row) -> Result<(String, Configuration), &'static str> {
    let raw = saved::parse_header(0, 0, row).map_err(|_| "Invalid owned editor row")?;
    let config = inventory::classify(&raw).map_err(|_| "Invalid edited Mii")?;
    let value = Configuration::from_saved(&config).map_err(|_| "Edited Mii resources differ")?;
    Ok((config.name, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{classify, RawSlot, MiiClass, TaggedId, CHARINFO_LEN, TAG_CHARA, TAG_HAT, TAG_OUTFIT};
    fn seed() -> MiiConfig {
        classify(&RawSlot { index: 0, slot_id: 2622, name_len: 1,
            name_units: [65, 0, 0, 0, 0, 0, 0, 0, 0, 0], colour: 1, voice: 37,
            specials: [2, 1, 0, 2], charinfo: [7; CHARINFO_LEN],
            tag_outfit: TaggedId((u64::from(TAG_OUTFIT) << 56) | (1 << 40) | 0x1bfef7a5f7),
            tag_hat: TaggedId((u64::from(TAG_HAT) << 56) | 0x0fa9efa5ea),
            tag_chara: TaggedId((u64::from(TAG_CHARA) << 56) | MiiClass::Swordfighter.chara_hash40()),
        }).unwrap()
    }
    #[test]
    fn owned_editor_row_drops_native_id_and_keeps_configuration() {
        let seed = seed(); let before = seed.clone();
        let row = from_seed(&seed).unwrap();
        assert_eq!(&row[..12], &[0; 12]);
        let (name, value) = parse(&row).unwrap();
        assert_eq!(name, seed.name);
        assert_eq!(value.canonical(), seed.canonical);
        assert_eq!(value.charinfo(), &seed.charinfo);
        assert_eq!(value.tokens().outfit, seed.outfit_hint());
        assert_eq!(value.tokens().hat, seed.hat_hint());
        assert_eq!(seed, before);
        let mut native_id = row;
        native_id[..4].copy_from_slice(&2622u32.to_le_bytes());
        assert!(parse(&native_id).is_err());
    }
    #[test]
    fn editor_name_boundary_never_splits_utf16_character() {
        let mut seed = seed();
        seed.name = "123456789😀".into();
        let row = from_seed(&seed).unwrap();
        assert_eq!(parse(&row).unwrap().0, "123456789");
        seed.name = "12345678😀".into();
        assert_eq!(parse(&from_seed(&seed).unwrap()).unwrap().0, seed.name);
        seed.name.clear();
        assert_eq!(parse(&from_seed(&seed).unwrap()).unwrap().0, "New Mii");
    }
    #[test]
    fn edited_row_keeps_valid_changes_and_refuses_bad_specials() {
        let seed = seed(); let mut row = from_seed(&seed).unwrap();
        row[0x142..0x146].copy_from_slice(&[0, 2, 2, 1]);
        let (_, edited) = parse(&row).unwrap();
        assert_eq!(edited.tokens().specials, [0, 2, 2, 1]);
        assert_ne!(edited.canonical(), seed.canonical);
        row[0x142] = 3;
        assert!(parse(&row).is_err());
    }
}
