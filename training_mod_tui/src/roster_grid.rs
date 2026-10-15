//! Pure model of the shared Player/CPU portrait roster editor.
//!
//! The physical grid mirrors the observed all-unlocked CSS and is independent of
//! the stable catalog order, which only the caller's kind slice defines. Rank
//! arrays are `[all, ranks...]` in catalog order, the layout the ordered roster
//! serializes. This module edits them for the caller but never owns them.

pub const COLUMNS: usize = 13;
pub const ROWS: usize = 7;
const FULL_ROWS: usize = ROWS - 1;
const LAST_ROW_CELLS: usize = 9;
/// The final row is centered: eight fighters and the role control.
const LAST_ROW_START: usize = (COLUMNS - LAST_ROW_CELLS) / 2;
pub const CELLS: usize = FULL_ROWS * COLUMNS + LAST_ROW_CELLS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Player,
    Cpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellContent {
    /// Catalog kind currently displayed by the cell.
    Fighter(u32),
    /// Visible placeholder that keeps its position and can never be selected.
    Unsupported,
    RoleToggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellInfo {
    pub row: usize,
    /// Physical column: the centered last row starts at column 2.
    pub column: usize,
    pub label: &'static str,
    pub content: CellContent,
}

impl CellInfo {
    pub fn kind(self) -> Option<u32> {
        match self.content {
            CellContent::Fighter(kind) => Some(kind),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    RoleToggled,
    RosterChanged,
}

/// Slots whose native entry holds several forms; X previews them independently.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Aegis,
    PokemonTrainer,
}

impl Group {
    const fn forms(self) -> &'static [(u32, &'static str)] {
        match self {
            Self::Aegis => &[(0x5a, "Pyra"), (0x5b, "Mythra")],
            Self::PokemonTrainer => &[(0x24, "Squirtle"), (0x25, "Ivysaur"), (0x26, "Charizard")],
        }
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Fighter(u32, &'static str),
    Shared(Group),
    Unsupported(&'static str),
    RoleToggle,
}

const fn f(kind: u32, name: &'static str) -> Slot {
    Slot::Fighter(kind, name)
}

/// Row-major physical cells, six rows of 13 then the 9-cell last row.
const GRID: [Slot; CELLS] = [
    f(0x00, "Mario"),
    f(0x01, "Donkey Kong"),
    f(0x02, "Link"),
    f(0x03, "Samus"),
    f(0x04, "Dark Samus"),
    f(0x05, "Yoshi"),
    f(0x06, "Kirby"),
    f(0x07, "Fox"),
    f(0x08, "Pikachu"),
    f(0x09, "Luigi"),
    f(0x0a, "Ness"),
    f(0x0b, "Captain Falcon"),
    f(0x0c, "Jigglypuff"),
    f(0x0d, "Peach"),
    f(0x0e, "Daisy"),
    f(0x0f, "Bowser"),
    Slot::Unsupported("Ice Climbers"),
    f(0x10, "Sheik"),
    f(0x11, "Zelda"),
    f(0x12, "Dr. Mario"),
    f(0x13, "Pichu"),
    f(0x14, "Falco"),
    f(0x15, "Marth"),
    f(0x16, "Lucina"),
    f(0x17, "Young Link"),
    f(0x18, "Ganondorf"),
    f(0x19, "Mewtwo"),
    f(0x1a, "Roy"),
    f(0x1b, "Chrom"),
    f(0x1c, "Mr. Game & Watch"),
    f(0x1d, "Meta Knight"),
    f(0x1e, "Pit"),
    f(0x1f, "Dark Pit"),
    f(0x20, "Zero Suit Samus"),
    f(0x21, "Wario"),
    f(0x22, "Snake"),
    f(0x23, "Ike"),
    Slot::Shared(Group::PokemonTrainer),
    f(0x27, "Diddy Kong"),
    f(0x28, "Lucas"),
    f(0x29, "Sonic"),
    f(0x2a, "King Dedede"),
    f(0x2b, "Olimar"),
    f(0x2c, "Lucario"),
    f(0x2d, "R.O.B."),
    f(0x2e, "Toon Link"),
    f(0x2f, "Wolf"),
    f(0x30, "Villager"),
    f(0x31, "Mega Man"),
    f(0x32, "Wii Fit Trainer"),
    f(0x33, "Rosalina & Luma"),
    f(0x34, "Little Mac"),
    f(0x35, "Greninja"),
    f(0x36, "Palutena"),
    f(0x37, "PAC-MAN"),
    f(0x38, "Robin"),
    f(0x39, "Shulk"),
    f(0x3a, "Bowser Jr."),
    f(0x3b, "Duck Hunt"),
    f(0x3c, "Ryu"),
    f(0x3d, "Ken"),
    f(0x3e, "Cloud"),
    f(0x3f, "Corrin"),
    f(0x40, "Bayonetta"),
    f(0x41, "Inkling"),
    f(0x42, "Ridley"),
    f(0x43, "Simon"),
    f(0x44, "Richter"),
    f(0x45, "King K. Rool"),
    f(0x46, "Isabelle"),
    f(0x47, "Incineroar"),
    f(0x51, "Piranha Plant"),
    f(0x52, "Joker"),
    f(0x53, "Hero"),
    f(0x54, "Banjo & Kazooie"),
    f(0x55, "Terry"),
    f(0x56, "Byleth"),
    f(0x57, "Min Min"),
    f(0x58, "Steve"),
    f(0x59, "Sephiroth"),
    Slot::Shared(Group::Aegis),
    f(0x5c, "Kazuya"),
    f(0x5d, "Sora"),
    Slot::Unsupported("Mii Brawler"),
    Slot::Unsupported("Mii Swordfighter"),
    Slot::Unsupported("Mii Gunner"),
    Slot::RoleToggle,
];

const fn row_len(row: usize) -> usize {
    if row < FULL_ROWS {
        COLUMNS
    } else {
        LAST_ROW_CELLS
    }
}

const fn row_start(row: usize) -> usize {
    if row < FULL_ROWS {
        0
    } else {
        LAST_ROW_START
    }
}

/// Cell index to (row, physical column). Rows hold `COLUMNS` cell indices each,
/// so the last row's offset is simply shifted by its start column.
const fn position(cell: usize) -> (usize, usize) {
    let row = cell / COLUMNS;
    (row, row_start(row) + cell % COLUMNS)
}

const fn cell_at(row: usize, column: usize) -> usize {
    row * COLUMNS + column - row_start(row)
}

/// Editor state: cursor cell, active role, and the previewed form of each group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RosterGrid {
    cursor: usize,
    role: Role,
    forms: [u8; 2],
}

impl RosterGrid {
    pub const fn new(role: Role) -> Self {
        Self {
            cursor: 0,
            role,
            forms: [0; 2],
        }
    }

    pub const fn role(&self) -> Role {
        self.role
    }

    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn toggle_role(&mut self) {
        self.role = match self.role {
            Role::Player => Role::Cpu,
            Role::Cpu => Role::Player,
        };
    }

    /// Left/Right wrap within the physical row. Up/Down wrap through the rows and
    /// land on the nearest physical column, so the centered last row clamps.
    pub fn step(&mut self, direction: Direction) {
        let (row, column) = position(self.cursor);
        let (row, column) = match direction {
            Direction::Left | Direction::Right => {
                let len = row_len(row);
                let offset = column - row_start(row);
                let next = match direction {
                    Direction::Right => (offset + 1) % len,
                    _ => (offset + len - 1) % len,
                };
                (row, row_start(row) + next)
            }
            Direction::Up | Direction::Down => {
                let next = match direction {
                    Direction::Down => (row + 1) % ROWS,
                    _ => (row + ROWS - 1) % ROWS,
                };
                let first = row_start(next);
                (next, column.clamp(first, first + row_len(next) - 1))
            }
        };
        self.cursor = cell_at(row, column);
    }

    /// X: advance the previewed form of a shared slot. Returns whether the cursor
    /// was on one. Never touches a roster.
    pub fn cycle_form(&mut self) -> bool {
        let Slot::Shared(group) = GRID[self.cursor] else {
            return false;
        };
        let form = &mut self.forms[group as usize];
        *form = (*form + 1) % group.forms().len() as u8;
        true
    }

    /// Geometry plus displayed kind and label of any cell; `None` past the grid.
    pub fn info(&self, cell: usize) -> Option<CellInfo> {
        (cell < CELLS).then(|| self.describe(cell))
    }

    pub fn cursor_info(&self) -> CellInfo {
        self.describe(self.cursor)
    }

    fn describe(&self, cell: usize) -> CellInfo {
        let (row, column) = position(cell);
        let (label, content) = match GRID[cell] {
            Slot::Fighter(kind, name) => (name, CellContent::Fighter(kind)),
            Slot::Shared(group) => {
                let (kind, name) = group.forms()[usize::from(self.forms[group as usize])];
                (name, CellContent::Fighter(kind))
            }
            Slot::Unsupported(name) => (name, CellContent::Unsupported),
            Slot::RoleToggle => ("Player/CPU", CellContent::RoleToggle),
        };
        CellInfo {
            row,
            column,
            label,
            content,
        }
    }

    /// A: toggle the role on the control cell, otherwise append the displayed
    /// kind to `values`, the active role's `[all, ranks...]`. Unsupported cells
    /// and kinds missing from `kinds` change nothing.
    pub fn confirm(&mut self, kinds: &[u32], values: &mut [u8]) -> Outcome {
        match self.cursor_info().content {
            CellContent::RoleToggle => {
                self.toggle_role();
                Outcome::RoleToggled
            }
            CellContent::Fighter(kind) => {
                let changed = catalog_index(kinds, kind).is_some_and(|i| select_kind(values, i));
                if changed { Outcome::RosterChanged } else { Outcome::Unchanged }
            }
            CellContent::Unsupported => Outcome::Unchanged,
        }
    }

    /// Separate removal control for the displayed kind in the active role's `values`.
    pub fn remove(&self, kinds: &[u32], values: &mut [u8]) -> bool {
        self.cursor_info()
            .kind()
            .and_then(|kind| catalog_index(kinds, kind))
            .is_some_and(|i| deselect_kind(values, i))
    }
}

/// Position of `kind` in the caller's stable catalog. The catalog lives in the
/// consts crate, which depends on this one, so it is passed in.
pub fn catalog_index(kinds: &[u32], kind: u32) -> Option<usize> {
    kinds.iter().position(|candidate| *candidate == kind)
}

/// Cycle position (1-based) of a catalog entry, `None` when unselected or in All mode.
pub fn rank(values: &[u8], catalog_index: usize) -> Option<u8> {
    values
        .get(1..)?
        .get(catalog_index)
        .copied()
        .filter(|rank| *rank != 0)
}

/// Append a catalog entry to the cycle. A selected entry keeps its rank, and
/// choosing from All leaves exactly that one explicit member.
pub fn select_kind(values: &mut [u8], catalog_index: usize) -> bool {
    let Some((all, ranks)) = values.split_first_mut() else {
        return false;
    };
    let Some(&current) = ranks.get(catalog_index) else {
        return false;
    };
    let all_mode = *all != 0;
    if !all_mode && current != 0 {
        return false;
    }
    let count = if all_mode {
        0
    } else {
        ranks.iter().filter(|rank| **rank != 0).count()
    };
    let Ok(next) = u8::try_from(count + 1) else {
        return false;
    };
    if all_mode {
        *all = 0;
        ranks.fill(0);
    }
    ranks[catalog_index] = next;
    true
}

/// Remove a catalog entry and compact later ranks. The final explicit member
/// stays, and All mode has nothing to remove.
pub fn deselect_kind(values: &mut [u8], catalog_index: usize) -> bool {
    let Some((all, ranks)) = values.split_first_mut() else {
        return false;
    };
    let Some(&removed) = ranks.get(catalog_index) else {
        return false;
    };
    if *all != 0 || removed == 0 || ranks.iter().filter(|rank| **rank != 0).count() == 1 {
        return false;
    }
    ranks[catalog_index] = 0;
    for rank in ranks.iter_mut().filter(|rank| **rank > removed) {
        *rank -= 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the consts crate's `trio::CHOICES` kind order (85 stable positions).
    const CATALOG: [u32; 85] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e,
        0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56,
        0x57, 0x58, 0x59, 0x5c, 0x5d, // legacy 80 end here; forms are appended below
        0x5a, 0x5b, 0x24, 0x25, 0x26,
    ];

    /// Observed CSS rows with the audit's names; shared slots show their first form.
    const ROW_LABELS: [&[&str]; ROWS] = [
        &[
            "Mario", "Donkey Kong", "Link", "Samus", "Dark Samus", "Yoshi", "Kirby", "Fox",
            "Pikachu", "Luigi", "Ness", "Captain Falcon", "Jigglypuff",
        ],
        &[
            "Peach", "Daisy", "Bowser", "Ice Climbers", "Sheik", "Zelda", "Dr. Mario", "Pichu",
            "Falco", "Marth", "Lucina", "Young Link", "Ganondorf",
        ],
        &[
            "Mewtwo", "Roy", "Chrom", "Mr. Game & Watch", "Meta Knight", "Pit", "Dark Pit",
            "Zero Suit Samus", "Wario", "Snake", "Ike", "Squirtle", "Diddy Kong",
        ],
        &[
            "Lucas", "Sonic", "King Dedede", "Olimar", "Lucario", "R.O.B.", "Toon Link", "Wolf",
            "Villager", "Mega Man", "Wii Fit Trainer", "Rosalina & Luma", "Little Mac",
        ],
        &[
            "Greninja", "Palutena", "PAC-MAN", "Robin", "Shulk", "Bowser Jr.", "Duck Hunt", "Ryu",
            "Ken", "Cloud", "Corrin", "Bayonetta", "Inkling",
        ],
        &[
            "Ridley", "Simon", "Richter", "King K. Rool", "Isabelle", "Incineroar",
            "Piranha Plant", "Joker", "Hero", "Banjo & Kazooie", "Terry", "Byleth", "Min Min",
        ],
        &[
            "Steve", "Sephiroth", "Pyra", "Kazuya", "Sora", "Mii Brawler", "Mii Swordfighter",
            "Mii Gunner", "Player/CPU",
        ],
    ];

    fn grid_at(row: usize, column: usize) -> RosterGrid {
        let mut grid = RosterGrid::new(Role::Player);
        grid.cursor = cell_at(row, column);
        grid
    }

    fn named(grid: &RosterGrid, label: &str) -> usize {
        (0..CELLS)
            .find(|cell| grid.info(*cell).unwrap().label == label)
            .unwrap_or_else(|| panic!("no cell labeled {label}"))
    }

    fn go_to(grid: &mut RosterGrid, label: &str) {
        grid.cursor = named(grid, label);
    }

    fn at(grid: &RosterGrid) -> (usize, usize, &'static str) {
        let info = grid.cursor_info();
        (info.row, info.column, info.label)
    }

    fn explicit(values: &[u8]) -> Vec<usize> {
        let mut order: Vec<_> = values[1..]
            .iter()
            .enumerate()
            .filter(|(_, rank)| **rank != 0)
            .map(|(index, rank)| (*rank, index))
            .collect();
        order.sort_unstable();
        order.into_iter().map(|(_, index)| index).collect()
    }

    fn assert_valid(values: &[u8]) {
        let ranks = &values[1..];
        let count = ranks.iter().filter(|rank| **rank != 0).count();
        if values[0] != 0 {
            assert_eq!(count, 0, "All mode with explicit ranks: {values:?}");
        } else {
            let mut sorted: Vec<_> = ranks.iter().copied().filter(|rank| *rank != 0).collect();
            sorted.sort_unstable();
            assert!(!sorted.is_empty(), "empty roster: {values:?}");
            assert!(
                sorted.iter().copied().eq(1..=sorted.len() as u8),
                "ranks not contiguous: {values:?}"
            );
        }
    }

    fn all_mode() -> Vec<u8> {
        let mut values = vec![0; CATALOG.len() + 1];
        values[0] = 1;
        values
    }

    fn explicit_mode(catalog_indexes: &[usize]) -> Vec<u8> {
        let mut values = vec![0; CATALOG.len() + 1];
        for (rank, index) in catalog_indexes.iter().enumerate() {
            values[index + 1] = rank as u8 + 1;
        }
        values
    }

    #[test]
    fn physical_rows_match_the_observed_save_exactly() {
        let grid = RosterGrid::new(Role::Player);
        assert_eq!(CELLS, 87);
        for (row, labels) in ROW_LABELS.iter().enumerate() {
            let start = if row == 6 { 2 } else { 0 };
            let cells: Vec<_> = (0..CELLS)
                .map(|cell| grid.info(cell).unwrap())
                .filter(|info| info.row == row)
                .collect();
            assert_eq!(
                cells.iter().map(|info| info.label).collect::<Vec<_>>(),
                *labels,
                "row {row}"
            );
            assert_eq!(
                cells.iter().map(|info| info.column).collect::<Vec<_>>(),
                (start..start + labels.len()).collect::<Vec<_>>(),
                "row {row} columns"
            );
        }
        assert_eq!(grid.info(CELLS), None);
        // Spot checks at the row boundaries.
        for (row, column, label) in [
            (0, 0, "Mario"),
            (0, 12, "Jigglypuff"),
            (1, 0, "Peach"),
            (1, 3, "Ice Climbers"),
            (2, 11, "Squirtle"),
            (5, 12, "Min Min"),
            (6, 2, "Steve"),
            (6, 4, "Pyra"),
            (6, 6, "Sora"),
            (6, 7, "Mii Brawler"),
            (6, 10, "Player/CPU"),
        ] {
            assert_eq!(at(&grid_at(row, column)), (row, column, label));
        }
    }

    #[test]
    fn grid_covers_the_stable_catalog_once_and_marks_unsupported_cells() {
        let grid = RosterGrid::new(Role::Player);
        let mut seen = Vec::new();
        for group in [Group::Aegis, Group::PokemonTrainer] {
            seen.extend(group.forms().iter().map(|(kind, _)| *kind));
        }
        for cell in 0..CELLS {
            if let Slot::Fighter(kind, _) = GRID[cell] {
                seen.push(kind);
            }
        }
        seen.sort_unstable();
        let mut expected = CATALOG.to_vec();
        expected.sort_unstable();
        assert_eq!(seen, expected);

        let unsupported: Vec<_> = (0..CELLS)
            .map(|cell| grid.info(cell).unwrap())
            .filter(|info| info.content == CellContent::Unsupported)
            .map(|info| (info.row, info.column, info.label))
            .collect();
        assert_eq!(
            unsupported,
            [
                (1, 3, "Ice Climbers"),
                (6, 7, "Mii Brawler"),
                (6, 8, "Mii Swordfighter"),
                (6, 9, "Mii Gunner"),
            ]
        );
        for info in (0..CELLS).map(|cell| grid.info(cell).unwrap()) {
            let supported = matches!(info.content, CellContent::Fighter(_));
            assert_eq!(info.kind().is_some(), supported);
            if supported {
                assert!(catalog_index(&CATALOG, info.kind().unwrap()).is_some());
            }
        }
        assert_eq!(grid.info(CELLS - 1).unwrap().content, CellContent::RoleToggle);
        assert_eq!(catalog_index(&CATALOG, 0x5a), Some(80));
        assert_eq!(catalog_index(&CATALOG, 0x48), None);
    }

    #[test]
    fn horizontal_movement_wraps_inside_each_physical_row() {
        let mut grid = grid_at(0, 12);
        grid.step(Direction::Right);
        assert_eq!(at(&grid), (0, 0, "Mario"));
        grid.step(Direction::Left);
        assert_eq!(at(&grid), (0, 12, "Jigglypuff"));
        let mut grid = grid_at(6, 10);
        grid.step(Direction::Right);
        assert_eq!(at(&grid), (6, 2, "Steve"));
        grid.step(Direction::Left);
        assert_eq!(at(&grid), (6, 10, "Player/CPU"));
        grid.step(Direction::Left);
        assert_eq!(at(&grid), (6, 9, "Mii Gunner"));
        // Unsupported placeholders are ordinary stops.
        let mut grid = grid_at(1, 2);
        grid.step(Direction::Right);
        assert_eq!(at(&grid), (1, 3, "Ice Climbers"));
        grid.step(Direction::Right);
        assert_eq!(at(&grid), (1, 4, "Sheik"));
    }

    #[test]
    fn vertical_movement_keeps_the_nearest_physical_column() {
        let mut grid = grid_at(2, 11);
        grid.step(Direction::Down);
        assert_eq!(at(&grid), (3, 11, "Rosalina & Luma"));
        grid.step(Direction::Up);
        grid.step(Direction::Up);
        assert_eq!(at(&grid), (1, 11, "Young Link"));
        // Into the centered last row: clamp to its first and last physical columns.
        for (from, to) in [
            (0, (6, 2, "Steve")),
            (1, (6, 2, "Steve")),
            (2, (6, 2, "Steve")),
            (4, (6, 4, "Pyra")),
            (9, (6, 9, "Mii Gunner")),
            (10, (6, 10, "Player/CPU")),
            (11, (6, 10, "Player/CPU")),
            (12, (6, 10, "Player/CPU")),
        ] {
            let mut grid = grid_at(5, from);
            grid.step(Direction::Down);
            assert_eq!(at(&grid), to, "down from column {from}");
        }
        // Out of it, and wrapping through the top and bottom rows.
        for column in 2..=10 {
            let mut grid = grid_at(6, column);
            grid.step(Direction::Up);
            assert_eq!((grid.cursor_info().row, grid.cursor_info().column), (5, column));
            let mut grid = grid_at(6, column);
            grid.step(Direction::Down);
            assert_eq!((grid.cursor_info().row, grid.cursor_info().column), (0, column));
        }
        let mut grid = grid_at(0, 0);
        grid.step(Direction::Up);
        assert_eq!(at(&grid), (6, 2, "Steve"));
        let mut grid = grid_at(0, 12);
        grid.step(Direction::Up);
        assert_eq!(at(&grid), (6, 10, "Player/CPU"));
    }

    #[test]
    fn group_previews_are_independent_and_move_with_the_cell() {
        let mut grid = RosterGrid::new(Role::Cpu);
        go_to(&mut grid, "Squirtle");
        assert!(grid.cycle_form());
        assert_eq!(grid.cursor_info().kind(), Some(0x25));
        assert_eq!(grid.cursor_info().label, "Ivysaur");
        // The Aegis slot is untouched by the Pokemon Trainer preview.
        assert_eq!(grid.info(cell_at(6, 4)).unwrap().kind(), Some(0x5a));
        go_to(&mut grid, "Pyra");
        assert!(grid.cycle_form());
        assert_eq!(grid.cursor_info().kind(), Some(0x5b));
        assert_eq!(grid.cursor_info().label, "Mythra");
        assert_eq!(grid.info(cell_at(2, 11)).unwrap().kind(), Some(0x25));
        assert!(grid.cycle_form());
        assert_eq!(grid.cursor_info().kind(), Some(0x5a));
        go_to(&mut grid, "Ivysaur");
        assert!(grid.cycle_form());
        assert_eq!(grid.cursor_info().kind(), Some(0x26));
        assert!(grid.cycle_form());
        assert_eq!(grid.cursor_info().kind(), Some(0x24));
        // A preview survives navigating away and back.
        grid.cycle_form();
        grid.step(Direction::Right);
        grid.step(Direction::Left);
        assert_eq!(grid.cursor_info().kind(), Some(0x25));
        assert_eq!(grid.role(), Role::Cpu);
    }

    #[test]
    fn form_cycling_ignores_other_cells_and_never_edits_a_roster() {
        let mut grid = RosterGrid::new(Role::Player);
        let before = grid;
        for label in ["Mario", "Ice Climbers", "Mii Gunner", "Player/CPU", "Sora"] {
            go_to(&mut grid, label);
            let moved = grid;
            assert!(!grid.cycle_form(), "{label}");
            assert_eq!(grid, moved, "{label}");
        }
        assert_ne!(grid, before);
        let values = explicit_mode(&[0]);
        let mut untouched = values.clone();
        go_to(&mut grid, "Pyra");
        grid.cycle_form();
        grid.cycle_form();
        assert_eq!(untouched, values);
        // Only A commits the displayed form.
        grid.cycle_form();
        assert_eq!(grid.confirm(&CATALOG, &mut untouched), Outcome::RosterChanged);
        assert_eq!(rank(&untouched, 81), Some(2));
        assert_eq!(rank(&untouched, 80), None);
    }

    #[test]
    fn role_toggle_cell_flips_role_without_touching_either_roster() {
        let mut rosters = [explicit_mode(&[0, 1]), explicit_mode(&[7])];
        let snapshot = rosters.clone();
        let mut grid = RosterGrid::new(Role::Player);
        go_to(&mut grid, "Player/CPU");
        for expected in [Role::Cpu, Role::Player, Role::Cpu] {
            let active = grid.role() as usize;
            assert_eq!(grid.confirm(&CATALOG, &mut rosters[active]), Outcome::RoleToggled);
            assert_eq!(grid.role(), expected);
        }
        assert_eq!(rosters, snapshot);
        assert_eq!(grid.cursor_info().kind(), None);
        assert!(!grid.remove(&CATALOG, &mut rosters[0]));
        assert_eq!(rosters, snapshot);
        // Roles keep separate arrays: the same shared cell can hold different forms.
        let mut grid = RosterGrid::new(Role::Player);
        go_to(&mut grid, "Pyra");
        let active = grid.role() as usize;
        assert_eq!(grid.confirm(&CATALOG, &mut rosters[active]), Outcome::RosterChanged);
        grid.toggle_role();
        grid.cycle_form();
        let active = grid.role() as usize;
        assert_eq!(grid.confirm(&CATALOG, &mut rosters[active]), Outcome::RosterChanged);
        // Both badges read the shown kind's ranks, whichever role is active.
        assert_eq!((rank(&rosters[0], 80), rank(&rosters[1], 80)), (Some(3), None));
        assert_eq!((rank(&rosters[0], 81), rank(&rosters[1], 81)), (None, Some(2)));
    }

    #[test]
    fn select_appends_once_and_preserves_existing_rank() {
        let mut values = explicit_mode(&[3, 9]);
        assert!(select_kind(&mut values, 5));
        assert_eq!(explicit(&values), [3, 9, 5]);
        let before = values.clone();
        assert!(!select_kind(&mut values, 3));
        assert!(!select_kind(&mut values, 9));
        assert_eq!(values, before);
        assert_eq!(rank(&values, 3), Some(1));
        assert_eq!(rank(&values, 5), Some(3));
        assert_eq!(rank(&values, 4), None);
    }

    #[test]
    fn choosing_from_all_leaves_exactly_that_explicit_member() {
        let mut values = all_mode();
        assert_eq!(rank(&values, 12), None);
        assert!(select_kind(&mut values, 12));
        assert_eq!(values[0], 0);
        assert_eq!(explicit(&values), [12]);
        assert_eq!(rank(&values, 12), Some(1));
        assert_valid(&values);
        // Through the grid, from a shared slot.
        let mut values = all_mode();
        let mut grid = RosterGrid::new(Role::Cpu);
        go_to(&mut grid, "Pyra");
        assert_eq!(grid.cursor_info().kind(), Some(0x5a));
        grid.cycle_form();
        assert_eq!(grid.confirm(&CATALOG, &mut values), Outcome::RosterChanged);
        assert_eq!(explicit(&values), [81]);
        assert_eq!(values[0], 0);
    }

    #[test]
    fn deselect_compacts_ranks_and_keeps_the_last_member() {
        let mut values = explicit_mode(&[4, 1, 8, 2]);
        assert!(deselect_kind(&mut values, 1));
        assert_eq!(explicit(&values), [4, 8, 2]);
        assert_eq!(rank(&values, 8), Some(2));
        assert_eq!(rank(&values, 2), Some(3));
        assert!(!deselect_kind(&mut values, 1), "already removed");
        assert!(deselect_kind(&mut values, 4));
        assert!(deselect_kind(&mut values, 2));
        assert_eq!(explicit(&values), [8]);
        let last = values.clone();
        assert!(!deselect_kind(&mut values, 8), "final explicit member stays");
        assert_eq!(values, last);
        assert_valid(&values);
        // All mode has nothing to remove.
        let mut all = all_mode();
        assert!(!deselect_kind(&mut all, 0));
        assert_eq!(all, all_mode());
    }

    #[test]
    fn invalid_catalog_indexes_and_short_arrays_change_nothing() {
        let mut values = explicit_mode(&[0]);
        let before = values.clone();
        for index in [CATALOG.len(), CATALOG.len() + 1, usize::MAX] {
            assert!(!select_kind(&mut values, index));
            assert!(!deselect_kind(&mut values, index));
            assert_eq!(rank(&values, index), None);
        }
        assert_eq!(values, before);
        let mut empty: [u8; 0] = [];
        assert!(!select_kind(&mut empty, 0));
        assert!(!deselect_kind(&mut empty, 0));
        assert_eq!(rank(&empty, 0), None);
        let mut only_flag = [0u8];
        assert!(!select_kind(&mut only_flag, 0));
        // A kind the caller's catalog lacks is ignored by the grid.
        let mut grid = RosterGrid::new(Role::Player);
        go_to(&mut grid, "Mario");
        assert_eq!(grid.confirm(&[0x01, 0x02], &mut values), Outcome::Unchanged);
        assert!(!grid.remove(&[0x01, 0x02], &mut values));
        assert_eq!(values, before);
    }

    #[test]
    fn grid_confirm_and_remove_follow_the_catalog_not_the_cell_order() {
        let mut values = vec![0u8; CATALOG.len() + 1];
        values[1] = 1; // Mario is the one explicit member.
        let mut grid = RosterGrid::new(Role::Player);
        // Sora is the 80th cell but catalog position 79; Charizard is catalog 84.
        go_to(&mut grid, "Sora");
        assert_eq!(grid.confirm(&CATALOG, &mut values), Outcome::RosterChanged);
        assert_eq!(rank(&values, 79), Some(2));
        go_to(&mut grid, "Squirtle");
        grid.cycle_form();
        grid.cycle_form();
        assert_eq!(grid.cursor_info().label, "Charizard");
        assert_eq!(grid.confirm(&CATALOG, &mut values), Outcome::RosterChanged);
        assert_eq!(rank(&values, 84), Some(3));
        assert_eq!(grid.confirm(&CATALOG, &mut values), Outcome::Unchanged);
        assert_eq!(explicit(&values), [0, 79, 84]);
        go_to(&mut grid, "Ice Climbers");
        assert_eq!(grid.confirm(&CATALOG, &mut values), Outcome::Unchanged);
        assert!(!grid.remove(&CATALOG, &mut values));
        go_to(&mut grid, "Sora");
        assert!(grid.remove(&CATALOG, &mut values));
        assert_eq!(explicit(&values), [0, 84]);
        assert_eq!(rank(&values, 84), Some(2));
        assert_valid(&values);
    }

    #[test]
    fn mutation_sequences_always_leave_a_valid_roster() {
        // Deterministic pseudo-random edits checked against a plain ordered list.
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut values = all_mode();
        let mut model: Vec<usize> = Vec::new();
        for step in 0..4000 {
            let index = next() as usize % 12; // a small pool forces repeats
            let command = next() % 40;
            if command == 0 {
                values = all_mode();
                model.clear();
            } else if command < 22 {
                let changed = select_kind(&mut values, index);
                assert_eq!(changed, !model.contains(&index), "step {step}");
                if changed {
                    model.push(index);
                }
            } else {
                let changed = deselect_kind(&mut values, index);
                let allowed = model.contains(&index) && model.len() > 1;
                assert_eq!(changed, allowed, "step {step}");
                if changed {
                    model.retain(|member| *member != index);
                }
            }
            assert_valid(&values);
            assert_eq!(explicit(&values), model, "step {step}");
            assert_eq!(values[0] != 0, model.is_empty(), "step {step}");
        }
    }
}
