//! Portable practice setups: the value-only pair of saved fighter states that
//! one Training save-state slot holds, as a named, versioned library file.
//!
//! A setup records *where and how* two fighters start: position, facing,
//! damage, situation, special charges, Steve materials, Player 1's held item
//! and, optionally, each role's exact Mii configuration. It carries no
//! execution state (the save-state machine's `state`), no slot index, no
//! battle-object id and no native pointer, so a file written by one console or
//! process is meaningful on another. The runtime copies these values into its
//! own slot pair and drives the ordinary restore. It also admits
//! `situation_kind` against the game's ground/air/cliff constants and decides
//! what a `stage_id` mismatch means; this crate knows neither.
//!
//! Files live under [`crate::SETUP_LIBRARY_DIR`] as `<id>.json` through the
//! same generic [`Store`] the named-macro library uses: atomic replacement,
//! path-safe ids, bounded size and version-aware errors.

use std::fmt;

use serde::{Deserialize, Serialize};
use training_mod_macros::{
    Listing, MacroError, Record, Store, MAX_GAME_VERSION_BYTES, MAX_NAME_CHARS,
};
use training_mod_mii::lifetime::Configuration;

use crate::character_items::HeldItemSnapshot;

/// Current setup file schema.
pub const SETUP_SCHEMA_VERSION: u32 = 1;
/// Largest setup file read. A compact setup is about 2 KB.
pub const MAX_SETUP_FILE_BYTES: u64 = 64 * 1024;
/// Highest fighter kind a saved state may name: `FIGHTER_KIND_TRAIL` (Sora),
/// the last playable kind in 13.0.x. Pokémon, Pyra/Mythra and the Mii class
/// kinds all fall inside `0..=MAX_FIGHTER_KIND`. Roster-menu membership is
/// deliberately not required: the Mii classes join that table only behind a
/// feature, yet a Mii CPU is an ordinary saved fighter.
pub const MAX_FIGHTER_KIND: i32 = 0x5d;
/// Inclusive damage bound. The game caps displayed damage at 999.9%.
pub const MAX_PERCENT: f32 = 1000.0;
/// Sanity bound for Steve tool material kinds (`FighterPickelMaterialKind`),
/// which the runtime writes straight into native tool records on load.
pub const MAX_STEVE_MATERIAL_KIND: u32 = 15;

/// Robin's book and Levin Sword durabilities. Values only.
#[derive(Serialize, Deserialize, Default, Copy, Clone, Debug, PartialEq)]
pub struct RobinCharges {
    pub thunder_kind: i32,
    pub yellow_book: i32,
    pub red_book: f32,
    pub green_book: i32,
    pub purple_book: i32,
    pub levin_charge: i32,
    pub levin_recharge: i32,
    pub has_levin: bool,
}

impl RobinCharges {
    /// Finite values only; the counts keep the fighter's own semantics.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.red_book.is_finite() {
            Ok(())
        } else {
            Err("Robin red book charge is not finite")
        }
    }
}

/// Special-move charge values captured by the save state. The meaning of each
/// slot depends on the fighter; the serialized shape is shared with
/// `save_states.toml` and must stay stable.
#[derive(Serialize, Deserialize, Default, Copy, Clone, Debug, PartialEq)]
pub struct ChargeState {
    pub int_x: Option<i32>,
    pub int_y: Option<i32>,
    pub int_z: Option<i32>,
    pub float_x: Option<f32>,
    pub float_y: Option<f32>,
    pub float_z: Option<f32>,
    pub has_charge: Option<bool>,
    pub robin_charges: Option<RobinCharges>,
}

impl ChargeState {
    pub fn int_x(mut self, int_x: i32) -> Self {
        self.int_x = Some(int_x);
        self
    }

    pub fn int_y(mut self, int_y: i32) -> Self {
        self.int_y = Some(int_y);
        self
    }

    pub fn int_z(mut self, int_z: i32) -> Self {
        self.int_z = Some(int_z);
        self
    }

    pub fn float_x(mut self, float_x: f32) -> Self {
        self.float_x = Some(float_x);
        self
    }

