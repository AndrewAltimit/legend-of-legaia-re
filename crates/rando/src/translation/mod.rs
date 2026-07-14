//! Translation / language-pack pipeline.
//!
//! Turns the disc's user-facing text into an editable YAML **language pack**
//! and applies a filled pack back as a same-size in-place patch:
//!
//! ```text
//! legaia-rando translate export --input DISC.bin -o legaia_en.yaml
//! legaia-rando translate init   --lang fr --from legaia_en.yaml -o legaia_fr.yaml
//! # ... fill the `translation:` fields ...
//! legaia-rando translate stats  --pack legaia_fr.yaml
//! legaia-rando translate import --input DISC.bin --pack legaia_fr.yaml \
//!     --output DISC_fr.bin --patch legaia_fr.ppf
//! ```
//!
//! - [`markup`] - the reversible text <-> game-byte codec (retail glyphs are
//!   printable ASCII; everything else is a `{xx}` / `{xx:yy}` escape).
//! - [`segments`] - the `0x1F <text> 0x00` dialog-segment scanner shared by
//!   export and import.
//! - [`pack`] - the YAML schema ([`pack::LanguagePack`]) + coverage stats.
//! - [`export`] - disc -> pack (SCUS name tables, scene-bundle MAN dialog,
//!   raw event-script text).
//! - [`import`] - pack -> patched disc via [`crate::disc::DiscPatcher`],
//!   with per-entry encodability / budget / provenance diagnostics.
//!
//! No Sony bytes ship with this crate: packs are generated from the user's
//! own disc, and exported packs (which contain game text) must not be
//! committed - see `docs/tooling/translation.md`.

pub mod export;
pub mod import;
pub mod markup;
pub mod pack;
pub mod segments;

pub use export::export_pack;
pub use import::{ImportReport, import_pack};
pub use pack::{Entry, LanguagePack, PACK_FORMAT};
