//! Mod-owned drafts and presets. No native-save IDs, game pointers or file writes.
//! Preview adapters still need current native ownership/row validation before loading a value.
use crate::inventory::{Canonical, MiiConfig, Snapshot, TaggedId, CHARINFO_LEN};
use crate::lifetime::Configuration;
use serde::{Deserialize, Serialize};

pub const MAX_NAME_CHARS: usize = 48;
pub const MAX_PRESETS: usize = 256;
const SCHEMA: u32 = 1;

#[derive(Clone)]
pub struct Draft {
    template: MiiConfig,
    specials: [u8; 4],
}

impl Draft {
    /// An existing qualified appearance is copied; editing never changes its native save.
    pub fn from_template(template: &MiiConfig) -> Result<Self, &'static str> {
        Configuration::from_saved(template).map_err(|_| "Invalid Mii appearance template")?;
        Ok(Self { template: template.clone(), specials: template.canonical.appearance.specials })
    }

    pub fn specials(&self) -> [u8; 4] { self.specials }

    pub fn set_specials(&mut self, specials: [u8; 4]) -> Result<(), &'static str> {
        if specials.iter().any(|value| *value > 2) {
            return Err("Choose one of the three moves for each special");
        }
        self.specials = specials;
        Ok(())
    }

    fn value(&self) -> MiiConfig {
        let mut value = self.template.clone();
        value.canonical.appearance.specials = self.specials;
        value
    }

    /// Owned configuration only. This does not submit a swap or authorize native writes.
    pub fn configuration(&self) -> Result<Configuration, &'static str> {
        Configuration::from_saved(&self.value()).map_err(|_| "Invalid Mii draft")
    }

    fn preset(&self, name: &str) -> Result<Preset, &'static str> {
        Preset::new(name, self.configuration()?, self.template.raw_tags)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Preset {
    name: String,
    configuration: Configuration,
    tags: [TaggedId; 3],
}

impl core::fmt::Debug for Preset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Preset").field("name", &self.name)
            .field("configuration", &self.configuration).finish_non_exhaustive()
    }
}

impl Preset {
    fn new(name: &str, configuration: Configuration, tags: [TaggedId; 3]) -> Result<Self, &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            return Err("Use a nonempty name of at most 48 characters without control characters");
        }
        Ok(Self { name: name.into(), configuration, tags })
    }

    pub fn name(&self) -> &str { &self.name }
    pub fn configuration(&self) -> Configuration { self.configuration }
    pub fn canonical(&self) -> Canonical { self.configuration.canonical() }

    fn stored(&self) -> StoredPreset {
        StoredPreset { name: self.name.clone(), canonical: self.canonical(),
            charinfo: self.configuration.charinfo().to_vec(), tags: self.tags.map(|tag| tag.0) }
    }
}

/// No native slot number is assigned to a mod preset. Integrating catalogs must preserve this
/// provenance instead of pretending a preset is a newly created native save file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    ExistingNative(Canonical),
    ExistingPreset(Canonical),
    Created(Canonical),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StoredLibrary", into = "StoredLibrary")]
pub struct Library {
    presets: Vec<Preset>,
}

impl Library {
    pub fn presets(&self) -> &[Preset] { &self.presets }

