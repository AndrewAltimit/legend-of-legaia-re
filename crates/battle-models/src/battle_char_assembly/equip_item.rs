//! Separating the **item** out of an equipped bone object.
//!
//! [`equip_diff`](super::equip_diff) exists because equipment is authored
//! into the bone object rather than attached to it. That is still true - but
//! for the two **weapon-bearing** sections it does not mean the item is
//! inseparable, only that geometry alone will not find it. The separator is
//! the primitive's **CLUT word** (`cba`): a weapon is drawn from its own
//! palette column, and across every section-2/3 record on the disc **no
//! primitive mixes the two**. The item is always an exact primitive subset
//! of the object, selected by material.
//!
//! Two things the naive readings get wrong:
//!
//! * **Connectivity alone misses it.** The item is a separate connected
//!   component for Gala and Noa but not for Vahn, whose weapon is welded to
//!   the fist at the grip aperture. The CLUT partition is exact in every
//!   case, the component partition is not.
//! * **"The bucket that is new vs the bare hand" does not discriminate.** A
//!   section re-textures the *whole* object: Vahn's bare fist draws from
//!   column 0 and his knife-holding fist from columns `0x0D` + `0x0E`, so
//!   both buckets are new.
//!
//! What does discriminate is the **joint**. A TMD object's vertices are
//! authored about its own bone origin, so the flesh half always reaches the
//! origin and the held item never does. [`item_partition`] takes the bucket
//! owning the vertex nearest the object origin as the limb and everything
//! else as the item.
//!
//! Sections 0 / 1 / 4 (body, head, feet) have **no clean boundary**: they
//! carry no surplus object at all (`nobj == attach_count` in all 51
//! records), and their palette buckets split body from trim, not garment
//! from body. There is no "body without armour" to subtract. The same is
//! true of the one weapon record whose object draws from a single palette
//! (Noa's Ra-Seru Terra $1). Those records are **not refused**: they come
//! back as [`ItemClass::Fused`] - every bone object the section replaced,
//! whole, with the host geometry it was authored into. That is a policy
//! choice, completeness over purity: an export of "the armour" that carries
//! the torso it is sculpted onto beats no export at all, provided the file
//! says so - which the class does.
//!
//! What no cut can recover: on a welded record the shaft inside a closed
//! fist **was never modelled**, so the exported item has an open grip (and,
//! on Vahn's Great Axe, a visibly interrupted haft). That is a property of
//! the disc. Callers must say so rather than cap it silently.

use std::collections::{BTreeMap, BTreeSet};

use legaia_tmd::{Tmd, legaia_prims};

use super::assembly::AssembledCharacter;

/// The two player-file sections whose slots carry a **held** item, and are
/// therefore candidates for the palette cut. Every other section still
/// exports - as [`ItemClass::Fused`].
pub const ITEM_SECTIONS: [usize; 2] = [2, 3];

/// How cleanly the item comes away from the bone object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemClass {
    /// The item is already its own TMD object - retail ships the split.
    OwnObject,
    /// The item's primitives form a connected component of their own: zero
    /// vertices shared with the limb. Lossless.
    SeparateComponent,
    /// The item is a palette subset welded to the limb at the grip rim. The
    /// cut is exact at primitive level, but the seam duplicates vertices and
    /// **the grip is left open** - the geometry inside the closed fist does
    /// not exist on the disc.
    WeldedSubset,
    /// No material boundary separates item from limb (armour, and the one
    /// single-palette weapon record). The export is every bone object the
    /// section replaced, **whole** - the item with the host geometry it was
    /// sculpted onto. Complete, but not pure.
    Fused,
}

impl ItemClass {
    /// A short tag for UI / file naming.
    pub fn tag(self) -> &'static str {
        match self {
            ItemClass::OwnObject => "own-object",
            ItemClass::SeparateComponent => "separate",
            ItemClass::WeldedSubset => "welded",
            ItemClass::Fused => "fused",
        }
    }

    /// The honest one-line description a downloader should see, in the file
    /// and in the UI: what they got, and what they did not.
    pub fn describe(self) -> &'static str {
        match self {
            ItemClass::OwnObject => "own object",
            ItemClass::SeparateComponent => "separate",
            ItemClass::WeldedSubset => "welded, grip open",
            ItemClass::Fused => "fused with the host limb",
        }
    }

    /// Whether the exported item is geometrically complete. `false` means
    /// the grip is open and the caller must say so.
    pub fn is_complete(self) -> bool {
        !matches!(self, ItemClass::WeldedSubset)
    }

    /// Whether the export carries **only** item geometry. `false` for
    /// [`ItemClass::Fused`], where the host limb rides along.
    pub fn is_pure(self) -> bool {
        !matches!(self, ItemClass::Fused)
    }
}

