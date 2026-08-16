//! Isolating the **item alone** out of an equipped section - the opinionated
//! cut, as opposed to [`equip_item`](super::equip_item)'s exact one.
//!
//! [`equip_item`](super::equip_item) answers "which primitives did retail
//! draw from the item's palette?" and refuses to guess past that: a welded
//! weapon exports with the limb it was cut from, armour exports fused with
//! the torso it was sculpted onto. That is the record-keeping export, and it
//! stays. This module answers the other question a downloader asks - "give
//! me just the great axe" - which has **no exact answer on the disc**,
//! because the section re-authors the whole bone object and nothing in it
//! says which primitives are the axe and which are Vahn's wrist. So the
//! answer here is a *policy* plus a **committed per-record override table**
//! ([`RULES_TOML`]), and every result says whether it was curated by hand
//! (`curated`) or came out of the heuristic alone.
//!
//! The policy is one sentence: **the item is everything the section spliced
//! in that is not the character's own flesh or an unchanged piece of them.**
//! Two per-section readings of "unchanged":
//!
//! * **Colour diff** (held items and headgear): a primitive is body when the
//!   texels it samples mostly reappear (within one 5-bit step per channel)
//!   in what the *bare* counterpart object samples - skin, hair, the
//!   wristband, the sleeve. The bare hand and head are exactly "no item", so
//!   what they show is what the item is not. This is what separates the axe
//!   from the wrist strap [`equip_item`](super::equip_item)'s palette cut
//!   claims, and keeps the claw's glove: a gauntlet is not skin.
//! * **Identity** (body and footwear): the bare torso and legs are not
//!   "nothing" - they are the default outfit - so colour alone would call a
//!   dark robe body because the default robe is dark. Here a primitive is
//!   body only when the bare object carries a primitive with the **same
//!   corner positions** *and* the same colours: unchanged trousers under
//!   re-textured boots stay behind, a re-sculpted robe comes along whole.
//!
//! Both are backed by a **skin-hue** rule (peach hue at moderate saturation,
//! most of the primitive's texels), because a Ra-Seru form re-textures the
//! fist it leaves bare with a palette the bare hand never used - measured on
//! Vahn's Meta $7..$9 and Noa's Terra $4..$6, where the fist otherwise stays
//! in the item.
//!
//! What the heuristic cannot know, the table says: a circlet's four
//! primitives that happen to sample hair-coloured texels, a hair strand
//! authored into a robe object. Every override names its record and says
//! why, so the visual pass that produced it can be re-run rather than
//! trusted.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use legaia_tim::Vram;
use legaia_tmd::mesh::VramMesh;
use legaia_tmd::{Tmd, legaia_prims};
use serde::Deserialize;

use super::assembly::AssembledCharacter;
use super::equip_item::ItemPartition;

/// The committed override table.
pub const RULES_TOML: &str = include_str!("../../data/equip-isolation.toml");

/// How the item / body split of a record is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationMode {
    /// Body = texels the bare counterpart object samples (+ skin hue).
    ColourDiff,
    /// Body = same corner positions **and** colours as a bare primitive (+
    /// skin hue).
    Identity,
    /// Every primitive the section spliced in is the item.
    Whole,
    /// Exactly [`equip_item`](super::equip_item)'s palette-column claim.
    Palette,
}

impl IsolationMode {
    /// The default reading per equipment section: identity for body (0) and
    /// footwear (4), colour diff for headgear (1) and the two held-item
    /// sections (2 / 3).
    pub fn default_for_section(section: usize) -> Self {
        match section {
            0 | 4 => IsolationMode::Identity,
            _ => IsolationMode::ColourDiff,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            IsolationMode::ColourDiff => "colour-diff",
            IsolationMode::Identity => "identity",
            IsolationMode::Whole => "whole",
            IsolationMode::Palette => "palette",
        }
    }
}

