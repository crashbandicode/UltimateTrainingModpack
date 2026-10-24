use serde::{Deserialize, Deserializer, Serialize, Serializer};
use training_mod_tui::{
    StatefulTable, SubMenu, SubMenuType, Toggle, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};

use crate::{
    trio::{
        CHOICES, LEGACY_CHOICE_COUNT, MII_ALPHA_CHOICE_COUNT, MII_CLASSES_CHOICE_COUNT, PRE_IC_CHOICE_COUNT,
        PRE_MII_CHOICE_COUNT,
    },
    TrioFighter,
};

/// Catalog positions stay fixed; nonzero ranks represent the user's cycle order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderedFighterRoster {
    all: bool,
    ranks: [u8; CHOICES.len()],
}

impl OrderedFighterRoster {
    pub const fn all() -> Self {
        Self {
            all: true,
            ranks: [0; CHOICES.len()],
        }
    }

    /// Retains the first occurrence of each supported fighter. Empty input is invalid.
    pub const fn from_fighters(fighters: &[TrioFighter]) -> Option<Self> {
        let mut roster = Self {
            all: false,
            ranks: [0; CHOICES.len()],
        };
        let mut rank = 0;
        let mut input = 0;
        while input < fighters.len() {
            let mut catalog = 0;
            while catalog < CHOICES.len() {
                let mut seen_group = false;
                let mut earlier = 0;
                while earlier < CHOICES.len() {
                    if roster.ranks[earlier] != 0
                        && training_mod_tui::roster_grid::group_kind(CHOICES[earlier].0)
                            == training_mod_tui::roster_grid::group_kind(fighters[input].kind())
                    {
                        seen_group = true;
                    }
                    earlier += 1;
                }
                if fighters[input].kind() == CHOICES[catalog].0 && !seen_group {
                    rank += 1;
                    roster.ranks[catalog] = rank;
                }
                catalog += 1;
            }
            input += 1;
        }
        if rank == 0 {
            None
        } else {
            Some(roster)
        }
    }

    pub fn fighters(self) -> Vec<TrioFighter> {
        self.fighters_excluding(None)
    }

    /// `[all, ranks...]`, the same array the portrait editor and JSON store.
    pub fn values(self) -> Vec<u8> {
        std::iter::once(u8::from(self.all))
            .chain(self.ranks)
            .collect()
    }

    /// Known roster catalog with Kirby removed. Does not apply original-P1 exclusion.
    pub fn hat_kinds(self) -> Vec<u32> {
        self.fighters()
            .into_iter()
            .filter(|fighter| {
                fighter.kind() != TrioFighter::KIRBY.kind()
                    && (cfg!(feature="mii-managed-helper-alpha") || !(0x48..=0x4a).contains(&fighter.kind()))
            })
            .map(|fighter| fighter.kind())
            .collect()
    }

    /// Runtime callers use the same native-group exclusion as the portrait editor.
    pub fn fighters_excluding(self, original: Option<u32>) -> Vec<TrioFighter> {
        let catalog = TrioFighter::available();
        if self.all {
            let kinds: Vec<_> = catalog.iter().map(|f| f.kind()).collect();
            let values: Vec<_> = std::iter::once(1).chain(self.ranks).collect();
            return training_mod_tui::roster_grid::css_order(&kinds, &values, original)
                .into_iter()
                .filter_map(TrioFighter::from_kind)
                .collect();
        }
        let mut selected: Vec<_> = catalog
            .into_iter()
            .zip(self.ranks)
            .filter(|(_, rank)| *rank != 0)
            .collect();
        selected.sort_unstable_by_key(|(_, rank)| *rank);
        selected
            .into_iter()
            .map(|(fighter, _)| fighter)
            .filter(|fighter| {
                original.map(training_mod_tui::roster_grid::group_kind)
                    != Some(training_mod_tui::roster_grid::group_kind(fighter.kind()))
            })
            .collect()
    }

