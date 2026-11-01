//! Coordinates into the player's disc. A pack names disc content only
//! through these; the engine resolves them at load time.
//!
//! Two levels of checking apply. [`DiscRef::check_range`] needs no disc: it
//! rejects coordinates no NA disc can satisfy (monster id 0, a PROT entry
//! past the last, a malformed scene name). [`DiscResolver`] is the
//! disc-backed half the engine implements: does the coordinate name the
//! kind of thing the pack says it does on *this* disc.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A reference to something on the retail disc.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// A retail field NPC: placement `index` of scene `scene`'s MAN actor
    /// list (partition 1; `0` is the scene controller, so NPCs are `1..`).
    /// The placement's model and idle clip are what a pack NPC borrows; the
    /// site's NPC page lists the same index as an NPC's `slot`.
    Npc { scene: String, index: u16 },
}

/// The monster archive's record count (PROT 867); ids are `1..=MONSTER_COUNT`.
pub const MONSTER_COUNT: u16 = 186;
/// `PROT.DAT`'s entry count; extraction indices are `0..PROT_ENTRY_COUNT`.
pub const PROT_ENTRY_COUNT: u16 = 1233;
/// The sound-test list's length; BGM ids are `0..BGM_COUNT`.
pub const BGM_COUNT: u16 = 81;
/// The longest scene name accepted. CDNAME names are short lowercase words;
/// the bound only rejects obvious garbage before a disc lookup.
pub const MAX_SCENE_NAME_LEN: usize = 32;

impl DiscRef {
    /// The `kind` tag as written in TOML (`monster`, `scene`, ...).
    pub fn kind(&self) -> &'static str {
        match self {
            DiscRef::Monster { .. } => "monster",
            DiscRef::Scene { .. } => "scene",
            DiscRef::Tmd { .. } => "tmd",
            DiscRef::Tim { .. } => "tim",
            DiscRef::Item { .. } => "item",
            DiscRef::Bgm { .. } => "bgm",
            DiscRef::Npc { .. } => "npc",
        }
    }

    /// The PROT extraction entry the coordinate lives in, for the kinds
    /// addressed that way.
    pub fn prot_entry(&self) -> Option<u16> {
        match self {
            DiscRef::Tmd { prot, .. } | DiscRef::Tim { prot, .. } => Some(*prot),
            _ => None,
        }
    }

    /// Reject coordinates that are out of range on every NA disc. Needs no
    /// disc; a coordinate that passes may still fail [`DiscResolver`].
    pub fn check_range(&self) -> Result<(), String> {
        match self {
            DiscRef::Monster { id } if !(1..=MONSTER_COUNT).contains(id) => {
                Err(format!("monster id {id} is outside 1..={MONSTER_COUNT}"))
            }
            DiscRef::Scene { name } if !is_valid_scene_name(name) => Err(format!(
                "scene name {name:?} is not a CDNAME name ([a-z0-9_], at most {MAX_SCENE_NAME_LEN} characters)"
            )),
            DiscRef::Tmd { prot, .. } | DiscRef::Tim { prot, .. } if *prot >= PROT_ENTRY_COUNT => {
                Err(format!(
                    "PROT entry {prot} is past the last entry ({})",
                    PROT_ENTRY_COUNT - 1
                ))
            }
            DiscRef::Bgm { id } if *id >= BGM_COUNT => {
                Err(format!("bgm id {id} is outside 0..{BGM_COUNT}"))
            }
            DiscRef::Npc { scene, .. } if !is_valid_scene_name(scene) => Err(format!(
                "scene name {scene:?} is not a CDNAME name ([a-z0-9_], at most {MAX_SCENE_NAME_LEN} characters)"
            )),
            DiscRef::Npc { index: 0, .. } => Err(
                "NPC placement 0 is the scene controller; NPC placements start at 1".to_string(),
            ),
            _ => Ok(()),
        }
    }
}

