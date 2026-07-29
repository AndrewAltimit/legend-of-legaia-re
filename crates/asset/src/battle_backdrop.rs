//! How retail places a battle-stage backdrop shell: **two** copies of one
//! registered TMD, and **not** every object in it.
//!
//! A `scene_tmd_stream` PROT entry holds one backdrop shell authored as a
//! half - object 0's vertex pool sits entirely on one side of `X = 0` or
//! `Z = 0` (see [`shell_shape`]). That half is what the file holds, and it
//! is complete; nothing is truncated on the way in. What completes the
//! circle is a **second draw of the same mesh under a second transform**,
//! and the transform is a per-stage decision the executable carries as a
//! table.
//!
//! [`shell_shape`]: crate::scene_tmd_stream::shell_shape
//!
//! # Battle init
//!
//! `FUN_800513F0` registers the backdrop TMD **once**
//! (`80051a60 jal 0x80026b4c`, handle stored at `DAT_80076810`) and then
//! allocates **two** actors from the same descriptor - `80051a7c` and
//! `80051aa8`, both `jal 0x80020de0` with `a0 = 0x8007680c` - parking their
//! pointers at `battle_ctx + 0x106C` (copy A) and `+0x1070` (copy B).
//! `FUN_80050120` drives the pair in lockstep: the depth-cue ramp at
//! `+0x78` and the draw-mode selector at `+0x56` are written to both on the
//! same path (`80050848..80050880`).
//!
//! Copy A is placed at raw coordinates. Copy B gets a transform:
//!
//! - **default** - `80051bc0 li v0,0x800` / `80051bc4 sh v0,0x26(v1)` writes
//!   `+0x26 = 0x800`. `+0x26` is the actor's world-Y angle (the second of
//!   the three half-words `FUN_80026988` reads at `actor + 0x24`; that
//!   kernel writes `sin` of it bare into matrix element `[0][2]` and `cos`
//!   into `[2][2]`, which only a Y rotation does). `0x800` of the `0x1000`
//!   full turn is exactly 180 degrees: [`SecondCopy::HalfTurn`].
//! - **per-stage exception** - `80051bc8..80051c18` walks the
//!   zero-terminated `u16` table at `DAT_80078B50`, comparing each entry
//!   against the backdrop id. On a hit (`80051cc4`) it writes `+0x5A = 2`
//!   and `+0x26 = 0` instead. `FUN_8001ADA4` case 3 turns `+0x5A & 2` into
//!   `_DAT_1F800348 = -0x1000` (`8001af28..8001af34`) and calls
//!   `FUN_8005B4E8` (`ScaleMatrix`) - X scale `-1.0` in 4.12, a reflection
//!   in the YZ plane: [`SecondCopy::MirrorX`]. The same predicate
//!   (`+0x5A & 0xE` at `8001afd8`) negates the per-object rotation argument
//!   and swaps the draw-call mode word from `0x40000000` to `0x48000000`,
//!   which is the winding compensation a negative-determinant transform
//!   needs.
//!
//! Both copies are drawn: `FUN_80050120` sets `+0x56 = 3` on each, which is
//! the `FUN_8001ADA4` dispatch selector (`8001ae60 lhu v0,0x56(s0)`, jump
//! table at `0x8001042C`).
//!
//! # Which objects draw
//!
//! Immediately after allocating the pair, `80051ad4..80051bac` decrements
//! each actor's object count at `**(actor + 0x44)` and left-shifts the
//! pointer array by one **from index 1**: `A[i] = B[i+1]` and
//! `B[i] = B[i+1]` for `i >= 1`. The surviving list is objects
//! `0, 2, 3, ...` - **object 1 is dropped**. It is still resident in the
//! relocated object table, just unreferenced by either actor.
//!
//! The whole block is gated on `DAT_8007B64B == 0` (`80051abc` /
//! `80051acc`). That byte is bit 5 of byte `+8` of the field scene's
//! encounter-region record (`801DA09C..801DA0AC` in the field battle-intro
//! overlay), the same byte whose low 5 bits pick which of a scene's stage
//! variants to use. Set, it keeps object 1.
//!
//! # Id space
//!
//! The value compared against the table is `word[0x80084540] +
//! byte[0x8007BD60] & 0x7F` (`80051a20..80051a40`), a small stage id - not
//! a pointer. It maps onto the PROT extraction index by
//! [`RUNTIME_ID_TO_PROT_OFFSET`]. Every one of the table's distinct ids
//! resolves to a `scene_tmd_stream` entry under that offset, which is what
//! pins it.
//!
//! # The sibling table
//!
//! `80051c1c..80051c6c` scans a **second** table at `DAT_80078C1C` the same
//! way and against the same id, but it touches neither actor - it sets a
//! byte at `0x8007BDA8`, which `FUN_80050120` reads to pick the backdrop's
//! depth-cue ceiling (`0x800` vs `0xC00`) and far-colour scaling. Its 13
//! ids are the wide-open outdoor stages. It is deliberately not modelled
//! here: it is a fog parameter, not a placement one.