    pub fn to_submenu<'a>(title: &'a str, id: &'a str, help_text: &'a str) -> SubMenu<'a> {
        let toggles = std::iter::once("All (CSS order)")
            .chain(CHOICES.iter().map(|(_, name)| *name))
            .enumerate()
            .map(|(index, title)| Toggle {
                title,
                value: u8::from(index == 0),
                max: 1,
            })
            .collect();
        SubMenu {
            title,
            id,
            help_text,
            submenu_type: SubMenuType::Ordered,
            toggles: StatefulTable::with_items(
                (CHOICES.len() + 1)
                    .div_ceil(NX_SUBMENU_COLUMNS)
                    .max(NX_SUBMENU_ROWS),
                NX_SUBMENU_COLUMNS,
                toggles,
            ),
            slider: None,
        }
    }
}

impl Serialize for OrderedFighterRoster {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        std::iter::once(u8::from(self.all))
            .chain(self.ranks.iter().copied())
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OrderedFighterRoster {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<u8>::deserialize(deserializer)?;
        if ![
            LEGACY_CHOICE_COUNT + 1,
            PRE_IC_CHOICE_COUNT + 1,
            PRE_MII_CHOICE_COUNT + 1,
            MII_ALPHA_CHOICE_COUNT + 1,
            MII_CLASSES_CHOICE_COUNT + 1,
        ]
        .contains(&values.len())
            || values[0] > 1
        {
            return Err(serde::de::Error::custom("invalid ordered character roster"));
        }
        let mut ranks = [0; CHOICES.len()];
        let supported = (values.len() - 1).min(CHOICES.len());
        ranks[..supported].copy_from_slice(&values[1..=supported]);
        let all = values[0] == 1;
        // Validate the complete persisted order before dropping the known
        // unsupported alpha choices. A bad tail must not hide a gap or duplicate.
        let mut sorted: Vec<_> = values[1..]
            .iter()
            .copied()
            .filter(|rank| *rank != 0)
            .collect();
        sorted.sort_unstable();
        let valid = if all {
            // New-format All may store one non-default start per group. Legacy
            // mixed All/ranks remains invalid, rather than silently discarding it.
            sorted.is_empty()
                || ([PRE_MII_CHOICE_COUNT + 1, MII_ALPHA_CHOICE_COUNT + 1, MII_CLASSES_CHOICE_COUNT + 1].contains(&values.len())
                    && values[1..].iter().enumerate().all(|(i, rank)| {
                        *rank == 0
                            || (*rank == 1
                                && CHOICES
                                    .get(i)
                                    .is_some_and(|(kind, _)| [0x5b, 0x25, 0x26].contains(kind)))
                    })
                    && ranks
                        .iter()
                        .zip(CHOICES)
                        .filter(|(rank, (kind, _))| **rank != 0 && [0x25, 0x26].contains(kind))
                        .count()
                        <= 1)
        } else {
            !sorted.is_empty() && sorted.iter().copied().eq(1..=sorted.len() as u8)
        };
        if !valid {
            return Err(serde::de::Error::custom(
                "choose All or a nonempty contiguous character order",
            ));
        }
        if all {
            return Ok(Self { all, ranks });
        }
        // Validate the legacy rank sequence before migrating duplicate groups.
        let mut entries: Vec<_> = CHOICES
            .iter()
            .zip(ranks)
            .filter(|(_, rank)| *rank != 0)
            .collect();
        entries.sort_unstable_by_key(|(_, rank)| *rank);
        let fighters: Vec<_> = entries
            .into_iter()
            .filter_map(|((kind, _), _)| TrioFighter::from_kind(*kind))
            .collect();
        // A valid Mii-only alpha roster has no supported choices on base.
        // Preserve a deterministic single choice rather than enabling All.
        if fighters.is_empty() && [MII_ALPHA_CHOICE_COUNT + 1, MII_CLASSES_CHOICE_COUNT + 1].contains(&values.len()) {
            return Ok(Self::from_fighters(&[TrioFighter::MARIO]).unwrap());
        }
        Self::from_fighters(&fighters)
            .ok_or_else(|| serde::de::Error::custom("empty character order"))
    }
}

pub const DEFAULT_PLAYER_ROSTER: OrderedFighterRoster =
    match OrderedFighterRoster::from_fighters(&[
        TrioFighter::GAMEWATCH,
        TrioFighter::PALUTENA,
        TrioFighter::PIKACHU,
    ]) {
        Some(roster) => roster,
        None => unreachable!(),
    };

