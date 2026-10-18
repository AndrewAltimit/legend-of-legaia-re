//! Which retail load path a build mimics (`SceneLoadKind`) and the options a build takes.

/// Which retail dispatch path the engine is mimicking when it builds a
/// scene's VRAM pre-pass. Controls whether scene_tmd_stream PROT
/// entries (the battle-only character mesh + texture chunks that
/// `FUN_8001FE70` walks at battle init) contribute to the engine-side
/// VRAM **and** to the parsed TMD pool.
///
/// In retail, field / town scene-load does NOT call `FUN_8001FE70`;
/// the leading TMD inside a scene_tmd_stream entry goes to the battle
/// character TMD register (`_DAT_8007B864`) and its type-0x01 chunks
/// upload battle character textures (CLUT row 479 fb_x=128..240).
/// Both are absent from VRAM in retail town saves (e.g. captures in
/// town01 carry row 479 fb_x=0..256 = zero) - the retail engine
/// simply never renders those meshes from a field scene.
///
/// See [`docs/formats/scene-bundles.md`] (`scene_tmd_stream`) and
/// [`docs/subsystems/asset-loader.md`] for the runtime caller chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneLoadKind {
    /// Mimic the retail field / town scene loader: skip every
    /// scene_tmd_stream PROT entry's contributions to the build -
    /// both the leading TMD (a battle character mesh that retail
    /// stores in the battle character TMD register, never rendered
    /// from field scenes) and its type-0x01 TIM upload chunks (the
    /// CLUTs / textures that mesh samples). With both filtered out
    /// the field-mode VRAM matches retail town saves, and the parsed
    /// TMD pool drops to "field NPC + terrain meshes only" so the
    /// prim filter doesn't drag down the keep ratio with battle
    /// meshes it can't texture.
    Field,
    /// Mimic the retail battle scene loader: include the leading TMD
    /// AND the in-tail `FUN_8001FE70` type-0x01 chunks AND any
    /// post-terminator continuation chunks. This is the "upload
    /// everything that looks like a TIM and parse every embedded
    /// TMD" pass that the engine port has historically used as its
    /// `build_targeted` default.
    Battle,
    /// Mimic the world-map scene loader. Texture / TMD inclusion is
    /// the same as `Field`, but the resulting [`SceneResources`] also
    /// carry a [`SceneRenderPolicy::world_map`] so the renderer routes
    /// every per-prim emit through the overlay variant (`dpcs` /
    /// `dpct` fog post-process). See
    /// [`docs/subsystems/world-map.md`].
    WorldMap,
}

impl SceneLoadKind {
    /// Mirrors the retail field-vs-battle filter decision. World-map
    /// inherits the field-mode filter (no battle character meshes) and
    /// gates the overlay variant separately.
    pub(super) fn excludes_scene_tmd_stream(self) -> bool {
        matches!(self, SceneLoadKind::Field | SceneLoadKind::WorldMap)
    }

    /// Map to the engine-vm `SceneKindHint` consumed by
    /// [`SceneRenderPolicy::from_scene_kind`].
    pub fn render_hint(self) -> legaia_render_kernels::prim_dispatch::SceneKindHint {
        use legaia_render_kernels::prim_dispatch::SceneKindHint;
        match self {
            SceneLoadKind::Field => SceneKindHint::Field,
            SceneLoadKind::Battle => SceneKindHint::Battle,
            SceneLoadKind::WorldMap => SceneKindHint::WorldMap,
        }
    }
}

/// Options forwarded to [`SceneResources::build_targeted_with_options`].
/// Kept in a struct so future build-time switches (e.g. "include the
/// post-terminator continuation list but not the in-tail one", or
/// "preserve VRAM from the previous scene") can be added without
/// breaking the API.
#[derive(Debug, Clone, Copy)]
pub struct BuildOptions<'a> {
    /// Whether the build mimics field-mode or battle-mode dispatch.
    /// Defaults to [`SceneLoadKind::Battle`] in [`BuildOptions::default`]
    /// so legacy `build_targeted` calls behave the same as before.
    pub kind: SceneLoadKind,
    /// When `true`, upload **every** collected scene TIM to its VRAM
    /// destination instead of only the render-targeted subset a mesh prim
    /// samples. This mirrors the retail field loader's "DMA every TIM"
    /// behaviour and is what the VRAM parity oracle wants (the targeted
    /// subset is a render optimisation, not a faithful VRAM image). The
    /// render path leaves this `false` so its prim filter still matches the
    /// uploaded set. Default `false`.
    pub upload_all_tims: bool,
    /// The boot-resident system-UI TIM bundle (raw PROT TOC entries 0/1;
    /// [`crate::scene::ProtIndex::system_ui_bundle`]). When set, the build
    /// layers its `FUN_800198E0`-style upload (image at declared rect,
    /// CLUT as a flattened strip) **underneath** the scene's VRAM: retail
    /// uploads raw entry 0 once at boot - before any scene load - and never
    /// evicts it, so it is resident in every scene/mode (rows 510/511
    /// strips, the `x >= 896` UI texture band, the menu-glyph atlas at
    /// `(960,256)` that town/dungeon env meshes sample via CBA
    /// `(64,510)`). Raw entry 1 (the GAME OVER page) is left out, as at
    /// retail boot. `None` skips the layer (legacy behaviour). Default
    /// `None`.
    pub system_ui: Option<&'a legaia_ui_formats::system_ui_bundle::SystemUiBundle>,
}

impl Default for BuildOptions<'_> {
    fn default() -> Self {
        Self {
            kind: SceneLoadKind::Battle,
            upload_all_tims: false,
            system_ui: None,
        }
    }
}