/// One record's hand-authored corrections. Object addressing is by **bone
/// tag** (`AssembledCharacter::bone_tags`), primitive addressing by
/// `"tag:ordinal"` where the ordinal is the primitive's position in the
/// object's flat group walk (the same numbering
/// `legaia_tmd::mesh::tmd_to_vram_mesh_with_prim_ids` reports).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RecordRule {
    /// `vahn` / `noa` / `gala` (player file slot 0 / 1 / 2).
    pub character: String,
    /// Equipment id (the descriptor record id).
    pub id: u32,
    /// Overrides the section default.
    #[serde(default)]
    pub mode: Option<IsolationMode>,
    /// Palette columns (`cba & 0x3F`) forced into the item.
    #[serde(default)]
    pub keep_columns: Vec<u16>,
    /// Palette columns forced out of the item.
    #[serde(default)]
    pub drop_columns: Vec<u16>,
    /// Bone tags whose whole object is forced into the item.
    #[serde(default)]
    pub keep_objects: Vec<u8>,
    /// Bone tags whose whole object is forced out of the item.
    #[serde(default)]
    pub drop_objects: Vec<u8>,
    /// `"tag:ordinal"` primitives forced into the item.
    #[serde(default)]
    pub keep: Vec<String>,
    /// `"tag:ordinal"` primitives forced out of the item.
    #[serde(default)]
    pub drop: Vec<String>,
    /// Also drop primitives whose texels mostly reappear in the bare **head**
    /// object (hair, face) - for the hair strands some robes are authored
    /// with. Off by default because a character's hair colour is also a
    /// clothing colour (Vahn's blue, Noa's red).
    #[serde(default)]
    pub drop_hair: bool,
    /// Why - what the visual pass saw.
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct RuleTable {
    #[serde(default)]
    pub record: Vec<RecordRule>,
}

impl RuleTable {
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing equipment isolation rules TOML")
    }

    /// The rule for `(character slot, equipment id)`, if any.
    pub fn rule_for(&self, character: usize, id: u32) -> Option<&RecordRule> {
        let who = character_key(character)?;
        self.record
            .iter()
            .find(|r| r.id == id && r.character.eq_ignore_ascii_case(who))
    }
}

/// The committed table, parsed once. Malformed TOML is an authoring error
/// caught by the unit test.
pub fn rules() -> &'static RuleTable {
    static RULES: OnceLock<RuleTable> = OnceLock::new();
    RULES.get_or_init(|| {
        RuleTable::from_toml(RULES_TOML)
            .expect("crates/asset/data/equip-isolation.toml is malformed")
    })
}

/// Player-file slot -> table key.
pub fn character_key(character: usize) -> Option<&'static str> {
    ["vahn", "noa", "gala"].get(character).copied()
}

/// Sorted corner positions of a primitive -> the (dilated) colour set it
/// samples; the identity reading's per-object index of the bare object.
type ShapeColours = BTreeMap<Vec<(i16, i16, i16)>, HashSet<u16>>;

/// One primitive of one object, as the isolation sees it.
#[derive(Debug, Clone)]
pub struct PrimRef {
    /// Object index in the assembled TMD.
    pub object: usize,
    /// Position in the object's flat group walk.
    pub ordinal: u32,
    /// `cba & 0x3F`.
    pub column: u16,
    pub cba: u16,
    pub tsb: u16,
    /// Vertex indices into the object's pool.
    pub corners: Vec<usize>,
    pub uvs: Vec<(u8, u8)>,
}

