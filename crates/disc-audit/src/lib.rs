//! Tooling that measures the disc rather than parsing one format: per-entry
//! byte accounting, bulk classification of PROT entries, the TIM catalogs and
//! their labels, where an overlay image stops being its own content, the
//! overlay jump-table and probe-table readers the accounting claims with, and
//! the disc-wide formation census.
//!
//! This crate is the top of the format stack: `byte_account` and `categorize`
//! call nearly every parser there is, so it depends on all the format crates
//! and none of them may depend on it.

#![forbid(unsafe_code)]

pub mod battle_jump_tables;
pub mod byte_account;
pub mod categorize;
pub mod field_probe_tables;
pub mod formation_census;
pub mod inherited_tail;
pub mod other3_roster;
pub mod switch_tables;
pub mod tim_catalog;
pub mod tim_deep_catalog;
pub mod tim_labels;
