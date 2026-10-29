//! Coordinates into the player's disc. A pack names disc content only
//! through these; the engine resolves them at load time.

use serde::{Deserialize, Serialize};

/// A reference to something on the retail disc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscRef {
    /// A retail monster by its 1-based PROT 867 record id.
    Monster { id: u16 },
    /// A retail scene by its CDNAME name (`town01`).
    Scene { name: String },
    /// The `index`th TMD object in PROT extraction entry `prot`.
    Tmd { prot: u16, index: u16 },
    /// A TIM in PROT extraction entry `prot`, `index`th in its pack.
    Tim { prot: u16, index: u16 },
    /// A retail item id (the `PTR_DAT_8007436C` id space).
    Item { id: u8 },
    /// A music cue by its sound-test id.
    Bgm { id: u16 },
}
