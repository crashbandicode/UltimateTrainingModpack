//! Persisted stage choice is independent of the ordered rotation's ranks.
use serde::{Deserialize, Serialize};

/// Shared with the native stage panel: paging moves the cursor, never ranks.
pub const PAGE_ROWS: usize = 4;
pub const PAGE_COLUMNS: usize = crate::NX_SUBMENU_COLUMNS;
pub const PAGE_SIZE: usize = PAGE_ROWS * PAGE_COLUMNS;

/// Wrap pages, retaining the cell where possible and clamping on the final page.
pub(crate) fn page_cursor(selected: usize, count: usize, next: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let selected = selected.min(count - 1);
    let pages = count.div_ceil(PAGE_SIZE);
    let page = selected / PAGE_SIZE;
    let target = if next { (page + 1) % pages } else { (page + pages - 1) % pages };
    Some((target * PAGE_SIZE + selected % PAGE_SIZE).min(count - 1))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Rotation,
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub mode: Mode,
    /// Native catalog ID, never a page/cell index. Validated by the catalog owner.
    pub direct_stage: u32,
}

impl Default for Preferences {
    fn default() -> Self { Self { mode: Mode::Rotation, direct_stage: 311 } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_wraps_and_clamps_the_short_final_page() {
        // All + 117 named stages occupy seven full pages and six final cells.
        assert_eq!(page_cursor(15, 118, true), Some(31));
        assert_eq!(page_cursor(111, 118, true), Some(117));
        assert_eq!(page_cursor(117, 118, true), Some(5));
        assert_eq!(page_cursor(0, 118, false), Some(112));
        assert_eq!(page_cursor(15, 118, false), Some(117));
        assert_eq!(page_cursor(117, 118, false), Some(101));
    }

    #[test]
    fn paging_a_single_or_empty_page_is_inert() {
        for next in [false, true] {
            assert_eq!(page_cursor(0, 0, next), None);
            assert_eq!(page_cursor(0, 1, next), Some(0));
            assert_eq!(page_cursor(3, 6, next), Some(3));
        }
    }
}
