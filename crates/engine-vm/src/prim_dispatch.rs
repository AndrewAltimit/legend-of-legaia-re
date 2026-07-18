//! Per-prim renderer dispatch (engine port of `FUN_80043390`).
//!
//! PORT: FUN_80043390
//!
//! Every per-actor case-5 TMD prim flows through a `(prim_mode,
//! alpha_state)` lookup that selects one of 20 per-mode renderer
//! leaves. On retail PSX, the table base is paged in two flavours:
//!
//! | Flag (`_DAT_1F800394 & 1`) | Table base   | Variant            |
//! |---|---|---|
//! | clear | `0x8007657C` | SCUS-resident                          |
//! | set   | `0x801F8968` | World-map-overlay, distance-fog post   |
//!
//! The SCUS table `0x8007657C` is 4 alpha banks × 20 kind slots × 4
//! bytes. Its structure - dumped straight from the SCUS PSX-EXE and
//! cross-checked against a static-recompilation of the executable
//! (see the render-dispatch cross-reference notes) - is:
//!
//! - **Slots 0..7 are NULL in every bank** - stream terminator / unused.
//! - **Slots 8..11 are bank-invariant** (the *same* handler in all four
//!   alpha banks) and are the **only handlers that run a light source**:
//!   they issue GTE `NCCS` / `NCCT` (Normal→Colour→Colour), the ROM's
//!   only NCC light code, statically associated with the world-map
//!   slot-4 landmark meshes. Handlers `0x8004409C` (k8 NCCS), `0x8004423C`
//!   (k9 NCCS), `0x80044434` (k10 NCCT), `0x800445B0` (k11 NCCT+NCCS).
//!   NB these handlers are **not observed executing at runtime** - GTE-op
//!   sampling of battle / summon / the `map01` world map issues zero
//!   `NCC*`; the world map renders unlit. So kinds 8..11 are the
//!   light-*capable* handlers, not a confirmed live light path (see
//!   [`docs/subsystems/renderer.md`] "Retail runs no light source").
//! - **Slots 12..19 are bank-dependent**: bank 0 issues no colour op
//!   (opaque, baked colour); banks 1/2/3 add `DPCS`/`DPCT` depth cue.
//!   These are the unlit field/battle handlers.
//!
//! **Topology is parity-based, not range-based** (definitive, from each
//! handler's `AVSZ3` vs `AVSZ4`): even kinds (8,10,12,14,16,18) are
//! triangles, odd kinds (9,11,13,15,17,19) are quads. Each of the
//! low/high ranges is therefore a tri/quad *mix*, not a uniform vertex
//! count. The textured-vs-gourand family for slots 12..19 is inferred
//! from packet length + the two handlers whose GP0 code byte is a clean
//! immediate; treat those `PolyKind`s as topology-anchored inferences.
//! Source-of-truth: [`docs/subsystems/renderer.md`] and
//! [`docs/subsystems/world-map.md`](../../../docs/subsystems/world-map.md#per-slot-delta-vs-scus-sibling).
//!
//! The engine port mirrors this shape. Rather than emitting raw GPU
//! packets, it returns a `RenderMode` enum that captures everything a
//! wgpu-side TMD-render path needs to know:
//!
//! - which base prim type to issue (`PolyKind`),
//! - which light source (if any) the handler applies (`NccMode`),
//! - whether to apply the world-map fog post-process (`Variant`),
//! - the alpha-state row index (0..3) for blend-mode selection on the
//!   SCUS path. The overlay path ignores alpha, mirroring
//!   `FUN_80043390`.
//!
//! The lookup is pure - no GTE state, no GPU side effects - so it
//! can be unit-tested in isolation. The actual prim emit lives in
//! `engine-render`; this module is the policy layer that tells the
//! renderer which path to take.
//!
//! ## PORT status of the lit path
//!
//! The NCC light handlers (slots 8..11) are **modelled here but not yet
//! consumed by a live render path**. The engine's wgpu renderer does not
//! currently draw the world-map slot-4 landmark meshes (the 3D world map
//! is served by the static-site viewer, and the engine `world_map`
//! module is the controller/simulation, not a mesh renderer). The GTE
//! `nccs`/`ncct` kernels *are* implemented in
//! `engine-render::gte::lighting` and exercised by the parity oracle
//! `gte_trace`. Wiring them is **low priority**: retail itself is not
//! observed dispatching to the NCC handlers at runtime (GTE-op sampling of
//! battle / summon / the `map01` world map shows zero `NCC*` - the world map
//! renders unlit), so leaving slots 8..11 unlit matches observed retail
//! output. The [`RenderMode::lit`] metadata is kept for fidelity in case a
//! light-using scene is ever found; until then this module is a faithful
//! *data model*, not a wired lighting path.
//!
//! ## Selection
//!
//! ```text
//! prim_mode  ∈ 0..20 ; mirrors the per-mode descriptor table at
//!                       DAT_8007326C (the Legaia TMD renderer's
//!                       cmd-byte table).
//! alpha_off  ∈ 0,0x50,0xA0,0xF0 ; PSX semi-transparency state.
//! ```
//!
//! ## Engine integration
//!
//! `engine-core::SceneResources::build_targeted` consults
//! [`SceneRenderPolicy::from_scene_kind`] at scene-load time so the
//! resource-build path knows which variant to ask the renderer for. The
//! world-map scene is the only retail case that toggles the overlay
//! variant; everything else uses `Variant::Scus`.