pub const DEFAULT_CPU_ROSTER: OrderedFighterRoster = match OrderedFighterRoster::from_fighters(&[
    TrioFighter::PIKACHU,
    TrioFighter::KIRBY,
    TrioFighter::BOWSER,
    TrioFighter::BAYONETTA,
]) {
    Some(roster) => roster,
    None => unreachable!(),
};

/// Kirby hats start on Mario, separate from either fighter-cycle default.
pub const DEFAULT_KIRBY_HATS: OrderedFighterRoster =
    match OrderedFighterRoster::from_fighters(&[TrioFighter::MARIO]) {
        Some(roster) => roster,
        None => unreachable!(),
    };

#[cfg(test)]
mod tests {
    use super::*;
    use training_mod_tui::InputControl;

    #[test]
    fn legacy_rosters_keep_their_order_and_all_semantics() {
        let mut legacy = vec![0u8; 81];
        legacy[80] = 1; // Sora first, then Mario.
        legacy[1] = 2;
        let roster: OrderedFighterRoster =
            serde_json::from_value(serde_json::json!(legacy)).unwrap();
        assert_eq!(
            roster
                .fighters()
                .iter()
                .map(|f| f.kind())
                .collect::<Vec<_>>(),
            vec![0x5d, 0]
        );
        let current = serde_json::to_value(roster).unwrap();
        assert_eq!(current.as_array().unwrap().len(), CHOICES.len() + 1);
        assert!(current.as_array().unwrap()[81..].iter().all(|v| v == 0));
        legacy.fill(0);
        legacy[0] = 1;
        let all: OrderedFighterRoster = serde_json::from_value(serde_json::json!(legacy)).unwrap();
        assert_eq!(
            all.fighters().len(),
            83 + 3 * usize::from(cfg!(feature = "mii-saved-alpha"))
        );
        legacy[1] = 1; // All and an explicit rank remain mutually exclusive.
        assert!(serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(legacy)).is_err());
        legacy[0] = 0;
        legacy[80] = 1; // Duplicate ranks remain invalid.
        assert!(serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(legacy)).is_err());
        legacy[80] = 3; // As do gaps.
        assert!(serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(legacy)).is_err());
        for length in [0, 80, 82, 85, MII_ALPHA_CHOICE_COUNT + 2] {
            assert!(
                serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(vec![
                    0u8;
                    length
                ]))
                .is_err()
            );
        }
    }

    #[cfg(not(feature = "mii-saved-alpha"))]
    #[test]
    fn alpha_tail_is_validated_before_base_downgrade() {
        let mut values = vec![0u8; MII_ALPHA_CHOICE_COUNT + 1];
        values[80] = 1; // Sora, Sword, Mario.
        values[87] = 2;
        values[1] = 3;
        let read = |v: &[u8]| serde_json::from_value::<OrderedFighterRoster>(serde_json::json!(v));
        assert_eq!(
            read(&values).unwrap().fighters(),
            vec![TrioFighter::from_kind(0x5d).unwrap(), TrioFighter::MARIO]
        );
        values[87] = 3; // Removing the unsupported tail must not hide a duplicate.
        assert!(read(&values).is_err());
        values[87] = 4; // Nor a gap.
        assert!(read(&values).is_err());
        values.fill(0);
        values[87] = 1;
        assert_eq!(read(&values).unwrap().fighters(), vec![TrioFighter::MARIO]);
        values[0] = 1; // All cannot silently discard a nonzero Sword preference.
        assert!(read(&values).is_err());
        values[87] = 0;
        values[82] = 1; // All with Mythra/Ivysaur start preferences is unchanged.
        values[84] = 1;
        assert_eq!(read(&values).unwrap().values(), values[..87]);
        values.push(0); // Unknown adjacent length is not a future-format escape.
        assert!(read(&values).is_err());
    }

    #[test]
    fn starting_forms_preserve_explicit_order() {
        let fighters: Vec<_> = [0x26, 0x5b, 0x24, 0x5a, 0x25]
            .iter()
            .copied()
            .map(|kind| TrioFighter::from_kind(kind).unwrap())
            .collect();
        let roster = OrderedFighterRoster::from_fighters(&fighters).unwrap();
        let restored: OrderedFighterRoster =
            serde_json::from_str(&serde_json::to_string(&roster).unwrap()).unwrap();
        assert_eq!(restored.fighters(), fighters[..2]);
    }

    fn choose(picker: &mut SubMenu<'_>, fighter: TrioFighter) {
        let index = picker
            .toggles
            .iter()
            .position(|toggle| toggle.title == fighter.name())
            .unwrap();
        picker
            .toggles
            .select(index / picker.toggles.cols, index % picker.toggles.cols);
        picker.on_a();
    }

    fn read_picker(picker: &SubMenu<'_>) -> OrderedFighterRoster {
        serde_json::from_str(&serde_json::to_string(picker).unwrap()).unwrap()
    }

    #[test]
    fn all_uses_the_complete_unchanged_catalog_order() {
        let all = OrderedFighterRoster::all();
        assert_eq!(
            all.fighters().len(),
            83 + 3 * usize::from(cfg!(feature = "mii-saved-alpha"))
        );
        assert_eq!(all.fighters()[16], TrioFighter::ICE_CLIMBERS);
        assert_eq!(all.fighters()[37].kind(), 0x24);
        assert_eq!(all.fighters().first(), Some(&TrioFighter::MARIO));
        assert!(all
            .fighters()
            .iter()
            .all(|fighter| TrioFighter::supported_kind(fighter.kind())));
        assert_eq!(
            serde_json::from_str::<OrderedFighterRoster>(&serde_json::to_string(&all).unwrap())
                .unwrap(),
            all
        );
    }

    #[test]
    fn choose_deselect_reselect_preserves_selection_order() {
        let mut picker = OrderedFighterRoster::to_submenu("Cycle", "roster", "Choose");
        for fighter in [
            TrioFighter::BAYONETTA,
            TrioFighter::KIRBY,
            TrioFighter::BOWSER,
        ] {
            choose(&mut picker, fighter);
        }
        choose(&mut picker, TrioFighter::KIRBY);
        choose(&mut picker, TrioFighter::KIRBY);
        assert_eq!(
            read_picker(&picker).fighters(),
            vec![
                TrioFighter::BAYONETTA,
                TrioFighter::BOWSER,
                TrioFighter::KIRBY
            ]
        );
        choose(&mut picker, TrioFighter::BOWSER);
        choose(&mut picker, TrioFighter::KIRBY);
        choose(&mut picker, TrioFighter::BAYONETTA);
        // Removing the last custom character is refused.
        assert_eq!(
            read_picker(&picker).fighters(),
            vec![TrioFighter::BAYONETTA]
        );
        picker.toggles.select(0, 0);
        picker.on_a();
        assert_eq!(read_picker(&picker), OrderedFighterRoster::all());
    }

    #[test]
    fn entire_catalog_can_be_selected_in_arbitrary_order() {
        let reversed: Vec<_> = TrioFighter::available().into_iter().rev().collect();
        let mut picker = OrderedFighterRoster::to_submenu("Cycle", "roster", "Choose");
        for fighter in &reversed {
            choose(&mut picker, *fighter);
        }
        let mut expected = Vec::new();
        for fighter in reversed {
            if !expected.iter().any(|f: &TrioFighter| {
                training_mod_tui::roster_grid::group_kind(f.kind())
                    == training_mod_tui::roster_grid::group_kind(fighter.kind())
            }) {
                expected.push(fighter);
            }
        }
        assert_eq!(read_picker(&picker).fighters(), expected);
    }

    #[test]
    fn invalid_empty_duplicate_gapped_or_mixed_ranks_are_rejected() {
        let mut values = vec![0u8; CHOICES.len() + 1];
        for sample in [
            values.clone(),
            {
                values[1] = 1;
                values[2] = 1;
                values.clone()
            },
            {
                values[2] = 3;
                values.clone()
            },
            {
                values[0] = 1;
                values.clone()
            },
        ] {
            assert!(serde_json::from_str::<OrderedFighterRoster>(
                &serde_json::to_string(&sample).unwrap()
            )
            .is_err());
        }
    }
}
