use serde::{Deserialize, Deserializer, Serialize, Serializer};
use training_mod_tui::{
    StatefulTable, SubMenu, SubMenuType, Toggle, NX_SUBMENU_COLUMNS, NX_SUBMENU_ROWS,
};

use crate::{trio::CHOICES, TrioFighter};

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
                if fighters[input].kind() == CHOICES[catalog].0 && roster.ranks[catalog] == 0 {
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
        let catalog = TrioFighter::available();
        if self.all {
            return catalog;
        }
        let mut selected: Vec<_> = catalog
            .into_iter()
            .zip(self.ranks)
            .filter(|(_, rank)| *rank != 0)
            .collect();
        selected.sort_unstable_by_key(|(_, rank)| *rank);
        selected.into_iter().map(|(fighter, _)| fighter).collect()
    }

    pub fn to_submenu<'a>(title: &'a str, id: &'a str, help_text: &'a str) -> SubMenu<'a> {
        let toggles = std::iter::once("All (menu order)")
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
        if values.len() != CHOICES.len() + 1 || values[0] > 1 {
            return Err(serde::de::Error::custom("invalid ordered character roster"));
        }
        let mut ranks = [0; CHOICES.len()];
        ranks.copy_from_slice(&values[1..]);
        let all = values[0] == 1;
        let mut sorted: Vec<_> = ranks.iter().copied().filter(|rank| *rank != 0).collect();
        sorted.sort_unstable();
        let valid = if all {
            sorted.is_empty()
        } else {
            !sorted.is_empty() && sorted.iter().copied().eq(1..=sorted.len() as u8)
        };
        if !valid {
            return Err(serde::de::Error::custom(
                "choose All or a nonempty contiguous character order",
            ));
        }
        Ok(Self { all, ranks })
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

#[cfg(test)]
mod tests {
    use super::*;
    use training_mod_tui::InputControl;

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
        assert_eq!(all.fighters(), TrioFighter::available());
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
        assert_eq!(read_picker(&picker).fighters(), reversed);
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