/// One of the eight POLY family kinds the PSX GTE supports. The
/// slot→kind mapping matches the `FUN_80043390` table layout; note that
/// PSX POLY families encode only topology + texture + gouraud, **not**
/// lighting - the NCC light source (slots 8..11) is carried separately
/// by [`NccMode`], because the same GT3/GT4 packet is emitted whether or
/// not the handler ran an NCC op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolyKind {
    /// `POLY_F3` - flat-shaded triangle.
    F3,
    /// `POLY_FT3` - flat-shaded textured triangle.
    FT3,
    /// `POLY_G3` - Gouraud-shaded triangle.
    G3,
    /// `POLY_GT3` - Gouraud-shaded textured triangle.
    GT3,
    /// `POLY_F4` - flat-shaded quad.
    F4,
    /// `POLY_FT4` - flat-shaded textured quad.
    FT4,
    /// `POLY_G4` - Gouraud-shaded quad.
    G4,
    /// `POLY_GT4` - Gouraud-shaded textured quad.
    GT4,
}

impl PolyKind {
    /// How many vertices this prim consumes from the actor vertex pool.
    /// Triangles = 3, quads = 4. The slot table's topology (which kind
    /// each slot maps to) is parity-based - see the module docs.
    pub fn vertex_count(self) -> u8 {
        match self {
            PolyKind::F3 | PolyKind::FT3 | PolyKind::G3 | PolyKind::GT3 => 3,
            PolyKind::F4 | PolyKind::FT4 | PolyKind::G4 | PolyKind::GT4 => 4,
        }
    }
    /// Whether this prim samples a texture (UVs + CLUT + TPage).
    pub fn is_textured(self) -> bool {
        matches!(
            self,
            PolyKind::FT3 | PolyKind::GT3 | PolyKind::FT4 | PolyKind::GT4
        )
    }
    /// Whether this prim carries per-vertex color (Gouraud).
    pub fn is_gouraud(self) -> bool {
        matches!(
            self,
            PolyKind::G3 | PolyKind::GT3 | PolyKind::G4 | PolyKind::GT4
        )
    }
}

/// The GTE light source a dispatch handler applies. Only slots 8..11
/// carry one; every other slot is [`NccMode::None`]. `NCC` = Normal →
/// Colour → Colour (light matrix `L` cr8-12 × light-colour matrix `LC`
/// cr16-20 + back-colour `RBK/GBK/BBK`), a real light source **without**
/// the depth-cue that `NCD*` would add. This is the ROM's only NCC light
/// code (statically tied to the world-map slot-4 landmark meshes) - but it
/// is **not observed running at runtime**: GTE-op sampling of battle,
/// summon, and the `map01` world map issues zero `NCC*`, so this is the
/// light-*capable* path, not a confirmed live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NccMode {
    /// No light source - the handler uses the baked colour word.
    None,
    /// `NCCS` - single-normal NCC (one light pass over the shared normal).
    Nccs,
    /// `NCCT` - triple-normal NCC (a light pass per vertex).
    Ncct,
    /// `NCCT` then `NCCS` - the kind-11 handler runs both.
    NcctNccs,
}

impl NccMode {
    /// Whether this handler applies any hardware light source.
    pub fn is_lit(self) -> bool {
        !matches!(self, NccMode::None)
    }
}