use crate::scene_tmd_stream;

/// Virtual address of the mirror-X stage table in `SCUS_942.54`.
pub const MIRROR_X_TABLE_VA: u32 = 0x8007_8B50;

/// Byte offset of [`MIRROR_X_TABLE_VA`] inside the `SCUS_942.54` file
/// image (`va - t_addr + 0x800`, the PS-X EXE header being `0x800` bytes).
pub const MIRROR_X_TABLE_SCUS_OFFSET: usize = 0x6_9350;

/// Upper bound on the zero-terminated table's length, so a mis-aimed
/// offset cannot walk the whole executable.
const MIRROR_X_TABLE_MAX_SLOTS: usize = 256;

/// Retail's stage id plus this is the entry's PROT extraction index.
pub const RUNTIME_ID_TO_PROT_OFFSET: u32 = 3;

/// The transform retail applies to the **second** copy of a backdrop shell.
///
/// Both alternatives complete a shell whose open side faces `-X` or `+X`.
/// Only [`HalfTurn`] completes one whose open side faces `-Z`, because a
/// `-Z`-open shell is symmetric about `X = 0` and so is mapped onto itself
/// by [`MirrorX`]. Retail's table respects that: no `-Z`-open stage is on
/// the mirror list.
///
/// [`HalfTurn`]: SecondCopy::HalfTurn
/// [`MirrorX`]: SecondCopy::MirrorX
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondCopy {
    /// `actor + 0x26 = 0x800` - a half turn about world Y.
    HalfTurn,
    /// `actor + 0x5A = 2` - X scale `-1`, a reflection in the YZ plane.
    MirrorX,
}

impl SecondCopy {
    /// Per-axis scale of the equivalent diagonal matrix. Both retail
    /// transforms are exact at the integer level, so no trigonometry is
    /// involved: a half turn about Y is `diag(-1, 1, -1)`.
    pub const fn scale(self) -> [f32; 3] {
        match self {
            SecondCopy::HalfTurn => [-1.0, 1.0, -1.0],
            SecondCopy::MirrorX => [-1.0, 1.0, 1.0],
        }
    }

    /// Whether the transform has negative determinant and so reverses
    /// triangle winding. Retail compensates for exactly this case by
    /// swapping the draw-call mode word (`0x40000000` -> `0x48000000`).
    pub const fn flips_winding(self) -> bool {
        matches!(self, SecondCopy::MirrorX)
    }

    /// Short label for a viewer status line.
    pub const fn label(self) -> &'static str {
        match self {
            SecondCopy::HalfTurn => "half-turn",
            SecondCopy::MirrorX => "mirrored",
        }
    }
}

/// The `SCUS_942.54` table of stages whose second backdrop copy is
/// mirrored rather than half-turned.
#[derive(Debug, Clone, Default)]
pub struct MirrorXTable {
    ids: Vec<u16>,
}

