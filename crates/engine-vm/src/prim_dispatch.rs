//! Per-prim renderer dispatch (engine port of `FUN_80043390`).
//!
//! Every per-actor case-5 TMD prim flows through a `(prim_mode,
//! alpha_state)` lookup that selects one of 20 per-mode renderer
//! leaves. On retail PSX, the table base is paged in two flavours:
//!
//! | Flag (`_DAT_1F800394 & 1`) | Table base   | Variant            |
//! |---|---|---|
//! | clear | `0x8007657C` | SCUS-resident, no fog                  |
//! | set   | `0x801F8968` | World-map-overlay, distance-fog post   |
//!
//! The two variants share slots 8..11 (the "low-mode" 3-vertex
//! emitters) and differ only in slots 12..19 (the "high-mode"
//! 4-vertex / textured emitters): the overlay variant adds GTE
//! `dpcs` / `dpct` plus a per-Z color-LUT tint to each vertex's
//! RGB packet. Source-of-truth lives in
//! [`docs/subsystems/world-map.md`](../../../docs/subsystems/world-map.md#per-slot-delta-vs-scus-sibling).
//!
//! The engine port mirrors this shape. Rather than emitting raw GPU
//! packets, it returns a `RenderMode` enum that captures everything a
//! wgpu-side TMD-render path needs to know:
//!
//! - which base prim type to issue (`PolyKind`),
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
//! ## Selection
//!
//! ```text
//! prim_mode  ∈ 0..20 ; mirrors the per-mode descriptor table at
//!                       DAT_8007326C (the Legaia TMD renderer's
//!                       cmd-byte table).
//! alpha_off  ∈ 0,0x50,0xA0,0xF0 ; PSX semi-transparency state.
//! ```
//!
//! - Slots 0..7 (every alpha row) are zero on retail; this port treats
//!   them as `None`.
//! - Slots 8..11 are the SCUS-shared low-mode renderers (POLY_F3 /
//!   POLY_FT3 / POLY_G3 / POLY_GT3).
//! - Slots 12..19 are the high-mode renderers; in `Variant::Overlay`
//!   they also apply the fog post-process.
//!
//! ## Engine integration
//!
//! `engine-core::SceneResources::build_targeted` consults
//! [`SceneRenderPolicy::from_scene_kind`] at scene-load time so the
//! resource-build path knows which variant to ask the renderer for. The
//! world-map scene is the only retail case that toggles the overlay
//! variant; everything else uses `Variant::Scus`.

/// One of the eight POLY family kinds the PSX GTE supports. Slot
/// mapping matches the `FUN_80043390` table layout.
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

/// Which dispatch-table variant is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// SCUS-resident high-mode renderer; no fog post-process. Used for
    /// every retail field / battle / cutscene render.
    Scus,
    /// World-map-overlay high-mode renderer; applies per-vertex
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
    /// `_DAT_1F800028 == 0x00`. Standard opaque blend.
    Off,
    /// `_DAT_1F800028 == 0x50`. 50% / 50% semi-transparency.
    Half,
    /// `_DAT_1F800028 == 0xA0`. Additive (B + F).
    Additive,
    /// `_DAT_1F800028 == 0xF0`. Subtractive (B - F).
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
    /// Numeric row index (`0..4`) matching the SCUS-path alpha offset.
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
    pub variant: Variant,
    pub alpha: AlphaState,
}

impl RenderMode {
    /// Returns `true` when this mode applies the world-map overlay's
    /// fog post-process (per-vertex `dpcs` / `dpct` plus per-Z LUT
    /// tint).
    pub fn applies_fog(self) -> bool {
        self.variant == Variant::Overlay
    }
}

/// `FUN_80043390`'s slot layout. Slots 0..7 are unused; 8..11 are
/// low-mode (3-vertex) renderers; 12..19 are high-mode renderers that
/// vary between SCUS and overlay.
pub const LOW_MODE_START: usize = 8;
pub const LOW_MODE_END: usize = 12; // exclusive
pub const HIGH_MODE_START: usize = 12;
pub const HIGH_MODE_END: usize = 20; // exclusive