/// Which dispatch-table variant is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// SCUS-resident renderer (table `0x8007657C`). Used for every
    /// retail field / battle / cutscene render. Slots 8..11 still apply
    /// their NCC light source here; the world-map LUT-tint fog
    /// post-process is *not* applied (that is the overlay variant's job),
    /// but a semi-transparent SCUS prim (alpha bank 1/2/3) does run the
    /// GTE `DPCS`/`DPCT` depth cue - see [`RenderMode::applies_depth_cue`].
    Scus,
    /// World-map-overlay renderer (table `0x801F8968`); applies per-vertex
    /// distance-cue fog via GTE `dpcs` (3-vertex prims) or `dpct + dpcs`
    /// (4-vertex prims) plus a per-Z color-LUT tint at GP-offset
    /// `-0x2bc`. Active only when the world-map overlay is paged in
    /// AND `_DAT_1F800394 & 1` is set.
    Overlay,
}

/// The four PSX semi-transparency states `FUN_80043390` cycles
/// through on the SCUS path. The overlay path ignores alpha and uses
/// only `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlphaState {
    /// `_DAT_1F800028 == 0x00`. Standard opaque blend (alpha bank 0).
    Off,
    /// `_DAT_1F800028 == 0x50`. 50% / 50% semi-transparency (bank 1).
    Half,
    /// `_DAT_1F800028 == 0xA0`. Additive (B + F) (bank 2).
    Additive,
    /// `_DAT_1F800028 == 0xF0`. Subtractive (B - F) (bank 3).
    Subtractive,
}

impl AlphaState {
    /// `_DAT_1F800028` value associated with each row.
    pub fn raw_byte(self) -> u8 {
        match self {
            AlphaState::Off => 0x00,
            AlphaState::Half => 0x50,
            AlphaState::Additive => 0xA0,
            AlphaState::Subtractive => 0xF0,
        }
    }
    /// Decode from a raw `_DAT_1F800028` byte; unknown values fall
    /// back to `Off` (matching `FUN_80043390`'s default branch).
    pub fn from_raw(b: u8) -> Self {
        match b {
            0x50 => AlphaState::Half,
            0xA0 => AlphaState::Additive,
            0xF0 => AlphaState::Subtractive,
            _ => AlphaState::Off,
        }
    }
    /// Numeric row index (`0..4`) matching the SCUS-path alpha offset,
    /// i.e. the alpha bank the handler is looked up in.
    pub fn row_index(self) -> usize {
        match self {
            AlphaState::Off => 0,
            AlphaState::Half => 1,
            AlphaState::Additive => 2,
            AlphaState::Subtractive => 3,
        }
    }
}

/// Result of one prim-dispatch lookup: which renderer is invoked plus
/// enough context for `engine-render` to issue the right GPU op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderMode {
    pub kind: PolyKind,
    /// The light source this handler applies (slots 8..11 only).
    pub lit: NccMode,
    pub variant: Variant,
    pub alpha: AlphaState,
}

impl RenderMode {
    /// Returns `true` when this mode applies the world-map overlay's
    /// fog post-process (the per-Z colour-LUT tint plus `dpcs`/`dpct`).
    /// This is the overlay-variant-only distance haze; it is distinct
    /// from the plain depth cue a semi-transparent SCUS prim also runs
    /// ([`Self::applies_depth_cue`]).
    pub fn applies_fog(self) -> bool {
        self.variant == Variant::Overlay
    }

    /// Returns `true` when the handler runs a GTE `DPCS`/`DPCT` depth
    /// cue. The overlay variant always does; on the SCUS path only the
    /// semi-transparent alpha banks (1/2/3 = Half/Additive/Subtractive)
    /// do - bank 0 (opaque) emits no colour op. This mirrors the SCUS
    /// table `0x8007657C`, whose banks 1/2/3 point at the DPCS/DPCT
    /// handler bodies for slots 12..19. (The engine's mesh shader may
    /// already apply a generic per-vertex depth cue on the textured /
    /// colour path; this flag records the retail table fact so a future
    /// consumer can reconcile the two rather than double-applying.)
    pub fn applies_depth_cue(self) -> bool {
        match self.variant {
            Variant::Overlay => true,
            Variant::Scus => !matches!(self.alpha, AlphaState::Off),
        }
    }

    /// Whether this handler applies an NCC hardware light source.
    pub fn is_lit(self) -> bool {
        self.lit.is_lit()
    }
}