impl fmt::Display for DiscRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscRef::Monster { id } => write!(f, "monster {id}"),
            DiscRef::Scene { name } => write!(f, "scene {name}"),
            DiscRef::Tmd { prot, index } => write!(f, "tmd PROT {prot:04} #{index}"),
            DiscRef::Tim { prot, index } => write!(f, "tim PROT {prot:04} #{index}"),
            DiscRef::Item { id } => write!(f, "item 0x{id:02X}"),
            DiscRef::Bgm { id } => write!(f, "bgm {id}"),
            DiscRef::Npc { scene, index } => write!(f, "npc {scene} #{index}"),
        }
    }
}

/// Is `name` shaped like a CDNAME scene name (`town01`, `init_data`).
pub fn is_valid_scene_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SCENE_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Disc-backed resolution of a [`DiscRef`]: the engine implements this over
/// the player's disc (`engine-scene`'s PROT index and catalogs). Returning
/// `Err` carries a message for the validation report.
pub trait DiscResolver {
    /// Does `r` name something of its kind on this disc.
    fn resolve(&self, r: &DiscRef) -> Result<(), String>;

    /// How many meshes scene `scene`'s environment geometry pack holds -
    /// the index space of a map's `{ object = N }` props and `mesh`
    /// grounds. `None` when the resolver cannot tell, which skips the check.
    fn scene_geometry_len(&self, _scene: &str) -> Option<usize> {
        None
    }

    /// How many battle-stage streams scene `scene`'s block carries - the
    /// index space of a `stage = { scene, index }`. `None` when the
    /// resolver cannot tell, which skips the check.
    fn battle_stage_count(&self, _scene: &str) -> Option<usize> {
        None
    }

    /// The placed shape of a map's mesh (`base_scene` is the map's base,
    /// which a `{ object = N }` indexes) turned by `rotation` (degrees) and
    /// scaled: what the resting check reads
    /// ([`crate::map_rest::rest_issues`]). `None` when the resolver cannot
    /// tell, which leaves the item unjudged.
    fn mesh_shape(
        &self,
        _base_scene: &str,
        _mesh: &crate::map::PropMesh,
        _rotation: [f32; 3],
        _scale: f32,
    ) -> Option<crate::map_rest::Shape> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_checks() {
        assert!(DiscRef::Monster { id: 1 }.check_range().is_ok());
        assert!(DiscRef::Monster { id: MONSTER_COUNT }.check_range().is_ok());
        assert!(DiscRef::Monster { id: 0 }.check_range().is_err());
        assert!(
            DiscRef::Monster {
                id: MONSTER_COUNT + 1
            }
            .check_range()
            .is_err()
        );
        assert!(
            DiscRef::Tmd {
                prot: 1232,
                index: 0
            }
            .check_range()
            .is_ok()
        );
        assert!(
            DiscRef::Tim {
                prot: 1233,
                index: 0
            }
            .check_range()
            .is_err()
        );
        assert!(DiscRef::Bgm { id: 80 }.check_range().is_ok());
        assert!(DiscRef::Bgm { id: 81 }.check_range().is_err());
        assert!(
            DiscRef::Scene {
                name: "town01".into()
            }
            .check_range()
            .is_ok()
        );
        assert!(
            DiscRef::Scene {
                name: "Town01".into()
            }
            .check_range()
            .is_err()
        );
        assert!(
            DiscRef::Scene {
                name: String::new()
            }
            .check_range()
            .is_err()
        );
    }

    #[test]
    fn toml_shape() {
        #[derive(Deserialize)]
        struct W {
            r: DiscRef,
        }
        let w: W = toml::from_str("r = { kind = \"tmd\", prot = 874, index = 3 }").unwrap();
        assert_eq!(
            w.r,
            DiscRef::Tmd {
                prot: 874,
                index: 3
            }
        );
        assert_eq!(w.r.kind(), "tmd");
        assert_eq!(w.r.prot_entry(), Some(874));
        assert_eq!(w.r.to_string(), "tmd PROT 0874 #3");
    }
}