/// Walk one object's primitives with the ordinal numbering the mesh builder
/// uses. Primitives without corners are counted but not returned.
pub fn object_prim_refs(tmd: &Tmd, blob: &[u8], object: usize) -> Vec<PrimRef> {
    let Some(o) = tmd.objects.get(object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut ordinal = 0u32;
    for g in
        legaia_prims::iter_groups_lenient(blob, o.primitives_byte_offset, o.primitives_byte_size)
    {
        for p in &g.prims {
            let this = ordinal;
            ordinal += 1;
            let corners: Vec<usize> = p.vertex_indices().iter().map(|&i| i as usize).collect();
            if corners.is_empty() {
                continue;
            }
            out.push(PrimRef {
                object,
                ordinal: this,
                column: p.cba & 0x3F,
                cba: p.cba,
                tsb: p.tsb,
                corners,
                uvs: p.uvs.clone(),
            });
        }
    }
    out
}

/// Decode one texel word (BGR555, `0` = transparent) the way the character
/// shader samples it: page from `tsb`, 4/8bpp CLUT indirection via `cba`.
pub fn texel_word(vram: &Vram, cba: u16, tsb: u16, u: usize, v: usize) -> u16 {
    crate::mesh_raster::texel_word(vram, cba, tsb, u, v)
}

/// Every opaque texel word a primitive samples: the texel centres inside its
/// UV polygon (quads as two triangles), falling back to the corner texels
/// for a degenerate (line / point) UV footprint. Colour only - the
/// semi-transparency bit is masked.
pub fn prim_texels(p: &PrimRef, vram: &Vram) -> Vec<u16> {
    let n = p.uvs.len();
    if n < 3 {
        return Vec::new();
    }
    let fans: &[[usize; 3]] = if n == 3 {
        &[[0, 1, 2]]
    } else {
        &[[0, 1, 2], [1, 3, 2]]
    };
    let mut out = Vec::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for f in fans {
        let (a, b, c) = (p.uvs[f[0]], p.uvs[f[1]], p.uvs[f[2]]);
        let centre = |t: (u8, u8)| (f32::from(t.0) + 0.5, f32::from(t.1) + 0.5);
        let ((ax, ay), (bx, by), (cx, cy)) = (centre(a), centre(b), centre(c));
        let det = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
        let (minu, maxu) = (
            a.0.min(b.0).min(c.0) as usize,
            a.0.max(b.0).max(c.0) as usize,
        );
        let (minv, maxv) = (
            a.1.min(b.1).min(c.1) as usize,
            a.1.max(b.1).max(c.1) as usize,
        );
        for v in minv..=maxv {
            for u in minu..=maxu {
                let inside = if det.abs() < 1e-6 {
                    true
                } else {
                    let (fx, fy) = (u as f32 + 0.5, v as f32 + 0.5);
                    let w0 = ((bx - fx) * (cy - fy) - (cx - fx) * (by - fy)) / det;
                    let w1 = ((cx - fx) * (ay - fy) - (ax - fx) * (cy - fy)) / det;
                    let w2 = 1.0 - w0 - w1;
                    w0 >= -0.02 && w1 >= -0.02 && w2 >= -0.02
                };
                if inside && seen.insert((u, v)) {
                    let w = texel_word(vram, p.cba, p.tsb, u, v);
                    if w != 0 {
                        out.push(w & 0x7FFF);
                    }
                }
            }
        }
    }
    if out.is_empty() {
        for &(u, v) in &p.uvs {
            let w = texel_word(vram, p.cba, p.tsb, usize::from(u), usize::from(v));
            if w != 0 {
                out.push(w & 0x7FFF);
            }
        }
    }
    out
}

/// Peach hue at moderate saturation - the party's skin. `r > g > b`, bright
/// enough, hue `8..=45` degrees, saturation `0.2..=0.68`. Deliberately narrow:
/// wood, leather and gold sit outside it.
pub fn skin_like(word: u16) -> bool {
    let (r, g, b) = (
        f32::from(word & 0x1F),
        f32::from((word >> 5) & 0x1F),
        f32::from((word >> 10) & 0x1F),
    );
    if !(r > g && g > b) || r < 17.0 || (r - b) < 4.0 {
        return false;
    }
    let sat = (r - b) / r;
    let hue = 60.0 * (g - b) / (r - b);
    (0.2..=0.68).contains(&sat) && (8.0..=45.0).contains(&hue)
}

/// A warm, moderately saturated colour of any brightness - the shape of a
/// skin texel without the brightness floor [`skin_like`] imposes. Used to
/// pick the character's **own** skin colours out of the bare head object
/// (Gala's is too dark for the generic band).
pub fn warm_like(word: u16) -> bool {
    let (r, g, b) = (
        f32::from(word & 0x1F),
        f32::from((word >> 5) & 0x1F),
        f32::from((word >> 10) & 0x1F),
    );
    if !(r > g && g > b) || r < 9.0 || (r - b) < 3.0 {
        return false;
    }
    let sat = (r - b) / r;
    let hue = 60.0 * (g - b) / (r - b);
    (0.2..=0.7).contains(&sat) && (8.0..=45.0).contains(&hue)
}

/// The character's own skin colours: every warm texel the bare head object
/// samples (the face is the largest skin patch on the model).
pub fn skin_colours(bare: &AssembledCharacter, bare_tmd: &Tmd, bare_vram: &Vram) -> HashSet<u16> {
    let mut set = HashSet::new();
    for (bi, &bs) in bare.section_of.iter().enumerate() {
        if bs == 1 {
            for bp in object_prim_refs(bare_tmd, &bare.tmd, bi) {
                set.extend(
                    prim_texels(&bp, bare_vram)
                        .into_iter()
                        .filter(|w| warm_like(*w)),
                );
            }
        }
    }
    set
}

/// Whether `word` is within one 5-bit step per channel of a colour in `set`.
pub fn near(set: &HashSet<u16>, word: u16) -> bool {
    near_within(set, word, 1)
}

/// Every colour within `radius` 5-bit steps per channel of a colour in
/// `set` - so a membership test replaces the neighbourhood walk
/// [`near_within`] does per texel. Built once per colour set.
pub fn dilate(set: &HashSet<u16>, radius: i32) -> HashSet<u16> {
    let mut out = HashSet::with_capacity(set.len() * 8);
    for &w in set {
        let (r, g, b) = (
            i32::from(w & 0x1F),
            i32::from((w >> 5) & 0x1F),
            i32::from((w >> 10) & 0x1F),
        );
        for dr in -radius..=radius {
            for dg in -radius..=radius {
                for db in -radius..=radius {
                    let (rr, gg, bb) = (r + dr, g + dg, b + db);
                    if (0..=31).contains(&rr) && (0..=31).contains(&gg) && (0..=31).contains(&bb) {
                        out.insert((rr as u16) | ((gg as u16) << 5) | ((bb as u16) << 10));
                    }
                }
            }
        }
    }
    out
}

/// [`near`] with a caller-chosen per-channel radius.
pub fn near_within(set: &HashSet<u16>, word: u16, radius: i32) -> bool {
    let (r, g, b) = (
        i32::from(word & 0x1F),
        i32::from((word >> 5) & 0x1F),
        i32::from((word >> 10) & 0x1F),
    );
    for dr in -radius..=radius {
        for dg in -radius..=radius {
            for db in -radius..=radius {
                let (rr, gg, bb) = (r + dr, g + dg, b + db);
                if !(0..=31).contains(&rr) || !(0..=31).contains(&gg) || !(0..=31).contains(&bb) {
                    continue;
                }
                if set.contains(&((rr as u16) | ((gg as u16) << 5) | ((bb as u16) << 10))) {
                    return true;
                }
            }
        }
    }
    false
}

/// Fraction of a texel list that a predicate accepts (`0` for an empty list).
fn fraction(tex: &[u16], pred: impl Fn(u16) -> bool) -> f32 {
    if tex.is_empty() {
        0.0
    } else {
        tex.iter().filter(|w| pred(**w)).count() as f32 / tex.len() as f32
    }
}

/// Body threshold on the colour-match fraction.
const BODY_MATCH: f32 = 0.5;
/// Body threshold on the skin-hue fraction.
const SKIN_MATCH: f32 = 0.6;
/// Body threshold on the fraction of texels matching the character's **own**
/// skin colours (exact colours, so a lower bar is safe).
const OWN_SKIN_MATCH: f32 = 0.45;
/// Per-channel radius (5-bit steps) for the own-skin match: the face is lit
/// brighter than a neck or an upper arm in shadow.
const OWN_SKIN_RADIUS: i32 = 2;

/// The item-alone selection for one equipped section.
#[derive(Debug, Clone)]
pub struct IsolatedItem {
    /// `(object index, prim ordinal)` of every primitive that is the item.
    pub keep: BTreeSet<(usize, u32)>,
    /// Objects the section spliced in (item candidates), post-sort indices.
    pub objects: BTreeSet<usize>,
    pub kept_primitives: usize,
    pub dropped_primitives: usize,
    pub mode: IsolationMode,
    /// A committed rule touched this record.
    pub curated: bool,
    /// The rule's note, if any.
    pub note: String,
}

impl IsolatedItem {
    pub fn claims(&self, object: usize, ordinal: u32) -> bool {
        self.keep.contains(&(object, ordinal))
    }
}

/// The item alone as geometry: the primitives `item` keeps, in the
/// assembled TMD's **object-local** space, with a per-vertex object id
/// parallel to the mesh (the bone each vertex poses on). Empty when the cut
/// kept nothing. The record-keeping palette cut and the whole-character
/// exports do not go through here; this is the mesh the item-alone export,
/// its preview and its thumbnail all share - and the one
/// [`equip_repair`](super::equip_repair) fills the grip of.
pub fn item_mesh(tmd: &Tmd, blob: &[u8], item: &IsolatedItem) -> (VramMesh, Vec<u32>) {
    let (full, ids, prims) = legaia_tmd::mesh::tmd_to_vram_mesh_with_prim_ids(tmd, blob);
    let mut mesh = VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    let mut out_ids: Vec<u32> = Vec::new();
    let mut remap = vec![u32::MAX; full.positions.len()];
    for v in 0..full.positions.len() {
        if !item.claims(ids[v] as usize, prims[v]) {
            continue;
        }
        remap[v] = mesh.positions.len() as u32;
        mesh.positions.push(full.positions[v]);
        mesh.uvs.push(full.uvs[v]);
        mesh.cba_tsb.push(full.cba_tsb[v]);
        mesh.normals
            .push(full.normals.get(v).copied().unwrap_or([0.0; 3]));
        mesh.colors
            .push(full.colors.get(v).copied().unwrap_or([0x80; 3]));
        out_ids.push(ids[v]);
    }
    for tri in full.indices.chunks_exact(3) {
        let m = [
            remap[tri[0] as usize],
            remap[tri[1] as usize],
            remap[tri[2] as usize],
        ];
        if m.iter().all(|&i| i != u32::MAX) {
            mesh.indices.extend_from_slice(&m);
        }
    }
    (mesh, out_ids)
}

/// Everything the isolation needs about the two assemblies.
pub struct IsolationInputs<'a> {
    pub section: usize,
    /// The same loadout with this section at its default.
    pub bare: &'a AssembledCharacter,
    pub bare_tmd: &'a Tmd,
    pub bare_vram: &'a Vram,
    pub equipped: &'a AssembledCharacter,
    pub equipped_tmd: &'a Tmd,
    pub vram: &'a Vram,
    /// The exact palette partition, for [`IsolationMode::Palette`] and for
    /// naming the item's objects.
    pub partition: &'a ItemPartition,
}