/// `FUN_80043390`'s slot layout. Slots 0..7 are unused; 8..11 are the
/// bank-invariant NCC-lit handlers; 12..19 are the bank-varying
/// (opaque / depth-cue) handlers.
pub const LOW_MODE_START: usize = 8;
pub const LOW_MODE_END: usize = 12; // exclusive
pub const HIGH_MODE_START: usize = 12;
pub const HIGH_MODE_END: usize = 20; // exclusive

/// Bank-0 handler entry addresses for slots 8..19, pinned from the SCUS
/// jump table `0x8007657C`. Slots 8..11 are bank-invariant; for 12..19
/// this records the bank-0 (opaque) body - banks 1/2/3 point at distinct
/// DPCS/DPCT bodies. Provenance/attribution only; the engine does not
/// dispatch on these.
pub const SLOT_HANDLER_VA: [Option<u32>; 20] = {
    let mut t: [Option<u32>; 20] = [None; 20];
    t[8] = Some(0x8004_409C);
    t[9] = Some(0x8004_423C);
    t[10] = Some(0x8004_4434);
    t[11] = Some(0x8004_45B0);
    t[12] = Some(0x8004_3658);
    t[13] = Some(0x8004_3768);
    t[14] = Some(0x8004_3B58);
    t[15] = Some(0x8004_3C6C);
    t[16] = Some(0x8004_38B8);
    t[17] = Some(0x8004_39E4);
    t[18] = Some(0x8004_3DD4);
    t[19] = Some(0x8004_3F10);
    t
};

/// Slot 8..19 → `PolyKind`. Topology (tri/quad) is definitive from each
/// handler's `AVSZ3`/`AVSZ4` and is parity-based (even = tri, odd =
/// quad). The texture/gouraud family for 12..19 is inferred from packet
/// length + GP0 code (see module docs); slots 8..11 are lit textured
/// prims (per-vertex colour comes from the NCC op, so modelled as GT3 /
/// GT4 with [`NccMode`] carrying the light source).
const SLOT_TO_KIND: [Option<PolyKind>; 20] = {
    let mut t: [Option<PolyKind>; 20] = [None; 20];
    // 8..11 - the NCC-lit handlers (bank-invariant). Lit textured
    // tris/quads; light source in SLOT_TO_NCC below.
    t[8] = Some(PolyKind::GT3); // 0x8004409C, tri, NCCS
    t[9] = Some(PolyKind::GT4); // 0x8004423C, quad, NCCS
    t[10] = Some(PolyKind::GT3); // 0x80044434, tri, NCCT
    t[11] = Some(PolyKind::GT4); // 0x800445B0, quad, NCCT+NCCS
    // 12..19 - unlit, bank-varying. Topology parity-based; family
    // inferred (topology anchored).
    t[12] = Some(PolyKind::F3); // tri,  flat
    t[13] = Some(PolyKind::F4); // quad, flat
    t[14] = Some(PolyKind::GT3); // tri,  tex/gouraud
    t[15] = Some(PolyKind::GT4); // quad, tex/gouraud
    t[16] = Some(PolyKind::FT3); // tri,  tex
    t[17] = Some(PolyKind::FT4); // quad, tex
    t[18] = Some(PolyKind::FT3); // tri,  tex (FT3-ext; bank2 emits code 0x24)
    t[19] = Some(PolyKind::GT4); // quad, tex (GT4-ext; bank3 = composite G3+LINE_F2)
    t
};

/// Slot 8..19 → light source. Only the four bank-invariant handlers
/// 8..11 carry one; every other slot is [`NccMode::None`].
const SLOT_TO_NCC: [NccMode; 20] = {
    let mut t: [NccMode; 20] = [NccMode::None; 20];
    t[8] = NccMode::Nccs;
    t[9] = NccMode::Nccs;
    t[10] = NccMode::Ncct;
    t[11] = NccMode::NcctNccs;
    t
};

/// Map an absolute prim-mode slot index to its `PolyKind`. Returns
/// `None` for the unused 0..7 range or any out-of-bounds value.
pub fn slot_to_kind(slot: usize) -> Option<PolyKind> {
    SLOT_TO_KIND.get(slot).copied().flatten()
}

/// The light source a slot's handler applies. `None` for unused /
/// out-of-bounds slots and every non-lit handler; `Nccs`/`Ncct`/
/// `NcctNccs` for slots 8..11.
pub fn slot_lit(slot: usize) -> NccMode {
    SLOT_TO_NCC.get(slot).copied().unwrap_or(NccMode::None)
}