    pub fn float_y(mut self, float_y: f32) -> Self {
        self.float_y = Some(float_y);
        self
    }

    pub fn float_z(mut self, float_z: f32) -> Self {
        self.float_z = Some(float_z);
        self
    }

    pub fn set_pikmin(
        mut self,
        pikmin_1: Option<i32>,
        pikmin_2: Option<i32>,
        pikmin_3: Option<i32>,
    ) -> Self {
        self.int_x = pikmin_1;
        self.int_y = pikmin_2;
        self.int_z = pikmin_3;
        self
    }

    pub fn set_robin(mut self, robin_charges: RobinCharges) -> Self {
        self.robin_charges = Some(robin_charges);
        self
    }

    pub fn has_charge(mut self, has_charge: bool) -> Self {
        self.has_charge = Some(has_charge);
        self
    }

    /// Finite values only; the integer meanings are per fighter.
    pub fn validate(&self) -> Result<(), &'static str> {
        let floats = [
            (self.float_x, "charge float_x is not finite"),
            (self.float_y, "charge float_y is not finite"),
            (self.float_z, "charge float_z is not finite"),
        ];
        for (value, why) in floats {
            if value.is_some_and(|v| !v.is_finite()) {
                return Err(why);
            }
        }
        match &self.robin_charges {
            Some(robin) => robin.validate(),
            None => Ok(()),
        }
    }
}

/// Steve's material counts and tool records. The `*_mat` fields hold a native
/// `FighterPickelMaterialKind`; `char` is the same size and serializes stably.
#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
pub struct SteveState {
    pub mat_g1: i32,
    pub mat_wood: i32,
    pub mat_stone: i32,
    pub mat_iron: i32,
    pub mat_gold: i32,
    pub mat_redstone: i32,
    pub mat_diamond: i32,
    pub sword_mat: char,
    pub sword_durability: f32,
    pub axe_mat: char,
    pub axe_durability: f32,
    pub pick_mat: char,
    pub pick_durability: f32,
    pub shovel_mat: char,
    pub shovel_durability: f32,
}

impl SteveState {
    /// Non-negative counts, bounded tool kinds, finite non-negative durability.
    pub fn validate(&self) -> Result<(), &'static str> {
        let counts = [
            self.mat_g1,
            self.mat_wood,
            self.mat_stone,
            self.mat_iron,
            self.mat_gold,
            self.mat_redstone,
            self.mat_diamond,
        ];
        if counts.iter().any(|count| *count < 0) {
            return Err("Steve material count is negative");
        }
        let tools = [
            (self.sword_mat, self.sword_durability),
            (self.axe_mat, self.axe_durability),
            (self.pick_mat, self.pick_durability),
            (self.shovel_mat, self.shovel_durability),
        ];
        for (material, durability) in tools {
            if material as u32 > MAX_STEVE_MATERIAL_KIND {
                return Err("Steve tool material kind is out of range");
            }
            if !durability.is_finite() || durability < 0.0 {
                return Err("Steve tool durability is not a finite non-negative number");
            }
        }
        Ok(())
    }
}

/// Which half of the pair a value belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Player,
    Cpu,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Player => "Player 1",
            Self::Cpu => "CPU",
        })
    }
}

/// One role's saved values. Field meanings match the runtime's saved state;
/// nothing here refers to a live object.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FighterSetup {
    /// Raw game fighter kind, `0..=MAX_FIGHTER_KIND`.
    pub fighter_kind: i32,
    pub x: f32,
    pub y: f32,
    pub percent: f32,
    /// Facing: `1.0` right, `-1.0` left.
    pub lr: f32,
    /// Raw game situation kind (ground/air/cliff), admitted by the runtime.
    pub situation_kind: i32,
    pub charge: ChargeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steve_state: Option<SteveState>,
    /// Player 1 only in current saves; the CPU stores `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_item: Option<HeldItemSnapshot>,
    /// Exact Mii identity for this role. The saving runtime attaches it only
    /// with proof that this role's live fighter carried it when the slot was
    /// saved; it is never inferred from an unrelated fighter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mii: Option<Configuration>,
}