/// Cut the item alone out of the section, under the section default or the
/// record's committed rule.
pub fn isolate_item(inp: &IsolationInputs<'_>, rule: Option<&RecordRule>) -> IsolatedItem {
    let mode = rule
        .and_then(|r| r.mode)
        .unwrap_or_else(|| IsolationMode::default_for_section(inp.section));
    let duplicate = inp.equipped.duplicate_objects(inp.equipped_tmd);

    // Candidate objects: everything the partition names plus everything the
    // section spliced in (a re-sculpted single-palette limb is in the second
    // set but not the first), minus byte-copy duplicates.
    //
    // A `200+` surplus that is *not* a byte copy is the section's alternate
    // pose of the same bone (the open hand behind the fist, reached through
    // the actor's `+0xA4` variant window) - a second copy of the same limb,
    // not a second piece of the item. It stays out of the item-alone cut;
    // `100+` extras are real extra geometry (a club head, a Ra-Seru spine)
    // and stay in.
    let variant = |ei: usize| inp.equipped.bone_tags.get(ei).is_some_and(|&t| t >= 200);
    let mut objects: BTreeSet<usize> = inp
        .partition
        .parts
        .iter()
        .map(|p| p.object)
        .filter(|&ei| !variant(ei))
        .collect();
    for (ei, &s) in inp.equipped.section_of.iter().enumerate() {
        if usize::from(s) == inp.section
            && !duplicate.get(ei).copied().unwrap_or(false)
            && !variant(ei)
        {
            objects.insert(ei);
        }
    }

    // Per-object bare counterpart material: the colour set (colour diff) and
    // the per-primitive corner-position -> colour map (identity).
    let mut bare_colours: BTreeMap<usize, HashSet<u16>> = BTreeMap::new();
    let mut bare_by_shape: BTreeMap<usize, ShapeColours> = BTreeMap::new();
    for &ei in &objects {
        let tag = inp.equipped.bone_tags.get(ei).copied();
        let mut colours = HashSet::new();
        let mut shapes: ShapeColours = BTreeMap::new();
        if let Some(tag) = tag
            && let Some(bi) = inp.bare.bone_tags.iter().position(|&t| t == tag)
            && let Some(bo) = inp.bare_tmd.objects.get(bi)
        {
            for bp in object_prim_refs(inp.bare_tmd, &inp.bare.tmd, bi) {
                let tex = prim_texels(&bp, inp.bare_vram);
                colours.extend(tex.iter().copied());
                let mut key: Vec<(i16, i16, i16)> = bp
                    .corners
                    .iter()
                    .filter_map(|&c| bo.vertices.get(c).map(|v| (v.x, v.y, v.z)))
                    .collect();
                key.sort_unstable();
                shapes.entry(key).or_default().extend(tex);
            }
        }
        bare_colours.insert(ei, dilate(&colours, 1));
        bare_by_shape.insert(
            ei,
            shapes
                .into_iter()
                .map(|(k, v)| (k, dilate(&v, 1)))
                .collect(),
        );
    }

    let own_skin = dilate(
        &skin_colours(inp.bare, inp.bare_tmd, inp.bare_vram),
        OWN_SKIN_RADIUS,
    );

    // Bare head material, for `drop_hair`: every object the bare assembly's
    // section 1 (headgear) default spliced in.
    let head_colours: HashSet<u16> = if rule.is_some_and(|r| r.drop_hair) {
        let mut set = HashSet::new();
        for (bi, &bs) in inp.bare.section_of.iter().enumerate() {
            if bs == 1 {
                for bp in object_prim_refs(inp.bare_tmd, &inp.bare.tmd, bi) {
                    set.extend(prim_texels(&bp, inp.bare_vram));
                }
            }
        }
        dilate(&set, 1)
    } else {
        HashSet::new()
    };

    let parse_ref = |s: &str| -> Option<(u8, u32)> {
        let (a, b) = s.split_once(':')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    };
    let forced: Vec<(u8, u32, bool)> = rule
        .map(|r| {
            r.keep
                .iter()
                .filter_map(|s| parse_ref(s).map(|(t, o)| (t, o, true)))
                .chain(
                    r.drop
                        .iter()
                        .filter_map(|s| parse_ref(s).map(|(t, o)| (t, o, false))),
                )
                .collect()
        })
        .unwrap_or_default();

    let mut keep = BTreeSet::new();
    let mut kept = 0usize;
    let mut dropped = 0usize;
    for &ei in &objects {
        let tag = inp.equipped.bone_tags.get(ei).copied().unwrap_or(u8::MAX);
        let Some(o) = inp.equipped_tmd.objects.get(ei) else {
            continue;
        };
        for p in object_prim_refs(inp.equipped_tmd, &inp.equipped.tmd, ei) {
            // 1. Explicit primitive, 2. whole object, 3. palette column, 4. mode.
            let decision = forced
                .iter()
                .find(|(t, ord, _)| *t == tag && *ord == p.ordinal)
                .map(|(_, _, k)| *k)
                .or_else(|| {
                    let r = rule?;
                    if r.keep_objects.contains(&tag) {
                        Some(true)
                    } else if r.drop_objects.contains(&tag) {
                        Some(false)
                    } else if r.keep_columns.contains(&p.column) {
                        Some(true)
                    } else if r.drop_columns.contains(&p.column) {
                        Some(false)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| match mode {
                    IsolationMode::Whole => true,
                    IsolationMode::Palette => inp.partition.claims(ei, p.cba),
                    IsolationMode::ColourDiff | IsolationMode::Identity => {
                        let tex = prim_texels(&p, inp.vram);
                        let skin = fraction(&tex, skin_like);
                        let own = fraction(&tex, |w| own_skin.contains(&w));
                        let set: Option<&HashSet<u16>> = if mode == IsolationMode::Identity {
                            let mut key: Vec<(i16, i16, i16)> = p
                                .corners
                                .iter()
                                .filter_map(|&c| o.vertices.get(c).map(|v| (v.x, v.y, v.z)))
                                .collect();
                            key.sort_unstable();
                            bare_by_shape.get(&ei).and_then(|m| m.get(&key))
                        } else {
                            bare_colours.get(&ei)
                        };
                        let matched = set
                            .filter(|s| !s.is_empty())
                            .map(|s| fraction(&tex, |w| s.contains(&w)))
                            .unwrap_or(0.0);
                        let hair = if head_colours.is_empty() {
                            0.0
                        } else {
                            fraction(&tex, |w| head_colours.contains(&w))
                        };
                        matched < BODY_MATCH
                            && skin < SKIN_MATCH
                            && own < OWN_SKIN_MATCH
                            && hair < BODY_MATCH
                    }
                });
            if decision {
                keep.insert((ei, p.ordinal));
                kept += 1;
            } else {
                dropped += 1;
            }
        }
    }

    IsolatedItem {
        keep,
        objects,
        kept_primitives: kept,
        dropped_primitives: dropped,
        mode,
        curated: rule.is_some(),
        note: rule.map(|r| r.note.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rules_parse_and_address_real_characters() {
        let t = rules();
        for r in &t.record {
            assert!(
                ["vahn", "noa", "gala"].contains(&r.character.to_ascii_lowercase().as_str()),
                "unknown character {:?}",
                r.character
            );
            assert!(r.id != 0, "id 0 is a section default, not an item");
            for s in r.keep.iter().chain(r.drop.iter()) {
                let (a, b) = s.split_once(':').expect("tag:ordinal");
                a.trim().parse::<u8>().expect("bone tag");
                b.trim().parse::<u32>().expect("ordinal");
            }
            assert!(
                !r.note.trim().is_empty(),
                "rule {}:{:#x} needs a note",
                r.character,
                r.id
            );
        }
    }

    #[test]
    fn section_defaults_split_identity_from_colour_diff() {
        assert_eq!(
            IsolationMode::default_for_section(0),
            IsolationMode::Identity
        );
        assert_eq!(
            IsolationMode::default_for_section(4),
            IsolationMode::Identity
        );
        for s in [1usize, 2, 3] {
            assert_eq!(
                IsolationMode::default_for_section(s),
                IsolationMode::ColourDiff
            );
        }
    }

    #[test]
    fn skin_hue_accepts_peach_and_rejects_wood_grey_and_gold() {
        let w = |r: u16, g: u16, b: u16| r | (g << 5) | (b << 10);
        // Peach: (28, 20, 15) -> hue ~23 deg, sat ~0.46.
        assert!(skin_like(w(28, 20, 15)));
        // Grey.
        assert!(!skin_like(w(20, 20, 20)));
        // Saturated wood brown (24, 12, 4): sat 0.83.
        assert!(!skin_like(w(24, 12, 4)));
        // Gold (30, 26, 4): hue ~51 deg.
        assert!(!skin_like(w(30, 26, 4)));
        // Dark.
        assert!(!skin_like(w(10, 7, 5)));
    }

    #[test]
    fn near_is_one_step_per_channel() {
        let w = |r: u16, g: u16, b: u16| r | (g << 5) | (b << 10);
        let set: HashSet<u16> = [w(10, 10, 10)].into_iter().collect();
        assert!(near(&set, w(11, 9, 10)));
        assert!(!near(&set, w(12, 10, 10)));
    }
}