/// Resolve a `(slot, alpha, variant)` triple to a `RenderMode`.
///
/// Returns `None` for slots without a populated renderer (0..7 on
/// retail). On `Variant::Overlay` the alpha is forced to `Off`
/// because the overlay path skips the alpha-offset add in
/// `FUN_80043390`.
pub fn resolve(slot: usize, alpha: AlphaState, variant: Variant) -> Option<RenderMode> {
    let kind = slot_to_kind(slot)?;
    let alpha = if matches!(variant, Variant::Overlay) {
        AlphaState::Off
    } else {
        alpha
    };
    Some(RenderMode {
        kind,
        lit: slot_lit(slot),
        variant,
        alpha,
    })
}

/// Policy attached to a built scene that tells the renderer which
/// dispatch variant to use across the scene's per-prim emit. Mirrors
/// the runtime gate `_DAT_1F800394 & 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneRenderPolicy {
    pub variant: Variant,
}

impl Default for SceneRenderPolicy {
    fn default() -> Self {
        Self {
            variant: Variant::Scus,
        }
    }
}

impl SceneRenderPolicy {
    /// World-map scenes (top-view + walk variants) toggle the overlay
    /// variant so the bulk continent terrain receives distance-cue
    /// fog. Every other retail scene class stays on the SCUS path.
    pub fn world_map() -> Self {
        Self {
            variant: Variant::Overlay,
        }
    }

    /// Decide the variant from a `SceneLoadKind` discriminator. The
    /// engine-core `SceneResources::build_targeted` path passes the
    /// load kind through here so the resource-build step can stash the
    /// policy alongside the built scene.
    pub fn from_scene_kind(kind: SceneKindHint) -> Self {
        match kind {
            SceneKindHint::WorldMap => Self::world_map(),
            _ => Self::default(),
        }
    }
}

