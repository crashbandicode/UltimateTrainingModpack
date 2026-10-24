//! Ordered mixed-roster selection only. No native admission, writes or persistence.
//! An original is an explicit position: a saved original and its numbered same-class
//! alternative must never be equated merely because their fighter kinds match.

use crate::{character_forms::Forms, TrioFighter};
use training_mod_mii::{
    inventory::{Canonical, MiiClass, Snapshot},
    selection::{self, Choice, Origin, PlayerOrigin, Preferences, Role},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    /// Actual non-Mii kind, including a live transformed member of a native group.
    Fighter(u32),
    Saved(Canonical),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    /// The caller has qualified its immutable native original, not just its kind.
    Original,
    Numbered {
        identity: Identity,
        /// Zero-based position in the last resolved cycle; only a hint after edits.
        cursor: Option<usize>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Restore the caller's immutable original, including its saved configuration.
    Original,
    Fighter(u32),
    Saved(Choice),
}

impl Target {
    /// The committed cycle position and the bytes published are independent. Slot0
    /// restores the captured configuration even when the last numbered target was ordinary.
    pub fn publish(self, origin:Option<training_mod_mii::lifetime::Configuration>,
        selected:Option<training_mod_mii::lifetime::Configuration>)
        ->Result<Option<training_mod_mii::lifetime::Configuration>,&'static str> {
        match self {
            Self::Original=>Ok(origin),
            Self::Fighter(_)=>Ok(None),
            Self::Saved(choice)=>selected.filter(|c|c.canonical()==choice.canonical)
                .map(Some).ok_or("selected configuration differs from committed target"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub target: Target,
    /// None is Player slot 0; Some(0) is the first numbered entry.
    pub cursor: Option<usize>,
}

/// A manual pick has no numbered cursor and never edits/traverses the cycle.
/// Mii class tiles use the same per-role saved identity resolver as cycling;
/// the protected Player original remains the separate slot-0 cycle target.
pub fn direct(
    kind: u32,
    catalog: &Snapshot,
    preferences: &Preferences,
    role: Role,
    original: Identity,
) -> Option<Step> {
    TrioFighter::from_kind(kind)?;
    let player_origin = match original {
        Identity::Fighter(kind) if MiiClass::from_kind(kind).is_some() => return None,
        Identity::Fighter(_) => PlayerOrigin::NotMii,
        Identity::Saved(canonical) => PlayerOrigin::Mii(Origin(canonical)),
    };
    let target = if let Some(class) = MiiClass::from_kind(kind) {
        Target::Saved(selection::resolve(catalog, preferences, role, class, player_origin)?)
    } else {
        // Preserve the explicitly requested starting form, even in the original group.
        Target::Fighter(kind)
    };
    Some(Step { target, cursor: None })
}

fn matches(target: Target, live: Identity) -> bool {
    match (target, live) {
        (Target::Fighter(kind), Identity::Fighter(current)) => {
            Forms::starting(kind).same_group(current)
        }
        (Target::Saved(choice), Identity::Saved(current)) => choice.canonical == current,
        _ => false,
    }
}

/// Resolve one ordered step using the existing catalog and per-role selectors.
/// `ordered` is the existing roster's `fighters()` order, before original exclusion.
/// The caller must supply a qualified exact original/live identity and commit the
/// returned cursor only after native completion. A result grants no native authority.
///
/// Player slot 0 is outside the numbered order. CPU has no slot-0 return: it wraps
/// through numbered choices, retaining the existing actual-group fallback on entry.
/// Missing/ineligible saved choices are skipped without rewriting preferences.
pub fn next_cycle(
    ordered: &[TrioFighter],
    catalog: &Snapshot,
    preferences: &Preferences,
    role: Role,
    original: Identity,
    live: Live,
) -> Option<Step> {
    next_cycle_with_ice_climbers(ordered, catalog, preferences, role, original, live, false)
}

/// The exception is limited to Ice Climbers with different original/cycle Solo
/// preferences. Saved-Mii identity and every other group keep their existing rule.
fn next_cycle_with_ice_climbers(
    ordered: &[TrioFighter], catalog: &Snapshot, preferences: &Preferences,
    role: Role, original: Identity, live: Live, separate_ice_climbers: bool,
) -> Option<Step> {
    // A kind-only Mii cannot establish the immutable configuration identity.
    if matches!(original, Identity::Fighter(kind) if MiiClass::from_kind(kind).is_some())
        || matches!(live, Live::Numbered { identity: Identity::Fighter(kind), .. }
            if MiiClass::from_kind(kind).is_some())
    {
        return None;
    }
    let player_origin = match original {
        Identity::Fighter(_) => PlayerOrigin::NotMii,
        Identity::Saved(canonical) => PlayerOrigin::Mii(Origin(canonical)),
    };
    let mut choices = Vec::with_capacity(ordered.len());
    for fighter in ordered {
        let kind = fighter.kind();
        if let Some(class) = MiiClass::from_kind(kind) {
            if choices.iter().any(
                |target| matches!(target, Target::Saved(choice) if choice.canonical.class == class),
            ) {
                continue;
            }
            if let Some(choice) =
                selection::resolve(catalog, preferences, role, class, player_origin)
            {
                choices.push(Target::Saved(choice));
            }
        } else {
            let forms = Forms::starting(kind);
            if (role == Role::Player
                && matches!(original, Identity::Fighter(current) if forms.same_group(current))
                && !(separate_ice_climbers && kind == 0x4b))
                || choices.iter().any(
                    |target| matches!(target, Target::Fighter(other) if forms.same_group(*other)),
                )
            {
                continue;
            }
            choices.push(Target::Fighter(kind));
        }
    }
    let position = match live {
        Live::Original if role == Role::Player => None,
        Live::Original => choices.iter().position(|target| matches(*target, original)),
        Live::Numbered { identity, cursor } => cursor
            .filter(|i| {
                choices
                    .get(*i)
                    .is_some_and(|target| matches(*target, identity))
            })
            .or_else(|| choices.iter().position(|target| matches(*target, identity))),
    };
    if let Some(index) = position {
        if let Some(target) = choices.get(index + 1) {
            return Some(Step {
                target: *target,
                cursor: Some(index + 1),
            });
        }
        if role == Role::Player {
            return Some(Step {
                target: Target::Original,
                cursor: None,
            });
        }
    }
    choices
        .first()
        .map(|target| Step {
            target: *target,
            cursor: Some(0),
        })
        .or_else(|| {
            (role == Role::Player).then_some(Step {
                target: Target::Original,
                cursor: None,
            })
        })
}


/// CPU-only preflight filtering of one numbered traversal. Rejected choices move
/// only this local lookup cursor: no live cursor or preference is changed here.
/// The caller supplies read-only target qualification, never INSTALL/publication.
pub fn next_qualified_cpu<T>(
    ordered: &[TrioFighter],
    catalog: &Snapshot,
    preferences: &Preferences,
    original: Identity,
    live: Live,
    qualify: impl FnMut(Step) -> Option<T>,
) -> Option<(Step, T)> {
    next_qualified(ordered, catalog, preferences, Role::Cpu, original, live, false, qualify)
}

/// Player traversal includes the protected original as its final, unnumbered
/// position. An unavailable numbered choice may be skipped before native work;
/// failure to qualify the original stops rather than wrapping past slot 0.
pub fn next_qualified_player<T>(
    ordered: &[TrioFighter],
    catalog: &Snapshot,
    preferences: &Preferences,
    original: Identity,
    live: Live,
    qualify: impl FnMut(Step) -> Option<T>,
) -> Option<(Step, T)> {
    next_qualified(ordered, catalog, preferences, Role::Player, original, live, false, qualify)
}

/// Player traversal with the same opposite-Solo exception as the portrait grid.
#[allow(clippy::too_many_arguments)]
pub fn next_qualified_player_with_ice_climbers<T>(
    ordered: &[TrioFighter], catalog: &Snapshot, preferences: &Preferences,
    original: Identity, live: Live, separate_ice_climbers: bool,
    qualify: impl FnMut(Step) -> Option<T>,
) -> Option<(Step, T)> {
    next_qualified(ordered, catalog, preferences, Role::Player, original, live,
        separate_ice_climbers, qualify)
}

#[allow(clippy::too_many_arguments)]
fn next_qualified<T>(
    ordered: &[TrioFighter],
    catalog: &Snapshot,
    preferences: &Preferences,
    role: Role,
    original: Identity,
    live: Live,
    separate_ice_climbers: bool,
    mut qualify: impl FnMut(Step) -> Option<T>,
) -> Option<(Step, T)> {
    let mut lookup = live;
    let mut first = None;
    // Numbered choices cannot outnumber roster entries. Player also has slot 0.
    let limit = ordered.len().saturating_add(usize::from(role == Role::Player));
    for _ in 0..limit {
        let step = next_cycle_with_ice_climbers(ordered, catalog, preferences, role,
            original, lookup, separate_ice_climbers)?;
        if first == Some(step) {
            break;
        }
        first.get_or_insert(step);
        if let Some(value) = qualify(step) {
            return Some((step, value));
        }
        if step.target == Target::Original {
            return None;
        }
        lookup = match step.target {
            Target::Original => Live::Original,
            Target::Fighter(kind) => Live::Numbered {
                identity: Identity::Fighter(kind), cursor: step.cursor,
            },
            Target::Saved(choice) => Live::Numbered {
                identity: Identity::Saved(choice.canonical), cursor: step.cursor,
            },
        };
    }
    None
}

#[cfg(all(test, feature = "mii-saved-alpha"))]
mod tests {
    use super::*;
    use training_mod_mii::inventory::{build_snapshot, RawSlot, TaggedId, CHARINFO_LEN};
    use training_mod_mii::selection::{Fallback, Source};

    #[test]
    fn opposite_solo_ice_climbers_stays_in_a_mixed_saved_mii_cycle() {
        let catalog = example();
        let preferences = Preferences::default();
        let ordered = roster(&[0x4b, 0x48, 6]);
        let original = Identity::Fighter(0x4b);
        let step = |live, different| next_qualified_player_with_ice_climbers(
            &ordered, &catalog, &preferences, original, live, different, Some,
        ).unwrap().0;
        let climbers = step(Live::Original, true);
        assert_eq!(climbers, Step { target: Target::Fighter(0x4b), cursor: Some(0) });
        let mii = step(applied(climbers), true);
        assert!(matches!(mii.target, Target::Saved(_)));
        let kirby = step(applied(mii), true);
        assert_eq!(kirby.target, Target::Fighter(6));
        assert_eq!(step(applied(kirby), true), Step { target: Target::Original, cursor: None });
        // Matching Solo preferences exclude only the duplicate IC; the saved
        // Mii is still the first numbered choice and slot 0 remains reachable.
        assert!(matches!(step(Live::Original, false).target, Target::Saved(_)));
        // Qualification failures skip only the unavailable Mii, never the IC.
        let no_mii = next_qualified_player_with_ice_climbers(
            &ordered, &catalog, &preferences, original, applied(climbers), true,
            |step| (!matches!(step.target, Target::Saved(_))).then_some(step),
        ).unwrap().0;
        assert_eq!(no_mii.target, Target::Fighter(6));
    }

    fn raw(index: usize, id: u32, class: MiiClass, face: u8) -> RawSlot {
        let mut charinfo = [face; CHARINFO_LEN];
        charinfo[..0x10].fill(index as u8);
        charinfo[0x10..0x24].fill(b'N');
        charinfo[0x24..0x26].fill(0);
        RawSlot {
            index,
            slot_id: id,
            name_len: 1,
            name_units: [u16::from(face), 0, 0, 0, 0, 0, 0, 0, 0, 0],
            colour: 0,
            voice: 37,
            specials: [0, 0, 0, 0],
            tag_outfit: TaggedId(0x1300011bfef7a5f7),
            tag_hat: TaggedId(0x5a00000fa9efa5ea),
            tag_chara: TaggedId(match class {
                MiiClass::Brawler => 0xc1004c13fdfdfb2b,
                MiiClass::Swordfighter => 0xc1004d15d7061f77,
                MiiClass::Gunner => 0xc1004e1257ab1d87,
            }),
            charinfo,
        }
    }

    fn example() -> Snapshot {
        build_snapshot(vec![
            raw(0, 2622, MiiClass::Swordfighter, b'a'),
            raw(1, 2700, MiiClass::Swordfighter, b'b'),
            raw(2, 4155, MiiClass::Brawler, b'c'),
            raw(3, 4200, MiiClass::Brawler, b'd'),
            raw(4, 4300, MiiClass::Brawler, b'e'),
            raw(5, 3137, MiiClass::Gunner, b'f'),
        ])
    }

    #[test]
    fn direct_mixed_roster_requests_keep_role_identity_and_continue_the_cycle() {
        let catalog = example();
        let preferences = Preferences::default()
            .with(Role::Player, MiiClass::Brawler, Some(id(&catalog, 4200))).unwrap()
            .with(Role::Cpu, MiiClass::Brawler, Some(id(&catalog, 4300))).unwrap();
        let before = preferences.clone();
        let original = Identity::Fighter(0);
        let ordered = roster(&[6, 0x48, 8]);
        let player_request = training_mod_mii::ordinary_player::Request::direct(7, 0x48).unwrap();
        let cpu_request = training_mod_mii::ordinary_cpu::Request::direct(7, 0x48).unwrap();
        assert!(player_request.current(7, None));
        assert!(cpu_request.current(7, None));
        assert!(!player_request.current(8, None));
        assert!(!cpu_request.current(8, None));
        assert_eq!(player_request.choice(), None);
        assert_eq!(cpu_request.choice(), None);
        for (role, kind, expected_id) in [
            (Role::Player, player_request.direct_kind().unwrap(), 4200),
            (Role::Cpu, cpu_request.direct_kind().unwrap(), 4300),
        ] {
            let picked = direct(kind, &catalog, &preferences, role, original).unwrap();
            assert_eq!(saved(picked).canonical, id(&catalog, expected_id));
            assert_eq!(picked.cursor, None);
            let next = next_cycle(&ordered, &catalog, &preferences, role, original, applied(picked)).unwrap();
            assert_eq!(next.target, Target::Fighter(8));
            assert_eq!(next.cursor, Some(2));
            let wrap = next_cycle(&ordered, &catalog, &preferences, role, original, applied(next)).unwrap();
            assert_eq!(wrap.target, if role == Role::Player { Target::Original } else { Target::Fighter(6) });
        }
        // Manual ordinary swaps remain available with a Mii in the cycle and
        // retain the requested group's starting form without adding a cycle slot.
        for kind in [6, 0x25, 0x5b] {
            assert_eq!(direct(kind, &catalog, &preferences, Role::Player, original),
                Some(Step { target: Target::Fighter(kind), cursor: None }));
        }
        assert_eq!(preferences, before);
        assert_eq!(ordered, roster(&[6, 0x48, 8]));
    }

    #[test]
    fn direct_mii_pick_preserves_original_and_does_not_fall_through_to_a_cycle() {
        let catalog = example();
        let original = Identity::Saved(id(&catalog, 4155));
        let preferences = Preferences::default()
            .with(Role::Player, MiiClass::Brawler, Some(id(&catalog, 4155))).unwrap();
        let picked = direct(0x48, &catalog, &preferences, Role::Player, original).unwrap();
        assert_ne!(saved(picked).canonical, id(&catalog, 4155));
        assert_eq!(saved(picked).canonical, id(&catalog, 4200));
        let cpu = direct(0x48, &catalog, &preferences, Role::Cpu, original).unwrap();
        assert_eq!(saved(cpu).canonical, id(&catalog, 4155));
        let next = next_cycle(&roster(&[0x48]), &catalog, &preferences, Role::Player, original, applied(picked)).unwrap();
        assert_eq!(next, Step { target: Target::Original, cursor: None });
        assert_eq!(direct(0x48, &Snapshot::default(), &preferences, Role::Player, original), None);
        assert_eq!(direct(u32::MAX, &catalog, &preferences, Role::Cpu, original), None);
        assert_eq!(direct(6, &catalog, &preferences, Role::Player, Identity::Fighter(0x48)), None);
        assert_eq!(training_mod_mii::ordinary_player::Request::direct(0, 6), None);
        assert_eq!(training_mod_mii::ordinary_cpu::Request::direct(0, 6), None);
    }

    fn id(catalog: &Snapshot, slot: u32) -> Canonical {
        catalog
            .configs
            .iter()
            .find(|c| c.slot_id == slot)
            .unwrap()
            .canonical
    }

    fn roster(kinds: &[u32]) -> Vec<TrioFighter> {
        kinds
            .iter()
            .map(|kind| TrioFighter::from_kind(*kind).unwrap())
            .collect()
    }

    fn applied(step: Step) -> Live {
        match step.target {
            Target::Original => Live::Original,
            Target::Fighter(kind) => Live::Numbered {
                identity: Identity::Fighter(kind),
                cursor: step.cursor,
            },
            Target::Saved(choice) => Live::Numbered {
                identity: Identity::Saved(choice.canonical),
                cursor: step.cursor,
            },
        }
    }

    fn saved(step: Step) -> Choice {
        match step.target {
            Target::Saved(choice) => choice,
            other => panic!("expected saved choice, got {:?}", other),
        }
    }

    #[test]
    fn saved_slot_zero_restores_captured_payload_after_an_ordinary_numbered_target() {
        use training_mod_mii::{lifetime::Configuration,ordinary_player::{retained_record,staged_descriptor}};
        let catalog=example();
        let config=|slot|Configuration::from_saved(catalog.configs.iter().find(|c|c.slot_id==slot).unwrap()).unwrap();
        let origin=config(2622);let alternate=config(2700);
        let order=roster(&[0x49,0x0b]);let preferences=Preferences::default();
        let first=next_cycle(&order,&catalog,&preferences,Role::Player,
            Identity::Saved(origin.canonical()),Live::Original).unwrap();
        assert_eq!(first.target.publish(Some(origin),Some(alternate)),Ok(Some(alternate)));
        assert!(first.target.publish(Some(origin),Some(origin)).is_err());
        let second=next_cycle(&order,&catalog,&preferences,Role::Player,
            Identity::Saved(origin.canonical()),applied(first)).unwrap();
        assert_eq!(second.target,Target::Fighter(0x0b));
        assert_eq!(second.target.publish(Some(origin),None),Ok(None));
        let returned=next_cycle(&order,&catalog,&preferences,Role::Player,
            Identity::Saved(origin.canonical()),applied(second)).unwrap();
        assert_eq!(returned,Step{target:Target::Original,cursor:None});
        let payload=returned.target.publish(Some(origin),None).unwrap();
        assert_eq!(payload,Some(origin));
        let after_alternate=retained_record([0x7d;0x68],Some(alternate));
        let after_fighter=retained_record(after_alternate,None);
        let after_return=retained_record(after_fighter,payload);
        assert_eq!(&after_return[..88],origin.charinfo());
        assert_eq!(&after_return[88..],&after_alternate[88..]);
        assert_ne!(&after_fighter[..88],origin.charinfo());
        let descriptor=staged_descriptor([0xa5;0x70],Some(origin));
        assert_eq!(staged_descriptor(descriptor,None),descriptor);
        // The same class alternate also returns through the explicit original position.
        let same_class=next_cycle(&roster(&[0x49]),&catalog,&preferences,Role::Player,
            Identity::Saved(origin.canonical()),applied(first)).unwrap();
        assert_eq!(same_class,returned);
        assert_eq!(same_class.target.publish(Some(origin),None),Ok(Some(origin)));
    }

    #[test]
    fn player_qualification_skips_unavailable_then_returns_through_original() {
        let catalog = example();
        let prefs = Preferences::default();
        let before = serde_json::to_vec(&prefs).unwrap();
        let order = roster(&[9, 0x48, 0x49, 0x4a]);
        let original = Identity::Fighter(9);
        let qualify = |step: Step| match step.target {
            Target::Saved(choice) if choice.canonical.class != MiiClass::Gunner => None,
            _ => Some(()),
        };
        let (gunner, ()) = next_qualified_player(&order, &catalog, &prefs,
            original, Live::Original, qualify).unwrap();
        assert_eq!(saved(gunner).slot_id.unwrap(), 3137);
        assert_eq!(gunner.cursor, Some(2));
        let (returned, ()) = next_qualified_player(&order, &catalog, &prefs,
            original, applied(gunner), qualify).unwrap();
        assert_eq!(returned, Step {target: Target::Original, cursor: None});
        assert_eq!(serde_json::to_vec(&prefs).unwrap(), before);
    }

    #[test]
    fn player_original_is_not_skipped_or_retried_after_refusal() {
        let catalog = example();
        let prefs = Preferences::default();
        let order = roster(&[0x49, 0x4a]);
        let mut visited = Vec::new();
        let result = next_qualified_player(&order, &catalog, &prefs,
            Identity::Fighter(9), Live::Original, |step| {
                visited.push(step);
                None::<()>
            });
        assert_eq!(result, None);
        assert_eq!(visited.len(), 3);
        assert_eq!(visited[2], Step {target: Target::Original, cursor: None});
        let (empty, ()) = next_qualified_player(&[], &catalog, &prefs,
            Identity::Fighter(9), Live::Original, |_| Some(())).unwrap();
        assert_eq!(empty, Step {target: Target::Original, cursor: None});
    }


    #[test]
    fn cpu_qualification_reaches_later_choice_and_wraps_without_committing() {
        let catalog = example();
        let prefs = Preferences::default();
        let prefs_before = serde_json::to_vec(&prefs).unwrap();
        let order = roster(&[0x48, 0x49, 0x4a, 9]);
        let origin = Identity::Fighter(9);
        let live = Live::Original;
        let qualify = |step: Step| match step.target {
            Target::Saved(choice) if choice.canonical.class != MiiClass::Gunner => None,
            _ => Some(()),
        };
        let (first, ()) = next_qualified_cpu(&order, &catalog, &prefs, origin, live, qualify).unwrap();
        assert_eq!(saved(first).slot_id.unwrap(), 3137);
        assert_eq!(first.cursor, Some(2));
        // The policy returns a proposal; the caller's actual live value stays original.
        assert_eq!(live, Live::Original);
        let (ordinary, ()) = next_qualified_cpu(&order, &catalog, &prefs, origin,
            applied(first), qualify).unwrap();
        assert_eq!(ordinary, Step {target: Target::Fighter(9), cursor: Some(3)});
        let (wrapped, ()) = next_qualified_cpu(&order, &catalog, &prefs, origin,
            applied(ordinary), qualify).unwrap();
        assert_eq!(wrapped, first);
        assert_eq!(serde_json::to_vec(&prefs).unwrap(), prefs_before);
    }

    #[test]
    fn cpu_all_unqualified_stops_after_one_distinct_traversal() {
        let catalog = example();
        let prefs = Preferences::default();
        let prefs_before = serde_json::to_vec(&prefs).unwrap();
        // Duplicate Sword must not make a rejected choice get qualified twice.
        let order = roster(&[0x48, 0x49, 0x49, 0x4a]);
        let live = Live::Original;
        let mut visited = Vec::new();
        let result = next_qualified_cpu(&order, &catalog, &prefs,
            Identity::Fighter(9), live, |step| {
                visited.push((step.cursor, saved(step).slot_id.unwrap()));
                None::<()>
            });
        assert_eq!(result, None);
        assert_eq!(visited, [(Some(0), 4155), (Some(1), 2622), (Some(2), 3137)]);
        assert_eq!(live, Live::Original);
        assert_eq!(serde_json::to_vec(&prefs).unwrap(), prefs_before);
    }

    #[test]
    fn original_sword_a_visits_sword_b_gunner_brawler_then_exact_slot_zero() {
        let catalog = example();
        let origin = Identity::Saved(id(&catalog, 2622));
        let order = roster(&[0x49, 0x4a, 0x48]);
        let prefs = Preferences::default();
        let mut live = Live::Original;
        for (index, slot) in [2700, 3137, 4155].iter().copied().enumerate() {
            let step = next_cycle(&order, &catalog, &prefs, Role::Player, origin, live).unwrap();
            assert_eq!(step.cursor, Some(index));
            assert_eq!(saved(step).slot_id.unwrap(), slot);
            live = applied(step);
        }
        let step = next_cycle(&order, &catalog, &prefs, Role::Player, origin, live).unwrap();
        assert_eq!(
            step,
            Step {
                target: Target::Original,
                cursor: None
            }
        );
        assert_eq!(
            saved(
                next_cycle(
                    &order,
                    &catalog,
                    &prefs,
                    Role::Player,
                    origin,
                    applied(step)
                )
                .unwrap()
            )
            .slot_id.unwrap(),
            2700
        );
    }

    #[test]
    fn mixed_order_preserves_ordinary_positions_and_one_saved_choice_per_class() {
        let catalog = example();
        let prefs = Preferences::default()
            .with(Role::Player, MiiClass::Brawler, Some(id(&catalog, 4300)))
            .unwrap();
        let order = roster(&[0x49, 0x0b, 0x48, 0x48, 0x4a]);
        let origin = Identity::Fighter(9);
        let mut live = Live::Original;
        let mut seen = Vec::new();
        for _ in 0..5 {
            let step = next_cycle(&order, &catalog, &prefs, Role::Player, origin, live).unwrap();
            seen.push(match step.target {
                Target::Original => (0, 0),
                Target::Fighter(kind) => (1, kind),
                Target::Saved(choice) => (2, choice.slot_id.unwrap()),
            });
            live = applied(step);
        }
        assert_eq!(seen, [(2, 2622), (1, 0x0b), (2, 4300), (2, 3137), (0, 0)]);
    }

    #[test]
    fn cpu_uses_its_preferences_includes_player_original_and_wraps_numbered() {
        let catalog = example();
        let prefs = Preferences::default()
            .with(
                Role::Player,
                MiiClass::Swordfighter,
                Some(id(&catalog, 2700)),
            )
            .unwrap()
            .with(Role::Cpu, MiiClass::Swordfighter, Some(id(&catalog, 2622)))
            .unwrap();
        let order = roster(&[0x49, 0x4a]);
        let mut live = Live::Original;
        for slot in [2622, 3137, 2622] {
            let step = next_cycle(
                &order,
                &catalog,
                &prefs,
                Role::Cpu,
                Identity::Fighter(9),
                live,
            )
            .unwrap();
            assert!(step.cursor.is_some());
            assert_eq!(saved(step).slot_id.unwrap(), slot);
            live = applied(step);
        }
        // Native CPU already in a numbered saved choice advances, without inserting slot 0.
        assert_eq!(
            saved(
                next_cycle(
                    &order,
                    &catalog,
                    &prefs,
                    Role::Cpu,
                    Identity::Saved(id(&catalog, 2622)),
                    Live::Original
                )
                .unwrap()
            )
            .slot_id.unwrap(),
            3137
        );
    }

    #[test]
    fn fallback_and_duplicate_only_original_keep_preferences_read_only() {
        let catalog = example();
        let origin = Identity::Saved(id(&catalog, 2622));
        let prefs = Preferences::default()
            .with(
                Role::Player,
                MiiClass::Swordfighter,
                Some(id(&catalog, 2622)),
            )
            .unwrap();
        let bytes = serde_json::to_vec(&prefs).unwrap();
        let order = roster(&[0x49, 0x4a]);
        let choice = saved(
            next_cycle(
                &order,
                &catalog,
                &prefs,
                Role::Player,
                origin,
                Live::Original,
            )
            .unwrap(),
        );
        assert_eq!(
            (choice.slot_id.unwrap(), choice.source),
            (2700, Source::FirstEligible(Fallback::PreferenceIsOrigin))
        );
        let only_original = build_snapshot(vec![
            raw(0, 2622, MiiClass::Swordfighter, b'a'),
            raw(1, 2900, MiiClass::Swordfighter, b'a'),
            raw(2, 3137, MiiClass::Gunner, b'f'),
        ]);
        assert_eq!(only_original.of_class(MiiClass::Swordfighter).count(), 1);
        assert_eq!(
            saved(
                next_cycle(
                    &order,
                    &only_original,
                    &prefs,
                    Role::Player,
                    origin,
                    Live::Original
                )
                .unwrap()
            )
            .slot_id.unwrap(),
            3137
        );
        let stale = Preferences::default()
            .with(
                Role::Player,
                MiiClass::Swordfighter,
                Some(id(&catalog, 2700)),
            )
            .unwrap();
        let stale_bytes = serde_json::to_vec(&stale).unwrap();
        let choice = saved(
            next_cycle(
                &order,
                &only_original,
                &stale,
                Role::Player,
                Identity::Fighter(9),
                Live::Original,
            )
            .unwrap(),
        );
        assert_eq!(
            (choice.slot_id.unwrap(), choice.source),
            (2622, Source::FirstEligible(Fallback::PreferenceUnavailable))
        );
        assert_eq!(serde_json::to_vec(&prefs).unwrap(), bytes);
        assert_eq!(serde_json::to_vec(&stale).unwrap(), stale_bytes);
    }

    #[test]
    fn transformed_groups_and_stale_cursor_follow_existing_ordinary_slots() {
        let catalog = example();
        let prefs = Preferences::default();
        let order = roster(&[0x0b, 0x5b, 0x25, 0x4b]);
        for (current, cursor, next, next_cursor) in [
            (0x5a, Some(1), 0x25, Some(2)),
            (0x26, Some(2), 0x4b, Some(3)),
            (0x5a, Some(99), 0x25, Some(2)),
        ] {
            let step = next_cycle(
                &order,
                &catalog,
                &prefs,
                Role::Player,
                Identity::Fighter(9),
                Live::Numbered {
                    identity: Identity::Fighter(current),
                    cursor,
                },
            )
            .unwrap();
            assert_eq!(
                step,
                Step {
                    target: Target::Fighter(next),
                    cursor: next_cursor
                }
            );
            let kinds: Vec<_> = order.iter().map(|f| f.kind()).collect();
            assert_eq!(
                crate::character_forms::next_cycle(current, cursor, &kinds, Some(9)),
                Some((next, next_cursor))
            );
        }
        let step = next_cycle(
            &order,
            &catalog,
            &prefs,
            Role::Player,
            Identity::Fighter(9),
            Live::Numbered {
                identity: Identity::Fighter(0x4b),
                cursor: Some(3),
            },
        )
        .unwrap();
        assert_eq!(step.target, Target::Original);
        // The original grouped fighter remains excluded as a group, unlike saved-Mii alternates.
        let step = next_cycle(
            &roster(&[0x5b, 0x0b]),
            &catalog,
            &prefs,
            Role::Player,
            Identity::Fighter(0x5a),
            Live::Original,
        )
        .unwrap();
        assert_eq!(
            step,
            Step {
                target: Target::Fighter(0x0b),
                cursor: Some(0)
            }
        );
    }

    #[test]
    fn mii_cursor_is_checked_by_identity_not_class_or_old_save_id() {
        let catalog = example();
        let order = roster(&[0x49, 0x4a]);
        let prefs = Preferences::default()
            .with(
                Role::Player,
                MiiClass::Swordfighter,
                Some(id(&catalog, 2700)),
            )
            .unwrap();
        let origin = Identity::Saved(id(&catalog, 2622));
        // A stale numeric cursor cannot equate Sword A with the selected Sword B.
        let step = next_cycle(
            &order,
            &catalog,
            &prefs,
            Role::Player,
            origin,
            Live::Numbered {
                identity: origin,
                cursor: Some(0),
            },
        )
        .unwrap();
        assert_eq!(saved(step).slot_id.unwrap(), 2700);
        // Reordered slots locate the live canonical configuration, not the old index.
        let reordered = roster(&[0x4a, 0x49]);
        let step = next_cycle(
            &reordered,
            &catalog,
            &prefs,
            Role::Player,
            origin,
            Live::Numbered {
                identity: Identity::Saved(id(&catalog, 2700)),
                cursor: Some(0),
            },
        )
        .unwrap();
        assert_eq!(step.target, Target::Original);
        // A new representative save ID does not change canonical identity.
        let replaced = build_snapshot(vec![
            raw(0, 9000, MiiClass::Swordfighter, b'b'),
            raw(1, 3137, MiiClass::Gunner, b'f'),
        ]);
        let step = next_cycle(
            &order,
            &replaced,
            &prefs,
            Role::Player,
            Identity::Fighter(9),
            Live::Numbered {
                identity: Identity::Saved(id(&catalog, 2700)),
                cursor: Some(0),
            },
        )
        .unwrap();
        assert_eq!(saved(step).slot_id.unwrap(), 3137);
    }

    #[test]
    fn unavailable_mii_choices_and_kind_only_mii_do_not_invent_configuration() {
        let mut catalog = example();
        let prefs = Preferences::default();
        catalog
            .findings
            .push(training_mod_mii::inventory::Finding::Source {
                slot_id: Some(9999),
                error: "unreadable".into(),
            });
        let order = roster(&[0x49, 0x4a]);
        let step = next_cycle(
            &order,
            &catalog,
            &prefs,
            Role::Player,
            Identity::Fighter(9),
            Live::Original,
        )
        .unwrap();
        assert_eq!(
            step,
            Step {
                target: Target::Original,
                cursor: None
            }
        );
        assert!(next_cycle(
            &order,
            &catalog,
            &prefs,
            Role::Cpu,
            Identity::Fighter(9),
            Live::Original
        )
        .is_none());
        assert!(next_cycle(
            &order,
            &catalog,
            &prefs,
            Role::Player,
            Identity::Fighter(0x49),
            Live::Original
        )
        .is_none());
        let step = next_cycle(
            &roster(&[0x49, 0x0b]),
            &catalog,
            &prefs,
            Role::Cpu,
            Identity::Fighter(9),
            Live::Original,
        )
        .unwrap();
        assert_eq!(
            step,
            Step {
                target: Target::Fighter(0x0b),
                cursor: Some(0)
            }
        );
    }
}
