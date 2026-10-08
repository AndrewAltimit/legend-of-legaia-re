//! Community language packs: disc text out to an editable YAML pack and a
//! filled pack back in as an in-place reimport (dialog, UI strings, names,
//! textures and fonts), plus the space and budget accounting behind it and
//! the PAL lift. See `docs/tooling/translation/`.
//!
//! `legaia-patcher` re-exports every module here at its old path. Doc links
//! that pointed back up at it are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate.

#![forbid(unsafe_code)]

// Items these files name as `crate::...`, which live in a crate below this
// one; binding them here keeps the moved files' paths unchanged.
use legaia_disc_patch::disc;
use legaia_disc_patch::man_compressed_budget;
use legaia_disc_patch::space_ledger;

pub mod translation;