    /// First explicit save commits a draft. Native configurations win duplicate lookup and
    /// existing names stay unchanged. Dropping/cancelling a Draft performs no library mutation.
    pub fn save(&mut self, draft: &Draft, name: &str, native: &Snapshot) -> Result<Saved, &'static str> {
        let preset = draft.preset(name)?;
        self.insert(preset, native)
    }

    /// Import a resolved owned replay value, preserving native-first canonical deduplication.
    pub fn import(&mut self, configuration: Configuration, name: &str, native: &Snapshot) -> Result<Saved, &'static str> {
        use crate::inventory::{TAG_CHARA, TAG_HAT, TAG_OUTFIT};
        let appearance = configuration.canonical().appearance;
        let tokens = configuration.tokens();
        let tag = |kind, row, hash| TaggedId((u64::from(kind) << 56) | (u64::from(row) << 40) | hash);
        let tags = [tag(TAG_OUTFIT, tokens.outfit, appearance.outfit),
            tag(TAG_HAT, tokens.hat, appearance.hat), tag(TAG_CHARA, 0u16, appearance.chara)];
        self.insert(Preset::new(name, configuration, tags)?, native)
    }

    fn insert(&mut self, preset: Preset, native: &Snapshot) -> Result<Saved, &'static str> {
        if !native.is_complete() { return Err("Wait for a complete native Mii catalog"); }
        let canonical = preset.canonical();
        let equal = |candidate: Canonical| candidate.hash() == canonical.hash() && candidate == canonical;
        if native.configs.iter().any(|config| equal(config.canonical)) {
            return Ok(Saved::ExistingNative(canonical));
        }
        if self.presets.iter().any(|entry| equal(entry.canonical())) {
            return Ok(Saved::ExistingPreset(canonical));
        }
        if self.presets.len() == MAX_PRESETS { return Err("Mii preset library is full"); }
        self.presets.push(preset);
        Ok(Saved::Created(canonical))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredLibrary {
    schema: u32,
    presets: Vec<StoredPreset>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPreset {
    name: String,
    canonical: Canonical,
    charinfo: Vec<u8>,
    tags: [u64; 3],
}

impl TryFrom<StoredPreset> for Preset {
    type Error = &'static str;
    fn try_from(stored: StoredPreset) -> Result<Self, Self::Error> {
        let charinfo: [u8; CHARINFO_LEN] = stored.charinfo.try_into().map_err(|_| "Invalid Mii appearance length")?;
        let tags = stored.tags.map(TaggedId);
        // Reuse the value validator, not the saved-file reader. These zero placeholders never
        // leave this function or become a native save selector.
        let value = MiiConfig { slot_id: 0, slot_index: 0, name: stored.name.clone(), charinfo,
            raw_tags: tags, canonical: stored.canonical, copies: Vec::new() };
        let configuration = Configuration::from_saved(&value).map_err(|_| "Mii preset appearance or resource tags differ")?;
        Self::new(&stored.name, configuration, tags)
    }
}

impl TryFrom<StoredLibrary> for Library {
    type Error = &'static str;
    fn try_from(stored: StoredLibrary) -> Result<Self, Self::Error> {
        if stored.schema != SCHEMA { return Err("Unsupported Mii preset library version"); }
        if stored.presets.len() > MAX_PRESETS { return Err("Mii preset library exceeds its capacity"); }
        let mut presets: Vec<Preset> = Vec::with_capacity(stored.presets.len());
        for record in stored.presets {
            let preset = Preset::try_from(record)?;
            let id = preset.canonical();
            if !presets.iter().any(|p| p.canonical().hash() == id.hash() && p.canonical() == id) {
                presets.push(preset);
            }
        }
        Ok(Self { presets })
    }
}

impl From<Library> for StoredLibrary {
    fn from(library: Library) -> Self {
        Self { schema: SCHEMA, presets: library.presets.iter().map(Preset::stored).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{classify, RawSlot, MiiClass, TAG_CHARA, TAG_HAT, TAG_OUTFIT};

    fn template() -> MiiConfig {
        let class = MiiClass::Swordfighter;
        classify(&RawSlot { index: 0, slot_id: 2622, name_len: 1, name_units: [65, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            colour: 1, voice: 37, specials: [0; 4], charinfo: [7; CHARINFO_LEN],
            tag_outfit: TaggedId((u64::from(TAG_OUTFIT) << 56) | (1 << 40) | 0x1bfef7a5f7),
            tag_hat: TaggedId((u64::from(TAG_HAT) << 56) | 0x0fa9efa5ea),
            tag_chara: TaggedId((u64::from(TAG_CHARA) << 56) | class.chara_hash40()),
        }).unwrap()
    }

    #[test]
    fn draft_preview_changes_all_four_moves_without_changing_template_or_library() {
        let original = template();
        let before = original.clone();
        let library = Library::default();
        let mut draft = Draft::from_template(&original).unwrap();
        draft.set_specials([2, 1, 2, 1]).unwrap();
        let preview = draft.configuration().unwrap();
        assert_eq!(preview.tokens().specials, [2, 1, 2, 1]);
        assert_eq!(preview.charinfo(), &before.charinfo);
        assert_eq!(preview.canonical().class, before.canonical.class);
        assert_eq!(preview.tokens().outfit, before.outfit_hint());
        assert_eq!(original, before);
        drop(draft); // Cancel creates no saved choice and requires no rollback of the template.
        assert!(library.presets().is_empty());
    }

    #[test]
    fn invalid_special_edit_keeps_last_valid_draft() {
        let mut draft = Draft::from_template(&template()).unwrap();
        draft.set_specials([1; 4]).unwrap();
        assert!(draft.set_specials([0, 3, 0, 0]).is_err());
        assert_eq!(draft.specials(), [1; 4]);
    }

    #[test]
    fn native_and_mod_duplicates_reuse_identity_without_renaming() {
        let original = template();
        let native = Snapshot { configs: vec![original.clone()], ..Snapshot::default() };
        let mut draft = Draft::from_template(&original).unwrap();
        let mut library = Library::default();
        assert_eq!(library.save(&draft, "Different name", &native), Ok(Saved::ExistingNative(original.canonical)));
        assert!(library.presets().is_empty());
        draft.set_specials([1, 2, 1, 2]).unwrap();
        let id = draft.configuration().unwrap().canonical();
        assert_eq!(library.save(&draft, "First", &native), Ok(Saved::Created(id)));
        assert_eq!(library.save(&draft, "Second", &native), Ok(Saved::ExistingPreset(id)));
        assert_eq!(library.presets()[0].name(), "First");
        assert_eq!(native.configs[0], original);
    }

    #[test]
    fn saved_library_roundtrip_preserves_full_preview_configuration() {
        let mut draft = Draft::from_template(&template()).unwrap();
        draft.set_specials([2, 1, 0, 2]).unwrap();
        let mut library = Library::default();
        library.save(&draft, "  Épée  ", &Snapshot::default()).unwrap();
        let wire = serde_json::to_string(&library).unwrap();
        assert!(!wire.contains("slot_id"));
        let loaded: Library = serde_json::from_str(&wire).unwrap();
        assert_eq!(loaded, library);
        assert_eq!(loaded.presets()[0].name(), "Épée");
        assert_eq!(loaded.presets()[0].configuration(), draft.configuration().unwrap());
    }

    #[test]
    fn invalid_record_refuses_whole_library_instead_of_loading_partial_values() {
        let draft = Draft::from_template(&template()).unwrap();
        let mut library = Library::default();
        library.save(&draft, "Valid", &Snapshot::default()).unwrap();
        let wire = serde_json::to_value(&library).unwrap();
        for field in ["schema", "charinfo", "tags", "name"] {
            let mut bad = wire.clone();
            match field {
                "schema" => bad["schema"] = serde_json::json!(99),
                "charinfo" => bad["presets"][0]["charinfo"][40] = serde_json::json!(123),
                "tags" => bad["presets"][0]["tags"][0] = serde_json::json!(0),
                _ => bad["presets"][0]["name"] = serde_json::json!("\n"),
            }
            assert!(serde_json::from_value::<Library>(bad).is_err(), "{field}");
        }
        let mut duplicate = wire;
        let mut second = duplicate["presets"][0].clone();
        second["name"] = serde_json::json!("Another name");
        duplicate["presets"].as_array_mut().unwrap().push(second);
        let loaded: Library = serde_json::from_value(duplicate).unwrap();
        assert_eq!(loaded.presets().len(), 1);
        assert_eq!(loaded.presets()[0].name(), "Valid");
    }

    #[test]
    fn unsuccessful_save_keeps_existing_library_unchanged() {
        let mut draft = Draft::from_template(&template()).unwrap();
        let mut library = Library::default();
        library.save(&draft, "First", &Snapshot::default()).unwrap();
        let before = library.clone();
        draft.set_specials([1; 4]).unwrap();
        for name in ["".into(), "x".repeat(MAX_NAME_CHARS + 1), "Bad\nName".into()] {
            assert!(library.save(&draft, &name, &Snapshot::default()).is_err());
            assert_eq!(library, before);
        }
        let mut incomplete = Snapshot::default();
        incomplete.findings.push(crate::inventory::Finding::Source { slot_id: None, error: "unreadable".into() });
        assert!(library.save(&draft, "Valid name", &incomplete).is_err());
        assert_eq!(library, before);
    }
}