fn invalid(role: Role, why: &'static str) -> SetupError {
    SetupError::InvalidFighter { role, why }
}

impl FighterSetup {
    /// Finite, in-range values for one role. Situation-kind membership is the
    /// runtime's check.
    pub fn validate(&self, role: Role) -> Result<(), SetupError> {
        if !(0..=MAX_FIGHTER_KIND).contains(&self.fighter_kind) {
            return Err(invalid(role, "fighter kind is outside the roster range"));
        }
        let floats = [
            (self.x, "x position is not finite"),
            (self.y, "y position is not finite"),
            (self.percent, "damage is not finite"),
            (self.lr, "facing is not finite"),
        ];
        for (value, why) in floats {
            if !value.is_finite() {
                return Err(invalid(role, why));
            }
        }
        if !(0.0..=MAX_PERCENT).contains(&self.percent) {
            return Err(invalid(role, "damage is outside 0 to 1000"));
        }
        if self.lr != 1.0 && self.lr != -1.0 {
            return Err(invalid(role, "facing must be -1 or 1"));
        }
        self.charge.validate().map_err(|why| invalid(role, why))?;
        if let Some(steve) = &self.steve_state {
            steve.validate().map_err(|why| invalid(role, why))?;
        }
        if let Some(item) = &self.held_item {
            if !item.valid() {
                return Err(invalid(role, "held item snapshot is not valid"));
            }
        }
        if let Some(mii) = &self.mii {
            if mii.canonical().class.kind() != self.fighter_kind as u32 {
                return Err(invalid(
                    role,
                    "exact Mii configuration belongs to a different fighter class",
                ));
            }
        } else if (0x48..=0x4a).contains(&self.fighter_kind) {
            return Err(invalid(role, "exact Mii identity missing; save this slot again"));
        }
        Ok(())
    }
}

/// A named, versioned, portable practice setup: both roles of one slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupRecord {
    pub schema_version: u32,
    pub name: String,
    /// Free-form tag of the game version it was saved on; may be empty.
    pub game_version: String,
    /// Stage the positions were saved on, when the runtime knew it. Advisory:
    /// the runtime decides whether to request that stage or refuse a load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<i32>,
    pub player: FighterSetup,
    pub cpu: FighterSetup,
}

impl SetupRecord {
    /// Builds a validated record; the name is trimmed.
    pub fn new(
        name: impl Into<String>,
        game_version: impl Into<String>,
        stage_id: Option<i32>,
        player: FighterSetup,
        cpu: FighterSetup,
    ) -> Result<Self, SetupError> {
        let name: String = name.into();
        let record = Self {
            schema_version: SETUP_SCHEMA_VERSION,
            name: name.trim().to_owned(),
            game_version: game_version.into(),
            stage_id,
            player,
            cpu,
        };
        record.validate()?;
        Ok(record)
    }

    /// Everything the library requires before it writes or accepts a file.
    pub fn validate(&self) -> Result<(), SetupError> {
        if self.schema_version != SETUP_SCHEMA_VERSION {
            return Err(SetupError::Store(MacroError::UnsupportedVersion {
                found: self.schema_version,
            }));
        }
        check_name(&self.name)?;
        if self.game_version.len() > MAX_GAME_VERSION_BYTES {
            return Err(SetupError::Invalid("game version is too long"));
        }
        if self.game_version.chars().any(char::is_control) {
            return Err(SetupError::Invalid("game version contains control characters"));
        }
        if self.stage_id.is_some_and(|stage| stage < 0) {
            return Err(SetupError::Invalid("stage id is negative"));
        }
        self.player.validate(Role::Player)?;
        self.cpu.validate(Role::Cpu)
    }

    /// Renames the setup (trimmed and validated); the fighters are untouched.
    pub fn set_name(&mut self, name: &str) -> Result<(), SetupError> {
        let name = name.trim();
        check_name(name)?;
        self.name = name.to_owned();
        Ok(())
    }

    pub fn role(&self, role: Role) -> &FighterSetup {
        match role {
            Role::Player => &self.player,
            Role::Cpu => &self.cpu,
        }
    }
}