/// One object's contribution to the item.
#[derive(Debug, Clone)]
pub struct ItemPart {
    /// Object index in the equipped assembly's TMD.
    pub object: usize,
    /// CLUT columns (`cba & 0x3F`) whose primitives belong to the item. The
    /// whole object when `whole_object`.
    pub columns: BTreeSet<u16>,
    /// The object carries nothing but item geometry.
    pub whole_object: bool,
}

/// Where the item lives inside an equipped assembly.
#[derive(Debug, Clone)]
pub struct ItemPartition {
    pub class: ItemClass,
    /// Objects (and their palette columns) the item occupies.
    pub parts: Vec<ItemPart>,
    /// Primitives the item claims.
    pub item_primitives: usize,
    /// Distinct vertex positions the item claims.
    pub item_vertices: usize,
    /// Primitives left on the limb.
    pub limb_primitives: usize,
    /// Distinct vertex positions used by both halves - the welded grip rim.
    /// Zero for [`ItemClass::OwnObject`] / [`ItemClass::SeparateComponent`].
    pub seam_vertices: usize,
}

impl ItemPartition {
    /// Whether primitive `(object, cba)` belongs to the item.
    pub fn claims(&self, object: usize, cba: u16) -> bool {
        self.parts
            .iter()
            .any(|p| p.object == object && (p.whole_object || p.columns.contains(&(cba & 0x3F))))
    }
}

/// One primitive's palette column and corner vertex indices.
struct PrimRef {
    column: u16,
    corners: Vec<usize>,
}

fn object_prims(tmd: &Tmd, blob: &[u8], obj: usize) -> Vec<PrimRef> {
    let Some(o) = tmd.objects.get(obj) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in
        legaia_prims::iter_groups_lenient(blob, o.primitives_byte_offset, o.primitives_byte_size)
    {
        for p in &g.prims {
            let corners: Vec<usize> = p.vertex_indices().iter().map(|&i| i as usize).collect();
            if corners.is_empty() {
                continue;
            }
            out.push(PrimRef {
                column: p.cba & 0x3F,
                corners,
            });
        }
    }
    out
}

/// Weld an object's vertices by position, returning per-vertex weld ids.
fn weld(tmd: &Tmd, obj: usize) -> Vec<usize> {
    let Some(o) = tmd.objects.get(obj) else {
        return Vec::new();
    };
    let mut seen: BTreeMap<(i16, i16, i16), usize> = BTreeMap::new();
    o.vertices
        .iter()
        .map(|v| {
            let n = seen.len();
            *seen.entry((v.x, v.y, v.z)).or_insert(n)
        })
        .collect()
}

/// Split the objects an equipped section changed into limb and item.
///
/// For a section in [`ITEM_SECTIONS`] this is the palette cut. For every
/// other section - and for a held-item section whose changed objects carry a
/// single palette column - it falls through to [`ItemClass::Fused`]: every
/// changed object, whole. `None` only when the section changed **no**
/// geometry at all, in which case there is nothing to export and the caller
/// should say that rather than ship an empty file.
pub fn item_partition(
    section: usize,
    bare: &AssembledCharacter,
    bare_tmd: &Tmd,
    equipped: &AssembledCharacter,
    equipped_tmd: &Tmd,
) -> Option<ItemPartition> {
    if ITEM_SECTIONS.contains(&section)
        && let Some(p) = held_item_partition(bare, bare_tmd, equipped, equipped_tmd)
    {
        return Some(p);
    }
    fused_partition(section, equipped, equipped_tmd)
}

/// [`ItemClass::Fused`]: the section's **whole contribution** to the
/// assembly - every object it spliced in (`section_of`), minus byte-copy
/// duplicates. Keyed on the section rather than on a diff against the bare
/// model, because a section can be geometrically identical to the default
/// and differ only in its texture pool (Noa's Green Robe is her starting
/// robe, and its body section is byte-for-byte the default's); the objects
/// are still what that equipment *is*, and the export must carry them.
fn fused_partition(
    section: usize,
    equipped: &AssembledCharacter,
    equipped_tmd: &Tmd,
) -> Option<ItemPartition> {
    let duplicate = equipped.duplicate_objects(equipped_tmd);
    let mut parts = Vec::new();
    let mut item_primitives = 0usize;
    let mut positions: BTreeSet<(i16, i16, i16)> = BTreeSet::new();
    for (ei, &dup) in duplicate.iter().enumerate() {
        if dup || equipped.section_of.get(ei).copied() != Some(section as u8) {
            continue;
        }
        let prims = object_prims(equipped_tmd, &equipped.tmd, ei);
        if prims.is_empty() {
            continue;
        }
        item_primitives += prims.len();
        collect_positions(equipped_tmd, ei, &prims, None, &mut positions);
        parts.push(ItemPart {
            object: ei,
            columns: BTreeSet::new(),
            whole_object: true,
        });
    }
    if parts.is_empty() {
        return None;
    }
    Some(ItemPartition {
        class: ItemClass::Fused,
        parts,
        item_primitives,
        item_vertices: positions.len(),
        limb_primitives: 0,
        seam_vertices: 0,
    })
}

