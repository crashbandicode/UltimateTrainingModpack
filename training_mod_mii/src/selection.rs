//! Identity-aware selection over an owned catalog; no native actions or persistence writes.
use crate::inventory::{Canonical, CatalogEntry, MiiClass, Snapshot};

/// Character-select tile order.
pub const CLASSES: [MiiClass; 3] = [MiiClass::Brawler, MiiClass::Swordfighter, MiiClass::Gunner];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Player,
    Cpu,
}

/// P1's exact character-select configuration (slot 0), captured once per session by the roster
/// adapter when the entry is first ready. Never changed by this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin(pub Canonical);

/// Unknown is distinct from a known ordinary CSS fighter. The adapter must bind the original
/// once per scene before Player selections are actionable. CPU never excludes P1's original.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayerOrigin {
    #[default]
    Unresolved,
    NotMii,
    Mii(Origin),
}

impl PlayerOrigin {
    fn canonical(self) -> Option<Canonical> {
        match self { Self::Mii(origin) => Some(origin.0), _ => None }
    }
}


fn class_index(class: MiiClass) -> usize {
    match class {
        MiiClass::Brawler => 0,
        MiiClass::Swordfighter => 1,
        MiiClass::Gunner => 2,
    }
}

/// Persisted per-role, per-class preferences (identity = `Canonical`, stored by the menu layer as
/// its hex form). This module only reads them; `with` exists for the menu layer and tests.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "StoredPreferences")]
pub struct Preferences {
    player: [Option<Canonical>; 3],
    cpu: [Option<Canonical>; 3],
}

impl Preferences {
    pub fn get(&self, role: Role, class: MiiClass) -> Option<Canonical> {
        match role {
            Role::Player => self.player[class_index(class)],
            Role::Cpu => self.cpu[class_index(class)],
        }
    }