fn check_name(name: &str) -> Result<(), SetupError> {
    if name.trim().is_empty() {
        return Err(SetupError::Invalid("name is empty"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(SetupError::Invalid("name is too long"));
    }
    if name.chars().any(char::is_control) {
        return Err(SetupError::Invalid("name contains control characters"));
    }
    Ok(())
}

/// Listing row for one stored setup; enough to pick, label or refuse it
/// without reading the payload again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupEntry {
    pub id: String,
    pub name: String,
    pub game_version: String,
    pub player_kind: i32,
    pub cpu_kind: i32,
    pub stage_id: Option<i32>,
    /// Whether the player and CPU halves carry an exact Mii configuration.
    pub exact_mii: [bool; 2],
}

/// Every failure the setup library reports.
#[derive(Debug)]
pub enum SetupError {
    /// Shared store failure: I/O, id, size, parse or version.
    Store(MacroError),
    /// A record-level field (schema, name, version, stage) is unacceptable.
    Invalid(&'static str),
    /// One role's saved values are unacceptable.
    InvalidFighter { role: Role, why: &'static str },
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(MacroError::UnsupportedVersion { found }) => write!(
                f,
                "unsupported setup schema version {} (supported: {})",
                found, SETUP_SCHEMA_VERSION
            ),
            Self::Store(MacroError::Parse(err)) => write!(f, "malformed setup file: {}", err),
            Self::Store(MacroError::InvalidId(id)) => write!(f, "invalid setup id {:?}", id),
            Self::Store(MacroError::NotFound(id)) => write!(f, "no setup with id {:?}", id),
            Self::Store(MacroError::NoFreeId) => f.write_str("no free setup id for this name"),
            Self::Store(err) => write!(f, "{}", err),
            Self::Invalid(why) => write!(f, "invalid setup: {}", why),
            Self::InvalidFighter { role, why } => write!(f, "invalid setup: {} {}", role, why),
        }
    }
}

impl std::error::Error for SetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(err) => Some(err),
            _ => None,
        }
    }
}

impl From<MacroError> for SetupError {
    fn from(err: MacroError) -> Self {
        Self::Store(err)
    }
}

impl Record for SetupRecord {
    type Error = SetupError;
    type Summary = SetupEntry;
    const SCHEMA_VERSIONS: &'static [u32] = &[SETUP_SCHEMA_VERSION];
    const MAX_FILE_BYTES: u64 = MAX_SETUP_FILE_BYTES;