impl MirrorXTable {
    /// Parse the zero-terminated `u16` table out of a `SCUS_942.54` image.
    ///
    /// Returns `None` when the buffer is too short to hold the table's
    /// first slot, or when the table does not terminate within
    /// [`MIRROR_X_TABLE_MAX_SLOTS`] - either means this is not the retail
    /// USA executable and the caller should fall back to the default
    /// transform rather than trust a garbage list.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let mut ids = Vec::new();
        let mut off = MIRROR_X_TABLE_SCUS_OFFSET;
        for _ in 0..MIRROR_X_TABLE_MAX_SLOTS {
            let raw = scus.get(off..off + 2)?;
            let v = u16::from_le_bytes([raw[0], raw[1]]);
            if v == 0 {
                return Some(Self { ids });
            }
            ids.push(v);
            off += 2;
        }
        None
    }

    /// Table slots in file order. Retail's list repeats one id, so this is
    /// longer than the set of distinct stages it names.
    pub fn ids(&self) -> &[u16] {
        &self.ids
    }

    /// Whether this runtime stage id takes the mirrored second copy.
    pub fn contains_runtime_id(&self, id: u16) -> bool {
        self.ids.contains(&id)
    }

    /// [`Self::contains_runtime_id`] keyed by PROT extraction index.
    pub fn contains_prot_index(&self, prot_index: u32) -> bool {
        runtime_stage_id(prot_index).is_some_and(|id| self.contains_runtime_id(id))
    }

    /// The second-copy transform for the entry at this PROT extraction
    /// index.
    pub fn second_copy_for_prot_index(&self, prot_index: u32) -> SecondCopy {
        if self.contains_prot_index(prot_index) {
            SecondCopy::MirrorX
        } else {
            SecondCopy::HalfTurn
        }
    }
}

/// Retail's stage id for a PROT extraction index, or `None` for the first
/// three entries, which are below the id space.
pub fn runtime_stage_id(prot_index: u32) -> Option<u16> {
    let id = prot_index.checked_sub(RUNTIME_ID_TO_PROT_OFFSET)?;
    u16::try_from(id).ok()
}

/// The PROT extraction index a runtime stage id names.
pub fn prot_index_for_runtime_id(id: u16) -> u32 {
    u32::from(id) + RUNTIME_ID_TO_PROT_OFFSET
}

/// The TMD object indices a backdrop actor draws, in draw order: object 0
/// then everything from index 2 up. Object 1 is dropped.
///
/// This is the `DAT_8007B64B == 0` arm, which is what every catalogued
/// battle takes. For the other arm see [`drawn_object_indices_gated`].
///
/// A one-object TMD would leave the actor with a zero count, so it draws
/// nothing; the retail corpus has no such entry (every `scene_tmd_stream`
/// backdrop carries either 2 or 4 objects).
pub fn drawn_object_indices(object_count: usize) -> Vec<usize> {
    drawn_object_indices_gated(object_count, false)
}

/// [`drawn_object_indices`] with retail's gate exposed.
///
/// `keep_object_1` is `DAT_8007B64B != 0` - bit 5 of byte `+8` of the field
/// scene's encounter-region record. Set, `80051abc` / `80051acc` branch
/// past the whole object edit, so every object stays in the draw list.
pub fn drawn_object_indices_gated(object_count: usize, keep_object_1: bool) -> Vec<usize> {
    if keep_object_1 {
        return (0..object_count).collect();
    }
    if object_count == 0 {
        return Vec::new();
    }
    std::iter::once(0).chain(2..object_count).collect()
}

/// The drawn-object subset of a backdrop TMD, as a standalone [`Tmd`] the
/// ordinary mesh builders can consume.
///
/// [`Tmd`]: legaia_tmd::Tmd
pub fn drawn_objects_tmd(tmd: &legaia_tmd::Tmd) -> legaia_tmd::Tmd {
    legaia_tmd::Tmd {
        header: tmd.header.clone(),
        objects: drawn_object_indices(tmd.objects.len())
            .into_iter()
            .filter_map(|i| tmd.objects.get(i).cloned())
            .collect(),
    }
}

