//! Materialised scene resources - the runtime view that turns a [`Scene`]'s
//! raw entry bytes into a populated PSX VRAM and a parsed TMD pool, without
//! the legacy `tim_scan/<entry>/` filesystem intermediate the asset-viewer
//! used.
//!
//! REF: FUN_8001FE70 (the battle-init `scene_tmd_stream` loader - chunk 0's
//! TMD allocated and copied with its pointer at `0x8007B864`, type-`0x01`
//! chunks uploaded through `FUN_800198E0`, a type-`0x02` chunk or zero size
//! ending the walk. Its walker is ported as `legaia_scene_formats::scene_tmd_stream`'s
//! type-`0x02`-terminated chunk walker; this module's whole-scene magic scan
//! is not that routine, which the old `PORT:` here claimed it was.)
//!
//! NB this module's TMD pool is a **magic scan** over the scene's raw entry
//! bytes, not retail's registration order: it cannot see a TMD inside an
//! LZS-compressed bundle descriptor, so for a scene whose models ship that way
//! it under-reports the pool (`koin3` 1 vs 77, `other7` 0 vs 65). The id space
//! a placement's `model_index` and the scripted-motion VM's op `0x0E` operand
//! index is [`crate::model_bank`], which walks the loader's own order and
//! carries the `FUN_80026B4C` tag this module used to claim.
//!
//! This is the engine-side mirror of the retail field-loader chain
//! ([`docs/subsystems/asset-loader.md`]): the runtime DMAs every TIM in the
//! scene's CDNAME block into VRAM up front (so cross-entry CLUT references
//! resolve), then the renderer walks each TMD with its CBA / TSB pointing
//! into the now-populated VRAM. This module performs the same bulk pre-pass
//! against bytes already in memory - so an engine binary can boot a scene
//! straight from `PROT.DAT` without any pre-extracted scan dirs.
//!
//! Build once per scene transition with [`SceneResources::build`]. Drop
//! when the scene changes.
//!
//! ## Shared-block overlay
//!
//! The retail field / town engine doesn't only load the scene's own
//! CDNAME block - it also keeps a small number of shared blocks resident
//! in VRAM across every field transition. The two confirmed examples are
//! `init_data` (PROT 0; shared UI/sprite atlas slots) and `player_data`
//! (PROT 876; the player character TMD + 256x256 atlas at VRAM
//! `fb=(768, 0)`). These survive scene transitions because they live in
//! VRAM slots the per-scene loader never touches.
//!
//! Callers that want this engine-side parity drive
//! [`SceneResources::build_with_shared`] and pass [`FIELD_SHARED_BLOCKS`]
//! as the shared scene set. The default zero-arg [`SceneResources::build`]
//! stays backward-compatible.
//! REF: FUN_8001E890

use anyhow::Result;
use legaia_actor_formats::battle_data_pack;
use legaia_container_formats::{anm_detect, pack, tim_scan, tmd_scan};
use legaia_disc_audit::categorize::Class;
use legaia_scene_formats::{kingdom_bundle, scene_tmd_stream, world_map_overlay};
use legaia_tim::Vram;

use crate::scene::{Scene, SceneEntry};

mod build;
mod load_kind;
mod resolved;
mod shared_build;
#[cfg(test)]
mod tests;
mod tmd_pools;

pub use load_kind::*;
pub use resolved::*;
use tmd_pools::*;

/// Default shared CDNAME blocks the retail field / town engine keeps
/// resident across scene transitions. Pair with
/// [`SceneResources::build_with_shared`] to match the runtime VRAM layout.
///
/// `init_data` (PROT 0) holds shared sprite / UI tiles at VRAM
/// `fb=(704, 0)` and `fb=(704, 256)`. `player_data` (PROT 876) holds
/// the player-character TMD + 256x256 atlas at VRAM `fb=(768, 0)` with
/// CLUT at `(0, 500)`.
///
/// `battle_data` (PROT 865..869) holds the character / monster TMDs
/// the retail engine loads at battle scene init via the
/// `FUN_8001E890` chain. It is NOT in the field shared set: the
/// byte-match corpus shows the row-479 NPC palettes that town01's
/// scene_tmd_stream entries reference don't appear in any
/// battle_data record (the corpus did pin record-pool placements at
/// `fb_x=768` and `fb_x=864`, but those are battle character
/// textures, not field-NPC CLUTs).
///
/// Engines that want battle_data resident before they enter a
/// battle scene can build a separate boot VRAM with
/// [`SceneResources::build_battle_boot_vram`] and merge it with the
/// field scene's VRAM at battle-init time. See
/// [`docs/formats/battle-data-pack.md`] for the corpus methodology
/// and [`docs/subsystems/asset-loader.md`] for the dispatch
/// boundaries between field and battle.
pub const FIELD_SHARED_BLOCKS: &[&str] = &["init_data", "player_data"];