/// The palette cut for a held-item section. `None` when no changed object
/// carries a second palette column - the caller falls back to
/// [`fused_partition`].
fn held_item_partition(
    bare: &AssembledCharacter,
    bare_tmd: &Tmd,
    equipped: &AssembledCharacter,
    equipped_tmd: &Tmd,
) -> Option<ItemPartition> {
    let mut parts: Vec<ItemPart> = Vec::new();
    let mut item_primitives = 0usize;
    let mut limb_primitives = 0usize;
    let mut item_positions: BTreeSet<(i16, i16, i16)> = BTreeSet::new();
    let mut seam = 0usize;
    let mut welded = false;
    let duplicate = equipped.duplicate_objects(equipped_tmd);

    // Class A first: an extra object with no counterpart in the unequipped
    // assembly is the item outright - retail already shipped the split, and
    // there is nothing to cut.
    let standalone: Vec<usize> = equipped
        .bone_tags
        .iter()
        .enumerate()
        .filter(|(ei, tag)| {
            **tag >= 100
                && !duplicate[*ei]
                && !bare.bone_tags.contains(tag)
                && !object_prims(equipped_tmd, &equipped.tmd, *ei).is_empty()
        })
        .map(|(ei, _)| ei)
        .collect();
    if !standalone.is_empty() {
        for ei in standalone {
            // A section may ship the same extra twice (Gala's Ra-Seru Ozma $7
            // carries two identical `0xFE` objects); export it once.
            if parts.iter().any(|p| {
                p.whole_object
                    && same_object(
                        equipped_tmd,
                        &equipped.tmd,
                        p.object,
                        equipped_tmd,
                        &equipped.tmd,
                        ei,
                    )
            }) {
                continue;
            }
            let prims = object_prims(equipped_tmd, &equipped.tmd, ei);
            item_primitives += prims.len();
            collect_positions(equipped_tmd, ei, &prims, None, &mut item_positions);
            parts.push(ItemPart {
                object: ei,
                columns: BTreeSet::new(),
                whole_object: true,
            });
        }
        return Some(ItemPartition {
            class: ItemClass::OwnObject,
            parts,
            item_primitives,
            item_vertices: item_positions.len(),
            limb_primitives: 0,
            seam_vertices: 0,
        });
    }

    for (ei, &tag) in equipped.bone_tags.iter().enumerate() {
        if duplicate[ei] {
            continue;
        }
        let prims = object_prims(equipped_tmd, &equipped.tmd, ei);
        if prims.is_empty() {
            continue;
        }
        let bare_idx = bare.bone_tags.iter().position(|&t| t == tag);
        // Unchanged objects belong to neither half.
        if bare_idx
            .is_some_and(|bi| same_object(bare_tmd, &bare.tmd, bi, equipped_tmd, &equipped.tmd, ei))
        {
            continue;
        }

        let columns: BTreeSet<u16> = prims.iter().map(|p| p.column).collect();
        if columns.len() < 2 {
            // One palette across the whole object: the section re-sculpted
            // the limb, and nothing here reads as a held item.
            limb_primitives += prims.len();
            continue;
        }

        // The limb half is the palette column owning the vertex nearest the
        // object's own bone origin. Everything else is the item.
        let Some(limb_column) = limb_column_of(equipped_tmd, ei, &prims) else {
            limb_primitives += prims.len();
            continue;
        };
        let item_columns: BTreeSet<u16> = columns
            .iter()
            .copied()
            .filter(|c| *c != limb_column)
            .collect();
        item_primitives += prims.iter().filter(|p| p.column != limb_column).count();
        limb_primitives += prims.iter().filter(|p| p.column == limb_column).count();
        collect_positions(
            equipped_tmd,
            ei,
            &prims,
            Some(&item_columns),
            &mut item_positions,
        );

        // Seam: welded vertices used by both halves of this object.
        let w = weld(equipped_tmd, ei);
        let mut item_w: BTreeSet<usize> = BTreeSet::new();
        let mut limb_w: BTreeSet<usize> = BTreeSet::new();
        for p in &prims {
            let target = if p.column == limb_column {
                &mut limb_w
            } else {
                &mut item_w
            };
            for &c in &p.corners {
                if let Some(&id) = w.get(c) {
                    target.insert(id);
                }
            }
        }
        let shared = item_w.intersection(&limb_w).count();
        seam += shared;
        if shared > 0 {
            welded = true;
        }
        parts.push(ItemPart {
            object: ei,
            columns: item_columns,
            whole_object: false,
        });
    }

    if parts.is_empty() || item_primitives == 0 {
        return None;
    }
    let class = if welded {
        ItemClass::WeldedSubset
    } else {
        ItemClass::SeparateComponent
    };
    Some(ItemPartition {
        class,
        parts,
        item_primitives,
        item_vertices: item_positions.len(),
        limb_primitives,
        seam_vertices: seam,
    })
}