    fn validate(&self) -> Result<(), SetupError> {
        SetupRecord::validate(self)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn summary(&self, id: &str) -> SetupEntry {
        SetupEntry {
            id: id.to_owned(),
            name: self.name.clone(),
            game_version: self.game_version.clone(),
            player_kind: self.player.fighter_kind,
            cpu_kind: self.cpu.fighter_kind,
            stage_id: self.stage_id,
            exact_mii: [self.player.mii.is_some(), self.cpu.mii.is_some()],
        }
    }
}

/// The setup library, normally rooted at [`crate::SETUP_LIBRARY_DIR`].
pub type SetupLibrary = Store<SetupRecord>;
/// A scan of the setup library.
pub type SetupListing = Listing<SetupEntry>;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU32, Ordering},
    };

    use serde_json::{json, Value};

    use super::*;
    use crate::trio::CHOICES;

    /// A unique directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "training_mod_consts-setup-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn exact_mii() -> Configuration {
        use training_mod_mii::inventory::{
            classify, MiiClass, RawSlot, TaggedId, CHARINFO_LEN, TAG_CHARA, TAG_HAT, TAG_OUTFIT,
        };
        let saved = classify(&RawSlot {
            index: 0,
            slot_id: 13231,
            name_len: 1,
            name_units: [65, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            charinfo: [7; CHARINFO_LEN],
            colour: 4,
            voice: 37,
            specials: [1, 0, 0, 0],
            tag_outfit: TaggedId((u64::from(TAG_OUTFIT) << 56) | (1 << 40) | 0x1b_fef7_a5f7),
            tag_hat: TaggedId((u64::from(TAG_HAT) << 56) | 0x0f_a9ef_a5ea),
            tag_chara: TaggedId((u64::from(TAG_CHARA) << 56) | MiiClass::Brawler.chara_hash40()),
        })
        .unwrap();
        Configuration::from_saved(&saved).unwrap()
    }

    fn charge() -> ChargeState {
        ChargeState::default().int_x(3).float_y(1.5).has_charge(true)
    }

    fn robin() -> ChargeState {
        ChargeState::default().set_robin(RobinCharges {
            thunder_kind: 2,
            yellow_book: 1,
            red_book: 0.5,
            green_book: 3,
            purple_book: 0,
            levin_charge: 4,
            levin_recharge: 0,
            has_levin: true,
        })
    }

    fn steve() -> SteveState {
        SteveState {
            mat_g1: 36,
            mat_wood: 18,
            mat_stone: 0,
            mat_iron: 3,
            mat_gold: 0,
            mat_redstone: 2,
            mat_diamond: 0,
            sword_mat: 1 as char,
            sword_durability: 25.0,
            axe_mat: 1 as char,
            axe_durability: 70.0,
            pick_mat: 1 as char,
            pick_durability: 70.0,
            shovel_mat: 1 as char,
            shovel_durability: 70.0,
        }
    }

    fn player() -> FighterSetup {
        FighterSetup {
            fighter_kind: 0x07,
            x: -12.5,
            y: 0.0,
            percent: 42.5,
            lr: -1.0,
            situation_kind: 0,
            charge: charge(),
            steve_state: None,
            held_item: Some(HeldItemSnapshot::Item {
                choice: 25,
                variation: 7,
                lifetime: 100,
                lifetime_max: 600,
                fuse: -1,
                fuse_max: -1,
            }),
            mii: None,
        }
    }

    fn cpu() -> FighterSetup {
        FighterSetup {
            fighter_kind: 0x48,
            x: 30.0,
            y: 12.0,
            percent: 0.0,
            lr: 1.0,
            situation_kind: 2,
            charge: ChargeState::default(),
            steve_state: None,
            held_item: None,
            mii: Some(exact_mii()),
        }
    }

    fn setup(name: &str) -> SetupRecord {
        SetupRecord::new(name, "13.0.5", Some(0x40), player(), cpu()).unwrap()
    }

    fn case(
        mutate: fn(&mut SetupRecord),
        role: Role,
        why: &'static str,
    ) -> (fn(&mut SetupRecord), Role, &'static str) {
        (mutate, role, why)
    }

    #[test]
    fn max_fighter_kind_is_the_roster_tables_highest_kind() {
        let highest = CHOICES.iter().map(|(kind, _)| *kind as i32).max().unwrap();
        assert_eq!(highest, MAX_FIGHTER_KIND);
        for kind in [0x24, 0x26, 0x48, 0x4a, 0x5a, 0x5b] {
            assert!((0..=MAX_FIGHTER_KIND).contains(&kind), "{:#x}", kind);
        }
    }

    #[test]
    fn json_is_versioned_values_only_and_round_trips() {
        let record = setup("Fox ledge drill");
        let value: Value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["schema_version"], json!(1));
        assert_eq!(value["name"], json!("Fox ledge drill"));
        assert_eq!(value["stage_id"], json!(0x40));
        for role in ["player", "cpu"] {
            let half = value[role].as_object().unwrap();
            assert!(!half.contains_key("state"), "{}: no restore machine state", role);
            for key in ["fighter_kind", "x", "y", "percent", "lr", "situation_kind", "charge"] {
                assert!(half.contains_key(key), "{}.{}", role, key);
            }
        }
        assert_eq!(value["player"]["held_item"]["state"], json!("item"));
        assert!(value["player"].get("mii").is_none(), "absent identity is omitted, not null");
        assert!(value["player"].get("steve_state").is_none());
        assert!(value["cpu"]["mii"].is_object());
        assert!(value["cpu"].get("held_item").is_none());

        let bytes = serde_json::to_vec(&record).unwrap();
        assert!((bytes.len() as u64) < MAX_SETUP_FILE_BYTES / 8, "{} bytes", bytes.len());
        assert_eq!(serde_json::from_slice::<SetupRecord>(&bytes).unwrap(), record);

        // Absent optional fields read back as None rather than failing.
        let mut trimmed = value.clone();
        trimmed.as_object_mut().unwrap().remove("stage_id");
        let parsed: SetupRecord = serde_json::from_value(trimmed).unwrap();
        assert_eq!(parsed.stage_id, None);
        assert_eq!(parsed.player, record.player);
        assert_eq!(parsed.cpu, record.cpu);
    }

    #[test]
    fn every_mii_class_requires_capture_time_identity_for_portable_setups() {
        for kind in 0x48..=0x4a {
            for role in [Role::Player, Role::Cpu] {
                let missing = FighterSetup { fighter_kind: kind, mii: None, ..player() };
                assert!(matches!(missing.validate(role), Err(SetupError::InvalidFighter {
                    why: "exact Mii identity missing; save this slot again", ..
                })));
            }
        }
        assert!(player().validate(Role::Player).is_ok());
        assert!(cpu().validate(Role::Cpu).is_ok());
    }

    #[test]
    fn library_round_trip_listing_and_stable_ids() {
        let tmp = TempDir::new();
        let dir = tmp.path().join("setups");
        let record = setup("Fox ledge drill");
        let id = SetupLibrary::new(&dir).save_new(&record).unwrap();
        assert_eq!(id, "fox-ledge-drill");

        // A new instance on the same path, as after a game restart.
        let reopened = SetupLibrary::new(&dir);
        assert_eq!(reopened.load(&id).unwrap(), record);
        let listing: SetupListing = reopened.list().unwrap();
        assert_eq!(
            listing.entries,
            vec![SetupEntry {
                id: id.clone(),
                name: "Fox ledge drill".to_owned(),
                game_version: "13.0.5".to_owned(),
                player_kind: 0x07,
                cpu_kind: 0x48,
                stage_id: Some(0x40),
                exact_mii: [false, true],
            }]
        );
        assert!(listing.skipped.is_empty());
        assert_eq!(file_names(&dir), ["fox-ledge-drill.json"], "no temp or backup left");

        // A rename through overwrite keeps the id and both fighters.
        let mut renamed = reopened.load(&id).unwrap();
        renamed.set_name("  Fox ledge drill v2 ").unwrap();
        reopened.overwrite(&id, &renamed).unwrap();
        let after = reopened.load(&id).unwrap();
        assert_eq!(after.name, "Fox ledge drill v2");
        assert_eq!(after.player, record.player);
        assert_eq!(after.cpu, record.cpu);
        assert_eq!(reopened.save_new(&record).unwrap(), "fox-ledge-drill-2");
    }

    #[test]
    fn fighter_validation_names_the_role_and_rejects_bad_values() {
        let cases = vec![
            case(|s| s.cpu.fighter_kind = -1, Role::Cpu, "fighter kind"),
            case(|s| s.player.fighter_kind = MAX_FIGHTER_KIND + 1, Role::Player, "fighter kind"),
            case(|s| s.player.lr = 0.5, Role::Player, "facing"),
            case(|s| s.cpu.lr = f32::NAN, Role::Cpu, "facing"),
            case(|s| s.player.percent = -0.5, Role::Player, "damage"),
            case(|s| s.cpu.percent = 1000.5, Role::Cpu, "damage"),
            case(|s| s.player.x = f32::NAN, Role::Player, "x position"),
            case(|s| s.cpu.y = f32::INFINITY, Role::Cpu, "y position"),
            case(|s| s.player.charge.float_x = Some(f32::NAN), Role::Player, "float_x"),
            case(
                |s| {
                    s.cpu.charge = ChargeState::default().set_robin(RobinCharges {
                        red_book: f32::NAN,
                        ..RobinCharges::default()
                    })
                },
                Role::Cpu,
                "red book",
            ),
            case(
                |s| s.player.steve_state = Some(SteveState { mat_wood: -1, ..steve() }),
                Role::Player,
                "material count",
            ),
            case(
                |s| s.player.steve_state = Some(SteveState { axe_durability: f32::NAN, ..steve() }),
                Role::Player,
                "durability",
            ),
            case(
                |s| s.player.steve_state = Some(SteveState { pick_mat: 200u8 as char, ..steve() }),
                Role::Player,
                "material kind",
            ),
            case(
                |s| {
                    s.player.held_item = Some(HeldItemSnapshot::Item {
                        choice: 11,
                        variation: 3,
                        lifetime: 0,
                        lifetime_max: 0,
                        fuse: -1,
                        fuse_max: -1,
                    })
                },
                Role::Player,
                "held item",
            ),
            // A Brawler configuration on a Swordfighter kind.
            case(|s| s.cpu.fighter_kind = 0x49, Role::Cpu, "different fighter class"),
        ];
        for (mutate, role, why) in cases {
            let mut record = setup("Case");
            mutate(&mut record);
            let err = record.validate().unwrap_err();
            assert!(
                matches!(err, SetupError::InvalidFighter { role: found, .. } if found == role),
                "{}: {}", why, err
            );
            let text = err.to_string();
            assert!(text.contains(why) && text.contains(&role.to_string()), "{}", text);
        }
        // The same values are accepted when they are in range.
        let mut steve_player = player();
        steve_player.fighter_kind = 0x58;
        steve_player.steve_state = Some(steve());
        steve_player.charge = robin();
        assert!(steve_player.validate(Role::Player).is_ok());
        let mut mii_player = player();
        mii_player.fighter_kind = 0x48;
        mii_player.mii = Some(exact_mii());
        assert!(mii_player.validate(Role::Player).is_ok());
    }

    #[test]
    fn record_validation_covers_schema_name_version_and_stage() {
        let mut record = setup("Ok");
        record.schema_version = 2;
        let err = record.validate().unwrap_err();
        assert!(matches!(err, SetupError::Store(MacroError::UnsupportedVersion { found: 2 })));
        assert_eq!(err.to_string(), "unsupported setup schema version 2 (supported: 1)");

        for (name, why) in [("   ", "empty"), ("x\ny", "control")] {
            assert!(
                matches!(
                    SetupRecord::new(name, "13.0.5", None, player(), cpu()),
                    Err(SetupError::Invalid(found)) if found.contains(why)
                ),
                "{:?}", name
            );
        }
        let long = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(
            SetupRecord::new(long.as_str(), "13.0.5", None, player(), cpu()),
            Err(SetupError::Invalid("name is too long"))
        ));
        let version = "v".repeat(MAX_GAME_VERSION_BYTES + 1);
        assert!(matches!(
            SetupRecord::new("Ok", version.as_str(), None, player(), cpu()),
            Err(SetupError::Invalid("game version is too long"))
        ));
        assert!(matches!(
            SetupRecord::new("Ok", "13.0.5", Some(-1), player(), cpu()),
            Err(SetupError::Invalid("stage id is negative"))
        ));

        // Trimming, a 48-scalar name, an empty version and no stage are fine.
        let padded = format!("  {}  ", "é".repeat(MAX_NAME_CHARS));
        let mut ok = SetupRecord::new(padded, "", None, player(), cpu()).unwrap();
        assert_eq!(ok.name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(ok.stage_id, None);
        assert!(matches!(ok.set_name("  "), Err(SetupError::Invalid("name is empty"))));
        assert_eq!(ok.name.chars().count(), MAX_NAME_CHARS, "a rejected rename keeps the name");
        assert_eq!(ok.role(Role::Cpu).fighter_kind, 0x48);
        assert_eq!(ok.role(Role::Player).fighter_kind, 0x07);
    }

    #[test]
    fn store_reports_version_size_and_corruption_and_touches_nothing() {
        let tmp = TempDir::new();
        let library = SetupLibrary::new(tmp.path());
        let good_id = library.save_new(&setup("Good")).unwrap();
        let mut future = serde_json::to_value(setup("Future")).unwrap();
        future["schema_version"] = json!(2);
        let bad: Vec<(&str, Vec<u8>)> = vec![
            ("future.json", serde_json::to_vec(&future).unwrap()),
            ("big.json", vec![b' '; MAX_SETUP_FILE_BYTES as usize + 1]),
            ("garbage.json", b"{ not json".to_vec()),
            ("Bad Name.json", serde_json::to_vec(&setup("Bad")).unwrap()),
        ];
        for (name, bytes) in &bad {
            fs::write(tmp.path().join(name), bytes).unwrap();
        }

        assert!(matches!(
            library.load("future"),
            Err(SetupError::Store(MacroError::UnsupportedVersion { found: 2 }))
        ));
        assert!(matches!(
            library.load("big"),
            Err(SetupError::Store(MacroError::FileTooLarge { max: MAX_SETUP_FILE_BYTES, .. }))
        ));
        assert!(matches!(library.load("garbage"), Err(SetupError::Store(MacroError::Parse(_)))));
        assert!(matches!(library.load("nothing"), Err(SetupError::Store(MacroError::NotFound(_)))));
        assert_eq!(library.load("nothing").unwrap_err().to_string(), "no setup with id \"nothing\"");

        let listing = library.list().unwrap();
        let ids: Vec<&str> = listing.entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, [good_id.as_str()]);
        let reason = |file: &str| {
            listing.skipped.iter().find(|s| s.file == file).unwrap().reason.clone()
        };
        assert_eq!(reason("future.json"), "unsupported setup schema version 2 (supported: 1)");
        assert!(reason("big.json").contains("bytes"), "{}", reason("big.json"));
        assert!(reason("garbage.json").starts_with("malformed setup file"), "{}", reason("garbage.json"));
        assert!(reason("Bad Name.json").starts_with("invalid setup id"), "{}", reason("Bad Name.json"));

        // Reporting must not modify, move or delete anything.
        for (name, bytes) in &bad {
            assert_eq!(&fs::read(tmp.path().join(name)).unwrap(), bytes, "{name}");
        }
        assert_eq!(library.load(&good_id).unwrap(), setup("Good"));
    }

    #[test]
    fn overwrite_failure_keeps_the_previous_setup() {
        let tmp = TempDir::new();
        let library = SetupLibrary::new(tmp.path());
        let id = library.save_new(&setup("Keep")).unwrap();
        let path = tmp.path().join(format!("{}.json", id));
        let before = fs::read(&path).unwrap();

        // Occupy the temp path with a directory so the new data cannot be written.
        fs::create_dir(tmp.path().join(format!("{}.json.tmp", id))).unwrap();
        assert!(matches!(
            library.overwrite(&id, &setup("Changed")),
            Err(SetupError::Store(MacroError::Io(_)))
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(library.load(&id).unwrap(), setup("Keep"));

        // An invalid replacement never reaches the disk either.
        let mut broken = setup("Broken");
        broken.player.lr = 0.0;
        assert!(matches!(
            library.overwrite(&id, &broken),
            Err(SetupError::InvalidFighter { role: Role::Player, .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(matches!(
            library.overwrite("ghost", &setup("Ghost")),
            Err(SetupError::Store(MacroError::NotFound(_)))
        ));
    }

    #[test]
    fn moved_value_types_keep_their_toml_shape() {
        // `save_states.toml` is written with toml::to_string_pretty of these types.
        let with_robin = robin();
        let text = toml::to_string_pretty(&with_robin).unwrap();
        assert!(text.contains("[robin_charges]"), "{}", text);
        assert_eq!(toml::from_str::<ChargeState>(&text).unwrap(), with_robin);

        let legacy = "int_x = 3\nfloat_y = 1.5\nhas_charge = true\n";
        assert_eq!(toml::from_str::<ChargeState>(legacy).unwrap(), charge());
        assert_eq!(toml::from_str::<ChargeState>("").unwrap(), ChargeState::default());

        let steve_text = toml::to_string_pretty(&steve()).unwrap();
        assert!(steve_text.contains("sword_mat"), "{}", steve_text);
        assert_eq!(toml::from_str::<SteveState>(&steve_text).unwrap(), steve());

        assert!(charge().validate().is_ok());
        assert!(with_robin.validate().is_ok());
        assert!(steve().validate().is_ok());
    }
}
