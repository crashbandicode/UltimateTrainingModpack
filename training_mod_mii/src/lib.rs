//! Owned saved-Mii identity, discovery and selection. No native pointers or game writes.
#![forbid(unsafe_code)]
pub mod inventory;
pub mod backend;
pub mod remembered;
pub mod lifetime;
pub mod native_values;
pub mod ordinary_player;
pub mod role_adapter;
pub mod saved;
pub mod selection;

pub mod ordinary_cpu;
pub mod draft;

pub mod draft_store;
pub mod role_owner;
pub mod auto_init;
pub mod catalog_request;
pub mod replay;
pub mod editor_row;