/// Slot 8..19 → PolyKind. Mirrors the SCUS-side slot ordering used by
/// the Legaia TMD renderer's per-mode descriptor table at
/// `DAT_8007326C`.
const SLOT_TO_KIND: [Option<PolyKind>; 20] = {
    let mut t: [Option<PolyKind>; 20] = [None; 20];
    t[8] = Some(PolyKind::F3);
    t[9] = Some(PolyKind::FT3);
    t[10] = Some(PolyKind::G3);
    t[11] = Some(PolyKind::GT3);
    t[12] = Some(PolyKind::F4);
    t[13] = Some(PolyKind::FT4);
    t[14] = Some(PolyKind::G4);
    t[15] = Some(PolyKind::GT4);
    // Slots 16..19 are alternate variants of the four high-mode types
    // (the SCUS table has eight non-zero high-mode entries that point
    // to four distinct renderer bodies in pairs). The mapping here
    // intentionally repeats the four kinds; engine-side renderers
    // discriminate the alt variants via the alpha state, which is
    // sufficient for the cases retail exercises.
    t[16] = Some(PolyKind::F4);
    t[17] = Some(PolyKind::FT4);
    t[18] = Some(PolyKind::G4);
    t[19] = Some(PolyKind::GT4);
    t
};

/// Map an absolute prim-mode slot index to its `PolyKind`. Returns
/// `None` for the unused 0..7 range or any out-of-bounds value.
pub fn slot_to_kind(slot: usize) -> Option<PolyKind> {
    SLOT_TO_KIND.get(slot).copied().flatten()
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
    fn slot_to_kind_covers_all_high_and_low_mode_slots() {
        // Low-mode slots 8..11 map to the four 3-vertex prim kinds.
        assert_eq!(slot_to_kind(8), Some(PolyKind::F3));
        assert_eq!(slot_to_kind(9), Some(PolyKind::FT3));
        assert_eq!(slot_to_kind(10), Some(PolyKind::G3));
        assert_eq!(slot_to_kind(11), Some(PolyKind::GT3));
        // High-mode slots 12..15 map to the four 4-vertex prim kinds.
        assert_eq!(slot_to_kind(12), Some(PolyKind::F4));
        assert_eq!(slot_to_kind(13), Some(PolyKind::FT4));
        assert_eq!(slot_to_kind(14), Some(PolyKind::G4));
        assert_eq!(slot_to_kind(15), Some(PolyKind::GT4));
        // Slots 16..19 are alt variants of the four high-mode kinds.
        assert_eq!(slot_to_kind(16), Some(PolyKind::F4));
        assert_eq!(slot_to_kind(17), Some(PolyKind::FT4));
        assert_eq!(slot_to_kind(18), Some(PolyKind::G4));
        assert_eq!(slot_to_kind(19), Some(PolyKind::GT4));
        // Slots 0..7 are unused.
        for s in 0..8 {
            assert_eq!(slot_to_kind(s), None, "slot {s} should be unused");
        }
        assert_eq!(slot_to_kind(20), None);
        assert_eq!(slot_to_kind(100), None);
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
        assert_eq!(r.kind, PolyKind::F4);
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
            assert_eq!(r.kind, PolyKind::FT4);
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
    fn resolve_every_high_mode_slot_in_overlay_variant_applies_fog() {
        // The overlay variant's high-mode slots (12..19) all add the
        // distance-fog post-process; the low-mode slots (8..11) are
        // shared with the SCUS path and only fog-track through the
        // variant hint.
        for slot in HIGH_MODE_START..HIGH_MODE_END {
            let r = resolve(slot, AlphaState::Off, Variant::Overlay).unwrap();
            assert!(
                r.applies_fog(),
                "slot {slot} should apply fog in overlay variant"
            );
        }
    }

    #[test]
    fn resolve_scus_variant_never_applies_fog() {
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
                    "slot {slot} alpha {alpha:?} should NOT fog in SCUS variant"
                );
            }
        }
    }
}
