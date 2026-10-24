//! Accepted lobby choices contain values only, never native binding/resource authority.
use crate::lifetime::Configuration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value<F, P> {
    pub fighter: F,
    pub forms: P,
    pub cursor: Option<usize>,
    pub configuration: Option<Configuration>,
    pub owned: bool,
    pub original: bool,
}

impl<F: PartialEq, P: PartialEq> Value<F, P> {
    pub fn ordinary(fighter: F, forms: P, cursor: Option<usize>) -> Self {
        Self { fighter, forms, cursor, configuration: None, owned: false, original: false }
    }

    /// Original slot0 remains the newly captured native origin. A differing remembered
    /// value is restored explicitly rather than silently replaced by that new origin.
    pub fn matches_original(&self, fighter: F, forms: P, configuration: Option<Configuration>) -> bool {
        self.original && self.fighter == fighter && self.forms == forms
            && self.configuration == configuration
    }
}