/// One-line description of how retail places this backdrop, for a viewer
/// status line. Pass the second-copy transform when the caller could
/// resolve it from `SCUS_942.54`; `None` still names the completion,
/// without claiming which of the two it is.
pub fn describe_placement(
    shape: Option<&scene_tmd_stream::ShellShape>,
    second: Option<SecondCopy>,
) -> String {
    let open = shape
        .filter(|s| s.is_half_shell())
        .map(|s| format!(", authored half open toward {}", s.open.label()))
        .unwrap_or_default();
    match second {
        Some(c) => format!(
            "battle-stage backdrop - drawn twice, {} second copy{open}",
            c.label()
        ),
        None => format!("battle-stage backdrop - drawn twice, second copy transformed{open}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_one_is_dropped_from_the_draw_list() {
        assert_eq!(drawn_object_indices(4), vec![0, 2, 3]);
        assert_eq!(drawn_object_indices(2), vec![0]);
        assert_eq!(drawn_object_indices(1), vec![0]);
        assert!(drawn_object_indices(0).is_empty());
    }

    #[test]
    fn the_gated_arm_keeps_every_object() {
        assert_eq!(drawn_object_indices_gated(4, true), vec![0, 1, 2, 3]);
        assert_eq!(drawn_object_indices_gated(2, true), vec![0, 1]);
        assert!(drawn_object_indices_gated(0, true).is_empty());
        // Default arm agrees with the convenience wrapper.
        for n in 0..6 {
            assert_eq!(
                drawn_object_indices_gated(n, false),
                drawn_object_indices(n)
            );
        }
    }

    #[test]
    fn half_turn_preserves_winding_and_mirror_reverses_it() {
        assert_eq!(SecondCopy::HalfTurn.scale(), [-1.0, 1.0, -1.0]);
        assert!(!SecondCopy::HalfTurn.flips_winding());
        assert_eq!(SecondCopy::MirrorX.scale(), [-1.0, 1.0, 1.0]);
        assert!(SecondCopy::MirrorX.flips_winding());
        // Determinant sign is what `flips_winding` reports.
        for c in [SecondCopy::HalfTurn, SecondCopy::MirrorX] {
            let s = c.scale();
            assert_eq!(c.flips_winding(), s[0] * s[1] * s[2] < 0.0);
        }
    }

    #[test]
    fn runtime_id_and_prot_index_round_trip() {
        assert_eq!(runtime_stage_id(88), Some(85));
        assert_eq!(runtime_stage_id(7), Some(4));
        assert_eq!(runtime_stage_id(2), None);
        assert_eq!(prot_index_for_runtime_id(85), 88);
    }

    #[test]
    fn table_parse_stops_at_the_terminator() {
        let mut scus = vec![0u8; MIRROR_X_TABLE_SCUS_OFFSET + 16];
        scus[MIRROR_X_TABLE_SCUS_OFFSET..MIRROR_X_TABLE_SCUS_OFFSET + 8]
            .copy_from_slice(&[0x60, 0x00, 0x04, 0x00, 0x00, 0x00, 0xFF, 0xFF]);
        let t = MirrorXTable::from_scus(&scus).expect("table");
        assert_eq!(t.ids(), &[96, 4]);
        assert!(t.contains_runtime_id(4));
        assert!(!t.contains_runtime_id(85));
        // 0007_town01 is on the list; 0088_map01 is not.
        assert_eq!(t.second_copy_for_prot_index(7), SecondCopy::MirrorX);
        assert_eq!(t.second_copy_for_prot_index(88), SecondCopy::HalfTurn);
    }

    #[test]
    fn a_truncated_or_unterminated_table_is_rejected() {
        assert!(MirrorXTable::from_scus(&[0u8; 16]).is_none());
        let scus = vec![0x11u8; MIRROR_X_TABLE_SCUS_OFFSET + 4096];
        assert!(MirrorXTable::from_scus(&scus).is_none());
    }
}