/// Engine-side hint for which dispatch variant a scene needs. Kept as
/// a small enum here (rather than depending on `engine-core`'s
/// `SceneLoadKind`) so this module remains free of upward deps. The
/// resource-builder maps `SceneLoadKind` -> `SceneKindHint` at the
/// call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneKindHint {
    /// Field map (towns, dungeons, etc). SCUS variant.
    Field,
    /// Battle scene. SCUS variant.
    Battle,
    /// World map (overworld + top-view debug). Overlay variant - fog
    /// post-process applied.
    WorldMap,
    /// Cutscene preamble / STR FMV. SCUS variant.
    Cutscene,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_to_kind_covers_all_populated_slots_with_correct_topology() {
        // Slots 8..11 are the lit handlers; topology is parity-based
        // (8,10 tris; 9,11 quads), modelled as GT3/GT4.
        assert_eq!(slot_to_kind(8), Some(PolyKind::GT3));
        assert_eq!(slot_to_kind(9), Some(PolyKind::GT4));
        assert_eq!(slot_to_kind(10), Some(PolyKind::GT3));
        assert_eq!(slot_to_kind(11), Some(PolyKind::GT4));
        // Slots 12..19 alternate tri/quad by parity.
        assert_eq!(slot_to_kind(12), Some(PolyKind::F3));
        assert_eq!(slot_to_kind(13), Some(PolyKind::F4));
        assert_eq!(slot_to_kind(14), Some(PolyKind::GT3));
        assert_eq!(slot_to_kind(15), Some(PolyKind::GT4));
        assert_eq!(slot_to_kind(16), Some(PolyKind::FT3));
        assert_eq!(slot_to_kind(17), Some(PolyKind::FT4));
        assert_eq!(slot_to_kind(18), Some(PolyKind::FT3));
        assert_eq!(slot_to_kind(19), Some(PolyKind::GT4));
        // Slots 0..7 are unused.
        for s in 0..8 {
            assert_eq!(slot_to_kind(s), None, "slot {s} should be unused");
        }
        assert_eq!(slot_to_kind(20), None);
        assert_eq!(slot_to_kind(100), None);
    }

    #[test]
    fn topology_is_parity_based_even_tri_odd_quad() {
        // Definitive from AVSZ3 (tri) vs AVSZ4 (quad): even kinds are
        // triangles, odd kinds are quads - across BOTH the 8..11 and
        // 12..19 ranges, so neither range is a uniform vertex count.
        for slot in LOW_MODE_START..HIGH_MODE_END {
            let vc = slot_to_kind(slot).unwrap().vertex_count();
            let expected = if slot % 2 == 0 { 3 } else { 4 };
            assert_eq!(vc, expected, "slot {slot} topology (parity)");
        }
    }

    #[test]
    fn slots_8_to_11_are_the_only_ncc_lit_handlers() {
        assert_eq!(slot_lit(8), NccMode::Nccs);
        assert_eq!(slot_lit(9), NccMode::Nccs);
        assert_eq!(slot_lit(10), NccMode::Ncct);
        assert_eq!(slot_lit(11), NccMode::NcctNccs);
        // Every other slot is unlit.
        for s in 0..8 {
            assert_eq!(slot_lit(s), NccMode::None, "slot {s}");
        }
        for s in 12..20 {
            assert_eq!(slot_lit(s), NccMode::None, "slot {s}");
        }
        assert!(slot_lit(8).is_lit() && slot_lit(11).is_lit());
        assert!(!slot_lit(12).is_lit());
    }

    #[test]
    fn resolve_carries_the_light_source_for_lit_slots() {
        let r = resolve(11, AlphaState::Off, Variant::Scus).unwrap();
        assert_eq!(r.lit, NccMode::NcctNccs);
        assert!(r.is_lit());
        // ...and the lit handlers stay lit on the overlay variant too
        // (they are bank-invariant / shared).
        let r = resolve(8, AlphaState::Half, Variant::Overlay).unwrap();
        assert_eq!(r.lit, NccMode::Nccs);
        assert!(r.is_lit());
        // Unlit slot.
        let r = resolve(12, AlphaState::Off, Variant::Scus).unwrap();
        assert_eq!(r.lit, NccMode::None);
        assert!(!r.is_lit());
    }

    #[test]
    fn handler_addresses_are_pinned_for_populated_slots() {
        assert_eq!(SLOT_HANDLER_VA[8], Some(0x8004_409C));
        assert_eq!(SLOT_HANDLER_VA[11], Some(0x8004_45B0));
        assert_eq!(SLOT_HANDLER_VA[19], Some(0x8004_3F10));
        for (s, va) in SLOT_HANDLER_VA.iter().enumerate().take(8) {
            assert_eq!(*va, None, "slot {s}");
        }
        // Every slot that has a kind has a pinned handler address, and
        // vice-versa.
        for (s, va) in SLOT_HANDLER_VA.iter().enumerate() {
            assert_eq!(
                slot_to_kind(s).is_some(),
                va.is_some(),
                "slot {s} kind/handler agreement"
            );
        }
    }

    #[test]
    fn alpha_state_round_trips_raw_byte() {
        for s in [
            AlphaState::Off,
            AlphaState::Half,
            AlphaState::Additive,
            AlphaState::Subtractive,
        ] {
            assert_eq!(AlphaState::from_raw(s.raw_byte()), s, "{s:?}");
        }
        // Unknown bytes fall back to Off (matches FUN_80043390 default).
        assert_eq!(AlphaState::from_raw(0xFF), AlphaState::Off);
        assert_eq!(AlphaState::from_raw(0x33), AlphaState::Off);
    }

    #[test]
    fn alpha_row_indices_are_unique_and_ordered() {
        let rows: Vec<usize> = [
            AlphaState::Off,
            AlphaState::Half,
            AlphaState::Additive,
            AlphaState::Subtractive,
        ]
        .iter()
        .map(|a| a.row_index())
        .collect();
        assert_eq!(rows, vec![0, 1, 2, 3]);
    }

    #[test]
    fn resolve_returns_none_for_unused_slots() {
        for s in 0..8 {
            assert_eq!(resolve(s, AlphaState::Off, Variant::Scus), None);
            assert_eq!(resolve(s, AlphaState::Half, Variant::Overlay), None);
        }
    }

    #[test]
    fn resolve_scus_preserves_alpha() {
        let r = resolve(12, AlphaState::Half, Variant::Scus).unwrap();
        assert_eq!(r.kind, PolyKind::F3);
        assert_eq!(r.variant, Variant::Scus);
        assert_eq!(r.alpha, AlphaState::Half);
        assert!(!r.applies_fog());
    }

    #[test]
    fn resolve_overlay_forces_alpha_off() {
        // Mirrors FUN_80043390's overlay-path skip of the alpha-offset
        // add: every overlay-table lookup goes through row 0 only.
        for alpha in [
            AlphaState::Off,
            AlphaState::Half,
            AlphaState::Additive,
            AlphaState::Subtractive,
        ] {
            let r = resolve(13, alpha, Variant::Overlay).unwrap();
            assert_eq!(r.kind, PolyKind::F4);
            assert_eq!(r.variant, Variant::Overlay);
            assert_eq!(r.alpha, AlphaState::Off);
            assert!(r.applies_fog());
        }
    }

    #[test]
    fn poly_kind_topology_helpers_match_psx_conventions() {
        for k in [PolyKind::F3, PolyKind::FT3, PolyKind::G3, PolyKind::GT3] {
            assert_eq!(k.vertex_count(), 3, "{k:?}");
        }
        for k in [PolyKind::F4, PolyKind::FT4, PolyKind::G4, PolyKind::GT4] {
            assert_eq!(k.vertex_count(), 4, "{k:?}");
        }
        for k in [PolyKind::FT3, PolyKind::GT3, PolyKind::FT4, PolyKind::GT4] {
            assert!(k.is_textured(), "{k:?}");
        }
        for k in [PolyKind::F3, PolyKind::G3, PolyKind::F4, PolyKind::G4] {
            assert!(!k.is_textured(), "{k:?}");
        }
        for k in [PolyKind::G3, PolyKind::GT3, PolyKind::G4, PolyKind::GT4] {
            assert!(k.is_gouraud(), "{k:?}");
        }
        for k in [PolyKind::F3, PolyKind::FT3, PolyKind::F4, PolyKind::FT4] {
            assert!(!k.is_gouraud(), "{k:?}");
        }
    }

    #[test]
    fn scene_render_policy_routes_world_map_to_overlay_variant() {
        assert_eq!(
            SceneRenderPolicy::from_scene_kind(SceneKindHint::WorldMap).variant,
            Variant::Overlay
        );
        for hint in [
            SceneKindHint::Field,
            SceneKindHint::Battle,
            SceneKindHint::Cutscene,
        ] {
            assert_eq!(
                SceneRenderPolicy::from_scene_kind(hint).variant,
                Variant::Scus,
                "{hint:?} should use SCUS variant"
            );
        }
    }

    #[test]
    fn overlay_variant_always_applies_fog_post_process() {
        // The overlay variant's per-Z LUT-tint haze applies to every
        // populated slot (the low-mode lit handlers fog-track through the
        // variant; the high-mode handlers add dpcs/dpct).
        for slot in LOW_MODE_START..HIGH_MODE_END {
            let r = resolve(slot, AlphaState::Off, Variant::Overlay).unwrap();
            assert!(
                r.applies_fog(),
                "slot {slot} should apply fog in overlay variant"
            );
        }
    }

    #[test]
    fn scus_variant_never_applies_overlay_fog() {
        // The overlay LUT-tint post-process is overlay-only; the SCUS
        // path never runs it, regardless of alpha bank.
        for slot in LOW_MODE_START..HIGH_MODE_END {
            for alpha in [
                AlphaState::Off,
                AlphaState::Half,
                AlphaState::Additive,
                AlphaState::Subtractive,
            ] {
                let r = resolve(slot, alpha, Variant::Scus).unwrap();
                assert!(
                    !r.applies_fog(),
                    "slot {slot} alpha {alpha:?} should NOT apply overlay fog"
                );
            }
        }
    }

    #[test]
    fn scus_depth_cue_tracks_the_alpha_bank() {
        // Retail table fact: bank 0 (Off) emits no colour op, but banks
        // 1/2/3 point at the DPCS/DPCT bodies - a semi-transparent SCUS
        // prim runs the depth cue.
        let opaque = resolve(15, AlphaState::Off, Variant::Scus).unwrap();
        assert!(
            !opaque.applies_depth_cue(),
            "opaque SCUS prim: no depth cue"
        );
        for alpha in [
            AlphaState::Half,
            AlphaState::Additive,
            AlphaState::Subtractive,
        ] {
            let r = resolve(15, alpha, Variant::Scus).unwrap();
            assert!(
                r.applies_depth_cue(),
                "semi-transparent SCUS prim (alpha {alpha:?}) runs DPCS/DPCT"
            );
        }
        // Overlay always applies its depth cue.
        assert!(
            resolve(15, AlphaState::Off, Variant::Overlay)
                .unwrap()
                .applies_depth_cue()
        );
    }
}
