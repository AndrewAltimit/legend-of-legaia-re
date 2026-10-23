//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;
use legaia_engine_world_views::minigame_caption::WorldMinigameCaption;

#[path = "minigames/baka_fighter.rs"]
mod baka_fighter;
#[path = "minigames/dance.rs"]
mod dance;
#[path = "minigames/fishing.rs"]
mod fishing;
#[path = "minigames/muscle_dome.rs"]
mod muscle_dome;
#[path = "minigames/slot_machine.rs"]
mod slot_machine;

/// The Baka duel surface on the GPU: the duel VRAM (re-uploaded when the
/// engine's surface generation moves - a rung seats a new opponent) and this
/// frame's posed meshes, the textured and untextured halves of the one
/// buffer set `BakaDuelScene` builds.
/// The player battle-file slot the dome's fighter assembles from (0 = Vahn,
/// the lead); the browser play page seats the same.
pub(super) const MUSCLE_DOME_CHAR_SLOT: u32 = 0;

/// The vertex streams a posed surface mesh was last uploaded from: equal
/// streams mean the GPU buffers already hold this frame's mesh.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MeshKey {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) uvs: Vec<[u8; 2]>,
    pub(super) cba_tsb: Vec<[u16; 2]>,
    pub(super) colors: Vec<[u8; 3]>,
    pub(super) flat_rgba: Vec<u8>,
    pub(super) textured_indices: Vec<u32>,
    pub(super) untextured_indices: Vec<u32>,
}

pub(super) struct BakaDuelGpu {
    pub(super) generation: u32,
    pub(super) vram: UploadedVram,
    pub(super) textured: Option<UploadedVramMesh>,
    pub(super) untextured: Option<UploadedColorMesh>,
    /// The surface camera's `vp_raw` for this frame's aspect
    /// (`DuelCamera` for the duel, `DomeCamera` for the dome).
    pub(super) mvp: Mat4,
}

/// The dance venue on the GPU: the `other7` hall the dance overlay's entry
/// loads (`legaia_engine_minigame_scenes::dance_venue::DanceVenue`), drawn in place of
/// the walked-in scene for as long as the entry's globals are staged. Its own
/// VRAM (the venue upload plus the HUD page and the five entry face stamps),
/// so the field VRAM is never touched and needs no restore.
pub(super) struct DanceVenueGpu {
    /// The staging generation this copy was built for.
    pub(super) generation: u32,
    pub(super) vram: UploadedVram,
    /// Whether the HUD page reached the venue VRAM.
    pub(super) hud_staged: bool,
    /// The hall's textured draws + the walk-ground heightfield, baked into
    /// raw world coordinates (placement transform and coplanar lift applied).
    pub(super) textured: VenueTexturedBake,
    /// The hall's untextured draws, baked the same way.
    pub(super) untextured: VenueColorBake,
    /// This frame's upload of [`Self::textured`], carrying only the
    /// triangles the PSX GPU draws under the venue camera
    /// (`dance_venue::psx_gpu_visible_indices`), and the index list it holds.
    pub(super) textured_gpu: Option<UploadedVramMesh>,
    pub(super) textured_kept: Option<Vec<u32>>,
    /// The same for [`Self::untextured`].
    pub(super) untextured_gpu: Option<UploadedColorMesh>,
    pub(super) untextured_kept: Option<Vec<u32>>,
    /// The floor's body meshes + choreography bank off this venue, handed to
    /// the cast surface when this copy is installed.
    pub(super) cast_assets:
        Option<std::sync::Arc<legaia_engine_minigame_scenes::dance_cast_scene::DanceCastAssets>>,
}

/// The venue's textured half in world space, kept CPU-side so each frame can
/// re-upload the triangles the GPU would draw.
#[derive(Default)]
pub(super) struct VenueTexturedBake {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) uvs: Vec<[u8; 2]>,
    pub(super) cba_tsb: Vec<[u16; 2]>,
    pub(super) normals: Vec<[f32; 3]>,
    pub(super) colors: Vec<[u8; 3]>,
    pub(super) indices: Vec<u32>,
}

/// The venue's untextured half in world space.
#[derive(Default)]
pub(super) struct VenueColorBake {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) colors: Vec<[u8; 3]>,
    pub(super) blend: Vec<u16>,
    pub(super) indices: Vec<u32>,
}

/// The dance floor's posed bodies on the GPU for this frame
/// ([`legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface::frame`]). They
/// sample the venue's VRAM and draw under the venue camera.
pub(super) struct DanceCastGpu {
    pub(super) textured: Option<UploadedVramMesh>,
    pub(super) untextured: Option<UploadedColorMesh>,
}