/// CDNAME blocks the retail engine pre-loads before any battle
/// scene fires: the four player battle files `PLAYER1..4` (retail
/// block `battle_data` = raw TOC 865..868 = extraction entries
/// 863..866; see `docs/formats/cdname.md` § numbering space). The
/// extraction *labels* split that range across "edstati3"
/// (Vahn/Noa, 863/864) and "battle_data" (Gala/Terra, 865/866),
/// so both labels are listed; non-pack entries inside either block
/// (1-sector stubs, the monster-archive head) fail
/// `battle_data_pack::detect` and are skipped. Pair with
/// [`SceneResources::build_battle_boot_vram`] (or pass through to
/// [`SceneResources::build_targeted_with_options`] as a shared
/// block) when an engine wants the boot VRAM populated.
pub const BATTLE_BOOT_BLOCKS: &[&str] = &["edstati3", "battle_data"];

/// Per-scene runtime resources: VRAM populated from every TIM in the
/// CDNAME block, plus a parsed TMD pool, plus a count of how many TIMs
/// fed VRAM. Owns its bytes - safe to hold across a subsequent scene
/// transition (the next [`SceneResources::build`] yields fresh state).
#[derive(Clone)]
pub struct SceneResources {
    /// Fully-populated PSX VRAM. Every TIM that the scene's CDNAME block
    /// carries (in any of its entries) has been DMA'd into the canonical
    /// `(fb_x, fb_y)` slot encoded in its TIM header - same model the PSX
    /// runtime uses, so cross-entry CLUT references resolve naturally.
    pub vram: Vram,
    /// Number of TIMs uploaded to [`SceneResources::vram`]. Useful for
    /// HUD / log output.
    pub tim_count: usize,
    /// Number of TIMs that failed to parse (typically zero - the TIM scanner
    /// is conservative). When non-zero, indicates the entry bytes carry a
    /// header magic that passes the cheap scan but fails the structural
    /// parse.
    pub tim_parse_failures: usize,
    /// Parsed TMDs - every TMD the scanner found across the scene's entries.
    /// The order is CDNAME-entry order, then byte-offset within each entry.
    pub tmds: Vec<ResolvedTmd>,
    /// Parsed ANM packs - every ANM container found across the scene's entries.
    /// The order is CDNAME-entry order, then byte-offset within each entry.
    pub anm_packs: Vec<ResolvedAnm>,
    /// Number of TIMs contributed by shared CDNAME blocks (e.g.
    /// `init_data`, `player_data`). Counted separately so the diagnostic
    /// "this scene has N scene-local TIMs and K shared TIMs" survives a
    /// single field. Zero when `build` was used (no shared overlay).
    pub shared_tim_count: usize,
    /// Number of TMDs contributed by shared CDNAME blocks. Same semantics
    /// as `shared_tim_count`.
    pub shared_tmd_count: usize,
    /// Per-prim renderer dispatch policy. Mirrors the runtime gate
    /// `_DAT_1F800394 & 1`: world-map scenes route every per-prim
    /// emit through the overlay variant (distance-cue fog), every
    /// other scene class uses the SCUS variant.
    pub render_policy: legaia_render_kernels::prim_dispatch::SceneRenderPolicy,
    /// Parsed kingdom slot-4 vertex pool (the per-kingdom object-mesh
    /// library), present only for `SceneLoadKind::WorldMap` scenes whose
    /// primary entry is a kingdom bundle (PROT 0085/0244/0391). `None`
    /// for every other scene class. The records are object-local GTE
    /// vertices; the faithful triangle topology lives in an unpinned
    /// cluster-A command stream, so a consumer can render them as an
    /// inspection wireframe (see `world_map_overlay::wireframe_segments_3d`)
    /// but not yet as placed solid meshes.
    pub world_map_slot4: Option<legaia_scene_formats::world_map_overlay::KingdomSlot4>,
}

impl SceneResources {
    /// Look up a TMD by its `(entry_idx, offset)` coordinates. Useful when
    /// an actor is bound to a specific TMD slot and the engine needs to
    /// fetch it from the resource pool.
    pub fn tmd_by_coords(&self, entry_idx: u32, offset: usize) -> Option<&ResolvedTmd> {
        self.tmds
            .iter()
            .find(|t| t.entry_idx == entry_idx && t.offset == offset)
    }

    /// Look up the ANM pack for actor slot `actor_idx`. Returns `None` if the
    /// scene has fewer ANM packs than the requested slot index.
    ///
    /// Ordering follows CDNAME entry order - the same ordering `FUN_8001E890`
    /// uses to register TMDs into `0x8007C018` (actor K → slot K).
    pub fn anm_pack_for_actor(&self, actor_idx: usize) -> Option<&ResolvedAnm> {
        self.anm_packs.get(actor_idx)
    }
}