/// The palette column that owns the vertex closest to the object's own bone
/// origin - the flesh half, since a TMD object is authored about its joint
/// and a held item never reaches it.
fn limb_column_of(tmd: &Tmd, obj: usize, prims: &[PrimRef]) -> Option<u16> {
    let verts = &tmd.objects.get(obj)?.vertices;
    let mut best: Option<(f32, u16)> = None;
    for p in prims {
        for &c in &p.corners {
            let Some(v) = verts.get(c) else { continue };
            let d = (f32::from(v.x)).hypot(f32::from(v.y)).hypot(f32::from(v.z));
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p.column));
            }
        }
    }
    best.map(|(_, c)| c)
}

fn collect_positions(
    tmd: &Tmd,
    obj: usize,
    prims: &[PrimRef],
    columns: Option<&BTreeSet<u16>>,
    out: &mut BTreeSet<(i16, i16, i16)>,
) {
    let Some(o) = tmd.objects.get(obj) else {
        return;
    };
    for p in prims {
        if let Some(cols) = columns
            && !cols.contains(&p.column)
        {
            continue;
        }
        for &c in &p.corners {
            if let Some(v) = o.vertices.get(c) {
                out.insert((v.x, v.y, v.z));
            }
        }
    }
}

/// Whether two objects carry the same vertex pool **and the same primitive
/// block**. The block matters: a section can re-texture a limb without
/// touching a single vertex (Vahn's Ironman Boots do exactly that to his
/// feet), and that is still a replaced object the export must carry.
fn same_object(a_tmd: &Tmd, a_blob: &[u8], a: usize, b_tmd: &Tmd, b_blob: &[u8], b: usize) -> bool {
    let (Some(ao), Some(bo)) = (a_tmd.objects.get(a), b_tmd.objects.get(b)) else {
        return false;
    };
    let same_verts = ao.claimed_n_primitive == bo.claimed_n_primitive
        && ao.vertices.len() == bo.vertices.len()
        && ao
            .vertices
            .iter()
            .zip(bo.vertices.iter())
            .all(|(x, y)| x.x == y.x && x.y == y.y && x.z == y.z);
    if !same_verts || ao.primitives_byte_size != bo.primitives_byte_size {
        return false;
    }
    let pa =
        a_blob.get(ao.primitives_byte_offset..ao.primitives_byte_offset + ao.primitives_byte_size);
    let pb =
        b_blob.get(bo.primitives_byte_offset..bo.primitives_byte_offset + bo.primitives_byte_size);
    matches!((pa, pb), (Some(x), Some(y)) if x == y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_held_item_sections_are_cut_candidates() {
        for s in [0usize, 1, 4] {
            assert!(!ITEM_SECTIONS.contains(&s), "section {s}");
        }
        for s in [2usize, 3] {
            assert!(ITEM_SECTIONS.contains(&s), "section {s}");
        }
    }

    #[test]
    fn the_class_flags_say_what_the_downloader_got() {
        assert!(!ItemClass::WeldedSubset.is_complete());
        assert!(ItemClass::OwnObject.is_complete());
        assert!(ItemClass::SeparateComponent.is_complete());
        // Fused is complete (nothing missing) but not pure (limb rides along).
        assert!(ItemClass::Fused.is_complete());
        assert!(!ItemClass::Fused.is_pure());
        assert!(ItemClass::WeldedSubset.is_pure());
        assert!(ItemClass::Fused.describe().contains("fused"));
        assert!(ItemClass::WeldedSubset.describe().contains("grip open"));
    }
}