impl PlayWindowApp {
    /// The monster stat archive (PROT 867) bytes, decoded + cached on first
    /// use. `None` if no disc is attached or the entry can't be read.
    pub(super) fn monster_archive_bytes(&mut self) -> Option<std::sync::Arc<Vec<u8>>> {
        if self.monster_archive.is_none() {
            const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;
            match self
                .session
                .host
                .index
                .entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY)
            {
                Ok(b) => self.monster_archive = Some(std::sync::Arc::new(b)),
                Err(e) => {
                    log::warn!("play-window: monster archive (PROT 867) load skipped: {e:#}");
                    return None;
                }
            }
        }
        self.monster_archive.clone()
    }
}

impl PlayWindowApp {
    /// Fire the minigame sessions' queued SFX cues (the fishing hub's and
    /// point exchange's blips) into the director's scheduler - the session's
    /// step (`BootSession::enqueue_minigame_sfx_cues`), shared with the
    /// browser play page.
    pub(super) fn drain_minigame_sfx_cues(&mut self) {
        let _ = self.session.enqueue_minigame_sfx_cues();
    }
}

impl PlayWindowApp {
    /// Re-upload `cpu_vram_base` to the GPU. Silent when the renderer is not
    /// up yet (a headless tick), which is the same guard every other VRAM
    /// mutation in this window carries.
    pub(in crate::window) fn upload_cpu_vram(&mut self) {
        if let (Some(r), Some(base)) = (self.win.renderer.as_ref(), self.cpu_vram_base.as_ref()) {
            match r.upload_vram(base) {
                Ok(v) => self.uploaded_vram = Some(v),
                Err(e) => log::error!("play-window: VRAM upload: {e:#}"),
            }
        }
    }
}

impl PlayWindowApp {
    /// Per-frame driver for every minigame side-channel this window hosts:
    /// the minigame and Baka duel cue queues, the dance auto-end and venue,
    /// the fishing venue actors, the Baka round chrome and the Muscle Dome
    /// hub-screen sounds. The effect pool itself is aged by `World::tick`,
    /// where every host reaches it.
    pub(super) fn tick_minigame_extras(&mut self) {
        self.drain_minigame_sfx_cues();
        // The Baka Fighter exchange hit (`BAKA_CUE_HIT`, written by the rules
        // kernel's damage step): the session's step, which self-gates to the
        // duel. The tick's audio frame fires it against the resident class-2
        // bank the same tick.
        let _ = self.session.enqueue_baka_sfx_cues();
        self.sync_dance_venue();
        self.stage_dance_hud_art();
        self.tick_fishing_actors();
        self.tick_baka_chrome();
        self.tick_muscle_hub();
        self.tick_slot_marquee();
    }
}

/// The window's own title, shown whenever no minigame caption is up.
pub(super) const WINDOW_TITLE: &str = "legaia-engine";

impl PlayWindowApp {
    /// The live minigame's status caption (`engine-minigame-scenes::minigame_status`),
    /// the one row model the browser play page exports as
    /// `play_mg_caption_json`. `None` outside a minigame.
    ///
    /// This host's two answers: the slot machine draws itself once its art
    /// is uploaded, and the fishing rows name pad buttons (the window's key
    /// map is fixed) plus `P`, the key that opens the point exchange.
    pub(super) fn minigame_caption(
        &self,
    ) -> Option<legaia_engine_minigame_scenes::minigame_status::MinigameCaption> {
        use legaia_engine_minigame_scenes::minigame_status as ms;
        self.session.host.world.minigame_caption(&ms::StatusHost {
            slot_machine_drawn: self.slot_gpu.is_some(),
            fishing_keys: ms::FishingKeys {
                cast: "Circle",
                reel_a: "Cross",
                reel_b: "Square",
                menu: "Triangle",
                quit: "Start",
                prizes: "P",
            },
        })
    }

    /// The title bar text for this frame: the window's name, then the
    /// minigame caption when one has rows.
    pub(super) fn window_title_line(&self) -> String {
        match self.minigame_caption().map(|c| c.title_line()) {
            Some(line) if !line.is_empty() => format!("{WINDOW_TITLE} - {line}"),
            _ => WINDOW_TITLE.to_string(),
        }
    }

    /// Show the minigame caption in the **title bar**, outside the game
    /// picture. The rows used to be printed inside the frame at fixed stage
    /// pens, where they lost to the game's own chrome: this window draws a
    /// game's screen primitives after its text layer, so the Baka Fighter
    /// title logo sat on the prompt row, and a long row ran off the frame's
    /// right edge. The default window is exactly the 4:3 stage, with no
    /// margin to move them into, so the caption leaves the surface
    /// altogether - which also keeps it out of every captured frame.
    /// Written only when the line changes.
    pub(super) fn sync_minigame_caption_title(&mut self) {
        let line = self.window_title_line();
        if line == self.caption_title {
            return;
        }
        if let Some(w) = self.win.window.as_ref() {
            w.set_title(&line);
        }
        self.caption_title = line;
    }
}
