//! Pending creation is an owned intent, never a zero/partially populated row.
use super::{inventory::{self, MiiClass}, value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// The session and resume slot already owned a fixed 432-byte row. Keep that
// bounded value inline: refining an intent must not allocate on the native
// handoff, and snapshots must remain independent copies.
#[allow(clippy::large_enum_variant)]
pub(super) enum Draft {
    NativeDefault(MiiClass),
    Ready(value::Row),
}

impl Draft {
    pub(super) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::NativeDefault(_) => Ok(()),
            Self::Ready(row) => value::parse(row).map(|_| ()),
        }
    }

    pub(super) fn row(&self) -> Option<&value::Row> {
        match self { Self::Ready(row) => Some(row), Self::NativeDefault(_) => None }
    }

    pub(super) fn hash(&self) -> u64 {
        self.row().map_or(0, |row| inventory::fnv1a64(row))
    }

    /// Refine only the matching pending intent. A completed value never selects
    /// a new default on resume, even if native defaults or the catalog change.
    pub(super) fn materialize(&mut self, class: MiiClass, row: value::Row) -> Result<(), &'static str> {
        if *self != Self::NativeDefault(class) { return Err("default_intent_changed"); }
        if row[..12] != [0; 12] { return Err("default_identity_not_zero"); }
        let (_, config) = value::parse(&row)?;
        if config.canonical().class != class { return Err("default_class_mismatch"); }
        *self = Self::Ready(row);
        Ok(())
    }
}