    pub fn with(mut self, role: Role, class: MiiClass, value: Option<Canonical>) -> Result<Self, String> {
        if let Some(value) = value {
            if value.class != class || Canonical::from_bytes(&value.to_bytes()) != Ok(value) {
                return Err("Mii preference does not match its class or canonical layout".into());
            }
        }
        match role {
            Role::Player => self.player[class_index(class)] = value,
            Role::Cpu => self.cpu[class_index(class)] = value,
        }
        Ok(self)
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StoredPreferences {
    player: [Option<Canonical>; 3],
    cpu: [Option<Canonical>; 3],
}

impl TryFrom<StoredPreferences> for Preferences {
    type Error = String;
    fn try_from(value: StoredPreferences) -> Result<Self, Self::Error> {
        let mut prefs = Self::default();
        for (role, values) in [(Role::Player, value.player), (Role::Cpu, value.cpu)] {
            for (class, value) in CLASSES.into_iter().zip(values) {
                prefs = prefs.with(role, class, value)?;
            }
        }
        Ok(prefs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    NoPreference,
    PreferenceIsOrigin,
    PreferenceUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Preference,
    FirstEligible(Fallback),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub canonical: Canonical,
    pub slot_id: Option<u32>,
    pub source: Source,
}

/// Eligible configurations of `class` for `role` in snapshot order: every deduplicated configuration
/// of that class, minus slot 0 for the player. Incomplete catalogs and an unresolved Player origin
/// produce no actionable choice (unresolved origin blocks Player only); the adapter re-evaluates
/// after binding the origin or obtaining a complete catalog.
pub fn eligible<'a>(
    snapshot: &'a Snapshot,
    role: Role,
    class: MiiClass,
    origin: PlayerOrigin,
) -> impl Iterator<Item = CatalogEntry<'a>> + 'a {
    snapshot.entries(class).filter(move |cfg| snapshot.is_complete() && match (role, origin) {
        (Role::Player, PlayerOrigin::Unresolved) => false,
        (Role::Player, PlayerOrigin::Mii(origin)) => cfg.canonical != origin.0,
        _ => true,
    })
}

/// A class tile is selectable when at least one eligible configuration exists. For P1's original
/// class this is exactly "another distinct configuration exists"; copies never count.
pub fn class_selectable(snapshot: &Snapshot, role: Role, class: MiiClass, origin: PlayerOrigin) -> bool {
    eligible(snapshot, role, class, origin).next().is_some()
}

/// Resolve the one configuration a role uses for a class this session. Never mutates `prefs`.
pub fn resolve(
    snapshot: &Snapshot,
    prefs: &Preferences,
    role: Role,
    class: MiiClass,
    origin: PlayerOrigin,
) -> Option<Choice> {
    let fallback = match prefs.get(role, class) {
        None => Fallback::NoPreference,
        Some(wanted) => {
            if let Some(hit) = eligible(snapshot, role, class, origin).find(|cfg| cfg.canonical == wanted) {
                return Some(Choice { canonical: hit.canonical, slot_id: hit.slot_id, source: Source::Preference });
            }
            if origin.canonical().is_some_and(|o| o == wanted) && role == Role::Player {
                Fallback::PreferenceIsOrigin
            } else {
                Fallback::PreferenceUnavailable
            }
        }
    };
    eligible(snapshot, role, class, origin)
        .next()
        .map(|cfg| Choice { canonical: cfg.canonical, slot_id: cfg.slot_id, source: Source::FirstEligible(fallback) })
}

/// One cycle entry per selectable class, in tile order. Slot 0 itself is not an entry; the roster
/// keeps it as the original.
pub fn cycle_entries(snapshot: &Snapshot, prefs: &Preferences, origin: PlayerOrigin) -> Vec<(MiiClass, Choice)> {
    CLASSES
        .iter()
        .filter_map(|class| resolve(snapshot, prefs, Role::Player, *class, origin).map(|choice| (*class, choice)))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownRow {
    pub slot_id: Option<u32>,
    pub name: String,
    pub copies: usize,
    pub canonical: Canonical,
    /// Not selectable: P1's slot 0, incomplete catalog, or unresolved Player origin.
    pub greyed: bool,
    /// The configuration this role currently resolves to.
    pub selected: bool,
}

/// Dropdown rows for one class page and role: all deduplicated configurations of the class in
/// snapshot order, with slot 0 greyed for the player and the resolved choice marked.
pub fn dropdown(
    snapshot: &Snapshot,
    prefs: &Preferences,
    role: Role,
    class: MiiClass,
    origin: PlayerOrigin,
) -> Vec<DropdownRow> {
    let chosen = resolve(snapshot, prefs, role, class, origin).map(|c| c.canonical);
    snapshot
        .entries(class)
        .map(|cfg| DropdownRow {
            slot_id: cfg.slot_id,
            name: cfg.name.to_owned(),
            copies: cfg.copies,
            canonical: cfg.canonical,
            greyed: !snapshot.is_complete()
                || (role == Role::Player && (origin == PlayerOrigin::Unresolved
                    || origin.canonical().is_some_and(|o| o == cfg.canonical))),
            selected: chosen == Some(cfg.canonical),
        })
        .collect()
}

/// What is live now, as the roster adapter reports it. `Origin` is P1 at slot 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerLive {
    Origin,
    Cycled { class: MiiClass, canonical: Canonical },
    NotMii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Active {
    pub player: PlayerLive,
    /// Active CPU Mii configuration, if the CPU currently is a Mii.
    pub cpu: Option<(MiiClass, Canonical)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ReloadPlayer(Choice),
    ReloadCpu(Choice),
}

/// Menu close: at most one action per role, only for a changed selector of the live class.
/// Unrelated menu edits preserve native CSS choices and current cycled configurations.
/// P1 at slot 0 never produces an action; unresolved or unchanged configurations stay intact.
pub fn on_menu_close(snapshot: &Snapshot, before: &Preferences, prefs: &Preferences, origin: PlayerOrigin, active: Active) -> Vec<Action> {
    let mut actions = Vec::with_capacity(2);
    if let PlayerLive::Cycled { class, canonical } = active.player {
        if let Some(choice) = resolve(snapshot, prefs, Role::Player, class, origin)
            .filter(|_| before.get(Role::Player, class) != prefs.get(Role::Player, class)) {
            if choice.canonical != canonical {
                actions.push(Action::ReloadPlayer(choice));
            }
        }
    }
    if let Some((class, canonical)) = active.cpu {
        if let Some(choice) = resolve(snapshot, prefs, Role::Cpu, class, origin)
            .filter(|_| before.get(Role::Cpu, class) != prefs.get(Role::Cpu, class)) {
            if choice.canonical != canonical {
                actions.push(Action::ReloadCpu(choice));
            }
        }
    }
    actions
}

// ------------------------------------------------------------------------------------------ tests
// Behavioral regression cases adapted from the accepted selection policy.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{build_snapshot, RawSlot, TaggedId, CHARINFO_LEN};

    const HAT: u64 = 0x1300011bfef7a5f7;
    const BODY: u64 = 0x5a00000fa9efa5ea;
    const BRAWLER: u64 = 0xc1004c13fdfdfb2b;
    const SWORD: u64 = 0xc1004d15d7061f77;
    const GUNNER: u64 = 0xc1004e1257ab1d87;

    fn slot(index: usize, slot_id: u32, chara: u64, face: u8, create_id: u8) -> RawSlot {
        let mut charinfo = [face; CHARINFO_LEN];
        charinfo[..0x10].fill(create_id);
        charinfo[0x10..0x24].fill(b'N');
        charinfo[0x24] = 0;
        charinfo[0x25] = 0;
        RawSlot {
            index,
            slot_id,
            name_len: 1,
            name_units: [u16::from(face), 0, 0, 0, 0, 0, 0, 0, 0, 0],
            colour: 0,
            voice: 37,
            specials: [0, 0, 0, 0],
            tag_outfit: TaggedId(HAT),
            tag_hat: TaggedId(BODY),
            tag_chara: TaggedId(chara),
            charinfo,
        }
    }

    /// Operator example: Sword A/B, Brawler A/B/C, Gunner A. Slot ids chosen so that A < B < C.
    fn example() -> Snapshot {
        build_snapshot(vec![
            slot(0, 2622, SWORD, b'a', 1),   // Sword A (origin)
            slot(1, 2700, SWORD, b'b', 2),   // Sword B
            slot(2, 4155, BRAWLER, b'c', 3), // Brawler A
            slot(3, 4200, BRAWLER, b'd', 4), // Brawler B
            slot(4, 4300, BRAWLER, b'e', 5), // Brawler C
            slot(5, 3137, GUNNER, b'f', 6),  // Gunner A
        ])
    }

    fn canonical_of(snapshot: &Snapshot, slot_id: u32) -> Canonical {
        snapshot.configs.iter().find(|c| c.slot_id == slot_id).unwrap().canonical
    }

    #[test]
    fn replay_import_uses_native_first_dedup_without_native_ids_or_preference_writes() {
        use crate::{draft::{Draft,Library,Saved},lifetime::Configuration};
        let native=example();let original=&native.configs[0];
        let mut library=Library::default();
        let original_config=Configuration::from_saved(original).unwrap();
        assert_eq!(library.import(original_config,"Replay known",&native),Ok(Saved::ExistingNative(original.canonical)));
        let mut draft=Draft::from_template(original).unwrap();draft.set_specials([1,0,0,0]).unwrap();
        let imported=draft.configuration().unwrap();let id=imported.canonical();
        assert_eq!(library.import(imported,"Replay unknown",&native),Ok(Saved::Created(id)));
        assert_eq!(library.import(imported,"Rename",&native),Ok(Saved::ExistingPreset(id)));
        let library:Library=serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
        assert_eq!(library.presets()[0].configuration(),imported);
        let catalog=native.clone().with_library(&library);
        let prefs=Preferences::default().with(Role::Cpu,id.class,Some(id)).unwrap();
        let before=serde_json::to_vec(&prefs).unwrap();
        let selected=resolve(&catalog,&prefs,Role::Cpu,id.class,PlayerOrigin::NotMii).unwrap();
        assert_eq!(selected.slot_id,None);assert_eq!(catalog.configuration(selected),Some(imported));
        assert_eq!(catalog.presets[0].name(),"Replay unknown");
        let rows=dropdown(&catalog,&prefs,Role::Cpu,id.class,PlayerOrigin::NotMii);
        assert!(rows.iter().any(|row|row.slot_id.is_none() && row.canonical==id && row.selected && !row.greyed));
        assert_eq!(cycle_entries(&catalog,&prefs,PlayerOrigin::NotMii).iter().filter(|(class,_)|*class==id.class).count(),1);
        assert_eq!(serde_json::to_vec(&prefs).unwrap(),before);
        assert_ne!(resolve(&catalog,&prefs,Role::Player,id.class,PlayerOrigin::NotMii).unwrap().canonical,id);
        let mut native_copy=original.clone();native_copy.slot_id=9000;
        native_copy.canonical=id;native_copy.charinfo = *imported.charinfo();
        let mut extended=native;extended.configs.push(native_copy);
        let merged=extended.with_library(&library);
        assert!(merged.presets.is_empty());
        let native_choice=resolve(&merged,&prefs,Role::Cpu,id.class,PlayerOrigin::NotMii).unwrap();
        assert_eq!(native_choice.slot_id,Some(9000));
        assert_eq!(merged.configuration(native_choice),Some(imported));
    }

    #[test]
    fn operator_example_cycle_and_dropdown() {
        let snap = example();
        assert!(snap.is_complete());
        let origin = Origin(canonical_of(&snap, 2622));
        let prefs = Preferences::default();
        let entries = cycle_entries(&snap, &prefs, PlayerOrigin::Mii(origin));
        let ids: Vec<(MiiClass, u32)> = entries.iter().map(|(c, ch)| (*c, ch.slot_id.unwrap())).collect();
        assert_eq!(ids, vec![(MiiClass::Brawler, 4155), (MiiClass::Swordfighter, 2700), (MiiClass::Gunner, 3137)]);
        assert!(entries.iter().all(|(_, ch)| ch.source == Source::FirstEligible(Fallback::NoPreference)));
        assert!(class_selectable(&snap, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)));

        let rows = dropdown(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin));
        assert_eq!(rows.iter().map(|r| (r.slot_id.unwrap(), r.greyed, r.selected)).collect::<Vec<_>>(), vec![(2622, true, false), (2700, false, true)]);

        // A persisted Brawler preference picks C; B and C stay dropdown rows, still one Brawler entry.
        let prefs = prefs.with(Role::Player, MiiClass::Brawler, Some(canonical_of(&snap, 4300))).unwrap();
        let entries = cycle_entries(&snap, &prefs, PlayerOrigin::Mii(origin));
        let brawler: Vec<&Choice> = entries.iter().filter(|(c, _)| *c == MiiClass::Brawler).map(|(_, ch)| ch).collect();
        assert_eq!(brawler.len(), 1);
        assert_eq!((brawler[0].slot_id.unwrap(), brawler[0].source), (4300, Source::Preference));
        assert_eq!(dropdown(&snap, &prefs, Role::Player, MiiClass::Brawler, PlayerOrigin::Mii(origin)).len(), 3);
    }

    #[test]
    fn duplicate_only_origin_class_is_not_selectable_for_player_but_cpu_may_use_it() {
        let snap = build_snapshot(vec![
            slot(0, 2622, SWORD, b'a', 1),
            slot(1, 2900, SWORD, b'a', 9), // byte-identical appearance, different name/create id: a copy
            slot(2, 3137, GUNNER, b'f', 6),
        ]);
        assert_eq!(snap.of_class(MiiClass::Swordfighter).count(), 1);
        let origin = Origin(canonical_of(&snap, 2622));
        assert!(!class_selectable(&snap, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)));
        assert!(class_selectable(&snap, Role::Player, MiiClass::Gunner, PlayerOrigin::Mii(origin)));
        assert!(resolve(&snap, &Preferences::default(), Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).is_none());
        let cpu = resolve(&snap, &Preferences::default(), Role::Cpu, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!((cpu.slot_id.unwrap(), cpu.canonical), (2622, origin.0));
        let entries = cycle_entries(&snap, &Preferences::default(), PlayerOrigin::Mii(origin));
        assert_eq!(entries.iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![MiiClass::Gunner]);
    }

    #[test]
    fn preference_equal_to_origin_or_stale_falls_back_without_mutation() {
        let snap = example();
        let origin = Origin(canonical_of(&snap, 2622));
        let prefs = Preferences::default().with(Role::Player, MiiClass::Swordfighter, Some(origin.0)).unwrap();
        let before = prefs.clone();
        let choice = resolve(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!((choice.slot_id.unwrap(), choice.source), (2700, Source::FirstEligible(Fallback::PreferenceIsOrigin)));
        assert_eq!(prefs, before);

        let stale = {
            let foreign = build_snapshot(vec![slot(0, 9999, SWORD, b'z', 42)]);
            canonical_of(&foreign, 9999)
        };
        let prefs = Preferences::default().with(Role::Player, MiiClass::Swordfighter, Some(stale)).unwrap();
        let before = prefs.clone();
        let choice = resolve(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!((choice.slot_id.unwrap(), choice.source), (2700, Source::FirstEligible(Fallback::PreferenceUnavailable)));
        assert_eq!(prefs, before);
    }

    #[test]
    fn player_and_cpu_choices_are_independent() {
        let snap = example();
        let origin = Origin(canonical_of(&snap, 2622));
        let prefs = Preferences::default()
            .with(Role::Cpu, MiiClass::Swordfighter, Some(origin.0)).unwrap()
            .with(Role::Cpu, MiiClass::Brawler, Some(canonical_of(&snap, 4200))).unwrap();
        let cpu_sword = resolve(&snap, &prefs, Role::Cpu, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!((cpu_sword.slot_id.unwrap(), cpu_sword.source), (2622, Source::Preference));
        let player_sword = resolve(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!(player_sword.slot_id.unwrap(), 2700);
        let player_brawler = resolve(&snap, &prefs, Role::Player, MiiClass::Brawler, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!((player_brawler.slot_id.unwrap(), player_brawler.source), (4155, Source::FirstEligible(Fallback::NoPreference)));
        let cpu_brawler = resolve(&snap, &prefs, Role::Cpu, MiiClass::Brawler, PlayerOrigin::Mii(origin)).unwrap();
        assert_eq!(cpu_brawler.slot_id.unwrap(), 4200);
        assert!(dropdown(&snap, &prefs, Role::Cpu, MiiClass::Swordfighter, PlayerOrigin::Mii(origin)).iter().all(|r| !r.greyed));
    }

    #[test]
    fn menu_close_touches_only_active_cycled_player_or_active_cpu() {
        let snap = example();
        let origin = Origin(canonical_of(&snap, 2622));
        let prefs = Preferences::default()
            .with(Role::Player, MiiClass::Swordfighter, Some(canonical_of(&snap, 2700))).unwrap()
            .with(Role::Player, MiiClass::Brawler, Some(canonical_of(&snap, 4300))).unwrap()
            .with(Role::Cpu, MiiClass::Brawler, Some(canonical_of(&snap, 4155))).unwrap();

        // P1 at slot 0: no player action whatever the preferences say.
        let at_origin = Active { player: PlayerLive::Origin, cpu: None };
        assert!(on_menu_close(&snap, &Preferences::default(), &prefs, PlayerOrigin::Mii(origin), at_origin).is_empty());

        // P1 cycled into Brawler A while the preference now says C: reload the player to C.
        let cycled = Active {
            player: PlayerLive::Cycled { class: MiiClass::Brawler, canonical: canonical_of(&snap, 4155) },
            cpu: Some((MiiClass::Gunner, canonical_of(&snap, 3137))),
        };
        let actions = on_menu_close(&snap, &Preferences::default(), &prefs, PlayerOrigin::Mii(origin), cycled);
        assert_eq!(actions.len(), 1);
        match actions[0] {
            Action::ReloadPlayer(choice) => assert_eq!((choice.slot_id.unwrap(), choice.source), (4300, Source::Preference)),
            other => panic!("unexpected {other:?}"),
        }

        // Unchanged resolution: nothing to do.
        let settled = Active {
            player: PlayerLive::Cycled { class: MiiClass::Brawler, canonical: canonical_of(&snap, 4300) },
            cpu: Some((MiiClass::Gunner, canonical_of(&snap, 3137))),
        };
        assert!(on_menu_close(&snap, &Preferences::default(), &prefs, PlayerOrigin::Mii(origin), settled).is_empty());

        // Active CPU Brawler B while the CPU preference says A: reload the CPU only.
        let cpu_change = Active { player: PlayerLive::Origin, cpu: Some((MiiClass::Brawler, canonical_of(&snap, 4200))) };
        let actions = on_menu_close(&snap, &Preferences::default(), &prefs, PlayerOrigin::Mii(origin), cpu_change);
        assert_eq!(actions, vec![Action::ReloadCpu(Choice { canonical: canonical_of(&snap, 4155), slot_id: Some(4155), source: Source::Preference })]);
    }

    #[test]
    fn unrelated_menu_close_preserves_native_cpu_and_cycled_player() {
        let snap = example();
        let before = Preferences::default();
        let active = Active {
            player: PlayerLive::Cycled { class: MiiClass::Brawler, canonical: canonical_of(&snap, 4200) },
            cpu: Some((MiiClass::Swordfighter, canonical_of(&snap, 2700))),
        };
        assert!(on_menu_close(&snap, &before, &before, PlayerOrigin::NotMii, active).is_empty());
        let unrelated = before.clone().with(Role::Cpu, MiiClass::Gunner, Some(canonical_of(&snap, 3137))).unwrap();
        assert!(on_menu_close(&snap, &before, &unrelated, PlayerOrigin::NotMii, active).is_empty());
        let selected = unrelated.clone().with(Role::Cpu, MiiClass::Swordfighter, Some(canonical_of(&snap, 2622))).unwrap();
        let actions = on_menu_close(&snap, &unrelated, &selected, PlayerOrigin::NotMii, active);
        assert_eq!(actions, vec![Action::ReloadCpu(Choice {
            canonical: canonical_of(&snap, 2622), slot_id: Some(2622), source: Source::Preference,
        })]);
    }

    #[test]
    fn incomplete_catalog_blocks_both_roles_and_unknown_original_blocks_only_player() {
        let mut snap = example();
        let prefs = Preferences::default();
        assert!(cycle_entries(&snap, &prefs, PlayerOrigin::Unresolved).is_empty());
        assert!(resolve(&snap, &prefs, Role::Cpu, MiiClass::Swordfighter, PlayerOrigin::Unresolved).is_some());
        assert!(dropdown(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Unresolved)
            .iter().all(|row| row.greyed && !row.selected));
        snap.findings.push(crate::inventory::Finding::Source { slot_id: Some(9999), error: "unreadable".into() });
        for role in [Role::Player, Role::Cpu] {
            for class in CLASSES {
                assert!(!class_selectable(&snap, role, class, PlayerOrigin::NotMii));
                assert!(resolve(&snap, &prefs, role, class, PlayerOrigin::NotMii).is_none());
                assert!(dropdown(&snap, &prefs, role, class, PlayerOrigin::NotMii)
                    .iter().all(|row| row.greyed && !row.selected));
            }
        }
    }

    #[test]
    fn identity_preference_survives_a_new_representative_save_id() {
        let original = build_snapshot(vec![slot(0, 5, SWORD, b'a', 1)]);
        let wanted = canonical_of(&original, 5);
        let prefs = Preferences::default().with(Role::Cpu, MiiClass::Swordfighter, Some(wanted)).unwrap();
        let before = serde_json::to_vec(&prefs).unwrap();
        let moved = build_snapshot(vec![slot(0, 700, SWORD, b'a', 9)]);
        let chosen = resolve(&moved, &prefs, Role::Cpu, MiiClass::Swordfighter, PlayerOrigin::NotMii).unwrap();
        assert_eq!((chosen.slot_id.unwrap(), chosen.canonical, chosen.source), (700, wanted, Source::Preference));
        assert_eq!(serde_json::to_vec(&prefs).unwrap(), before);
    }

    #[test]
    fn session_fallback_keeps_persisted_bytes_and_recovers_in_next_session() {
        let snap = example();
        let wanted = canonical_of(&snap, 2622);
        let prefs = Preferences::default().with(Role::Player, MiiClass::Swordfighter, Some(wanted)).unwrap();
        let before = serde_json::to_vec(&prefs).unwrap();
        assert_eq!(resolve(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::Mii(Origin(wanted))).unwrap().slot_id.unwrap(), 2700);
        let absent = build_snapshot(vec![slot(0, 2700, SWORD, b'b', 2)]);
        assert_eq!(resolve(&absent, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::NotMii).unwrap().slot_id.unwrap(), 2700);
        assert_eq!(serde_json::to_vec(&prefs).unwrap(), before);
        assert_eq!(resolve(&snap, &prefs, Role::Player, MiiClass::Swordfighter, PlayerOrigin::NotMii).unwrap().source, Source::Preference);
    }

    #[test]
    fn all_six_selectors_roundtrip_and_wrong_class_is_rejected() {
        let snap = example();
        let mut prefs = Preferences::default();
        for (role, ids) in [(Role::Player, [4300, 2700, 3137]), (Role::Cpu, [4155, 2622, 3137])] {
            for (class, id) in CLASSES.into_iter().zip(ids) {
                prefs = prefs.with(role, class, Some(canonical_of(&snap, id))).unwrap();
            }
        }
        let bytes = serde_json::to_vec(&prefs).unwrap();
        assert_eq!(serde_json::from_slice::<Preferences>(&bytes).unwrap(), prefs);
        let mut bad = serde_json::to_value(&prefs).unwrap();
        bad["cpu"][0] = bad["cpu"][1].clone();
        assert!(serde_json::from_value::<Preferences>(bad).is_err());
        assert!(prefs.with(Role::Cpu, MiiClass::Gunner, Some(canonical_of(&snap, 2622))).is_err());
        assert_eq!(serde_json::from_str::<Preferences>("{}").unwrap(), Preferences::default());
        let player_only = serde_json::json!({"player":[null, canonical_of(&snap, 2622), null]});
        let parsed: Preferences = serde_json::from_value(player_only).unwrap();
        assert_eq!(parsed.get(Role::Player, MiiClass::Swordfighter), Some(canonical_of(&snap, 2622)));
        assert!(CLASSES.into_iter().all(|class| parsed.get(Role::Cpu, class).is_none()));
    }
}
