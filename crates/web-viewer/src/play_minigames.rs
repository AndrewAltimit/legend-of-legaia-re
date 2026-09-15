//! In-world minigames on the play page: the draw + input side of the
//! sessions the shared scene host installs.
//!
//! Entry is already host-agnostic: the field-VM op-`0x3E` `op0 >= 100` arm
//! and the world-map `MinigameDoor` walk-on publish `World::minigames
//! .pending_warp`, and `SceneHost::tick` drains it into
//! `enter_fishing / enter_slot / enter_baka / enter_muscle / enter_dance`
//! on both hosts. Fishing has its own module ([`crate::play_fishing`]); this
//! one is the surface for the other four `SceneMode`s, which the page used
//! to enter with a frozen field and no UI.
//!
//! # Shape
//!
//! Three things happen here and nowhere else on the page:
//!
//! 1. **Presentation assets.** The standalone minigames page already decodes
//!    every one of the four games' art off the disc - the slot machine's
//!    scene graph + art pack, the dome's arena / fighter / monster meshes and
//!    hub pages, the Baka roster meshes + stage, the dance hall + dancers -
//!    through [`crate::minigames::LegaiaMinigames`]. Rather than port those
//!    builders a second time against the engine's `ProtIndex`, the play page
//!    hands that same type a **compact PROT image**: the minigame entries
//!    copied out of the loaded archive behind a synthetic TOC of the retail
//!    shape ([`compact_prot_image`]), so every `*_rgba` / `*_positions` /
//!    `*_json` export the standalone page calls works unchanged here. The
//!    image is built once, on the first minigame entry.
//! 2. **Session read-outs.** The rules engines run in the engine's own
//!    `World` (`tick_slot_machine` / `tick_baka_fighter` / `tick_muscle_dome`
//!    / `tick_dance`, off the pad word the page already routes through
//!    `set_pad`), so the page adds no input path. The per-game state JSON the
//!    standalone renderers read (`slot_state_json` ...) is rebuilt here over
//!    the *world's* session instead of the standalone's private one.
//! 3. **The text HUD**, through [`LegaiaRuntime::minigame_overlay_draws`]:
//!    the same `format!` lines the native window prints at `(8, 62)` /
//!    `(8, 80)`, stage-scaled onto the overlay canvas. Native is the poorer
//!    host here (it draws no art for three of the four); the page draws the
//!    art *and* the lines, so a disc whose art does not decode still tells the
//!    player which phase the session is in.
//!
//! Exit is the engine's: Start leaves any minigame (`World::poll_minigame_escape`)
//! and each game's own `exit_*` runs the bookkeeping - the slot cash-out into
//! the coin bank, the Baka winnings through the mode-24 return warp, the dome
//! contest's settle. This module only observes the edge and tells the page to
//! put the field VRAM back.
//!
//! Per-game exports live in the sibling modules
//! [`crate::play_minigame_slots`] and [`crate::play_minigame_arena`].

use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};

use crate::minigames::LegaiaMinigames;
use crate::runtime::LegaiaRuntime;

/// Which of the four in-world minigames owns the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveGame {
    Slot,
    Baka,
    Muscle,
    Dance,
}

impl ActiveGame {
    pub(crate) fn of_mode(mode: SceneMode) -> Option<Self> {
        match mode {
            SceneMode::SlotMachine => Some(Self::Slot),
            SceneMode::BakaFighter => Some(Self::Baka),
            SceneMode::MuscleDome => Some(Self::Muscle),
            SceneMode::Dance => Some(Self::Dance),
            _ => None,
        }
    }

    /// The page-facing label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Baka => "baka",
            Self::Muscle => "muscle",
            Self::Dance => "dance",
        }
    }
}

/// Presentation state for the in-world minigame screens.
#[derive(Default)]
pub(crate) struct MinigameUi {
    /// The standalone page's presentation bundle over the compact PROT image
    /// - built on the first minigame entry, kept for the session.
    pub(crate) art: Option<Box<LegaiaMinigames>>,
    /// The build was attempted and failed (no disc, or the image did not
    /// parse); don't retry every frame.
    art_failed: bool,
    /// The game that owned the screen last tick (edge detection).
    pub(crate) game: Option<ActiveGame>,
    /// Bumped on every entry so the page rebuilds its scene.
    pub(crate) generation: u32,
    /// Muscle Dome hub-screen timers + staged opponent.
    pub(crate) muscle: crate::play_minigame_arena::MuscleUi,
    /// Baka Fighter's staged opponent.
    pub(crate) baka: crate::play_minigame_arena::BakaUi,
    /// Slot machine payout edges.
    pub(crate) slot: crate::play_minigame_slots::SlotUi,
    /// A minigame that uploaded its own VRAM just left: the page must put
    /// the field texture back. Drained by `play_mg_take_vram_restore`.
    pub(crate) vram_restore_pending: bool,
}

/// The PROT entries the standalone presentation bundle reads, in extraction
/// index space: the player battle files + monster archive (dome bodies),
/// the SFX banks the dome / duel cues resolve through, `etim`, the party
/// field pack (Noa's dance atlas), the battle overlay (dome tables), the five
/// minigame overlays, and the `1198..=1231` art/scene band (slot art + SFX,
/// Baka HUD/stage/roster packs, dome hub + arena, the dance hall scene block
/// + its art, chart SFX and choreography).
const COMPACT_PROT_ENTRIES: &[std::ops::RangeInclusive<u32>] = &[
    863..=870,
    874..=874,
    876..=876,
    889..=889,
    898..=898,
    972..=972,
    975..=977,
    980..=980,
    1198..=1231,
];

/// PROT sector size.
const SECTOR: usize = 0x800;

/// Build a **compact PROT.DAT image** out of the loaded archive: a header +
/// TOC of the retail shape (`[pad][file_num - 1][header_sectors]`, then one
/// start LBA per entry with entry `p` at `toc[p + 2]`), followed by the
/// wanted entries' bytes sector-aligned. Every other entry is one zero
/// sector, so the TOC stays monotonic, every index keeps its number, and a
/// parser pointed at an absent entry fails its magic check rather than
/// reading a neighbour.
///
/// The standalone bundle's readers (`parse_prot_toc`, `Archive::from_bytes`)
/// walk this exactly as they walk the disc's own file, so nothing about
/// their entry arithmetic is duplicated here - only the container is.
pub(crate) fn compact_prot_image(index: &ProtIndex) -> Option<Vec<u8>> {
    let entries = index.entries();
    let count = entries.iter().map(|e| e.index + 1).max()? as usize;
    let header_bytes = 16 + 4 * (count + 1);
    let header_sectors = header_bytes.div_ceil(SECTOR);
    let mut body: Vec<u8> = Vec::new();
    let mut lbas: Vec<u32> = Vec::with_capacity(count + 1);
    let mut cursor = header_sectors as u32;
    for p in 0..count as u32 {
        lbas.push(cursor);
        let wanted = COMPACT_PROT_ENTRIES.iter().any(|r| r.contains(&p));
        let bytes = if wanted {
            entries
                .iter()
                .find(|e| e.index == p)
                .and_then(|e| {
                    index
                        .prot_dat_raw_bytes(e.byte_offset, e.size_bytes as usize)
                        .ok()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let sectors = bytes.len().div_ceil(SECTOR).max(1);
        let padded = sectors * SECTOR;
        body.extend_from_slice(&bytes);
        body.resize(body.len() + (padded - bytes.len()), 0);
        cursor += sectors as u32;
    }
    lbas.push(cursor);
    let mut img = vec![0u8; header_sectors * SECTOR];
    img[4..8].copy_from_slice(&(count as u32).to_le_bytes());
    img[8..12].copy_from_slice(&(header_sectors as u32).to_le_bytes());
    for (i, lba) in lbas.iter().enumerate() {
        let off = 16 + 4 * i;
        img[off..off + 4].copy_from_slice(&lba.to_le_bytes());
    }
    img.extend_from_slice(&body);
    Some(img)
}

impl LegaiaRuntime {
    /// The presentation bundle, built on first use. `None` when no disc is
    /// loaded or the compact image did not parse.
    pub(crate) fn ensure_minigame_art(&mut self) -> bool {
        if self.minigame_ui.art.is_some() {
            return true;
        }
        if self.minigame_ui.art_failed {
            return false;
        }
        let Some(host) = self.scene_host.as_ref() else {
            return false;
        };
        let Some(img) = compact_prot_image(&host.index) else {
            self.minigame_ui.art_failed = true;
            return false;
        };
        let mut art = Box::new(LegaiaMinigames::new());
        if art.load_disc(img).is_err() {
            self.minigame_ui.art_failed = true;
            return false;
        }
        self.minigame_ui.art = Some(art);
        true
    }

    /// The presentation bundle, when built.
    pub(crate) fn minigame_art(&self) -> Option<&LegaiaMinigames> {
        self.minigame_ui.art.as_deref()
    }

    /// Fire one of the minigame rules engines' SFX cues through the page's
    /// scheduler. The engine's cue ids are the static descriptor rows
    /// (`< 0x100`); a runtime-bank id (`>= 0x200`) has no page-side voice
    /// yet and is dropped, the same silence the native window keeps when no
    /// audio is attached.
    pub(crate) fn minigame_sfx(&mut self, id: u16) {
        if let Ok(id) = u8::try_from(id) {
            self.enqueue_sfx(id, 0);
        }
    }

    /// Per-tick presentation step. Cheap no-op outside a minigame mode.
    pub(crate) fn tick_minigame_ui(&mut self) {
        let now = self
            .scene_host
            .as_ref()
            .and_then(|h| ActiveGame::of_mode(h.world.mode));
        let prev = self.minigame_ui.game;
        if now != prev {
            if let Some(g) = prev {
                self.on_minigame_exit(g);
            }
            if let Some(g) = now {
                self.on_minigame_enter(g);
            }
            self.minigame_ui.game = now;
        }
        match now {
            Some(ActiveGame::Slot) => self.tick_slot_ui(),
            Some(ActiveGame::Baka) => self.tick_baka_ui(),
            Some(ActiveGame::Muscle) => self.tick_muscle_ui(),
            Some(ActiveGame::Dance) | None => {}
        }
        // The dome's between-leg hub screens (INTERVAL + tally) run after
        // the leg has closed, i.e. back in the field mode - so the hub tick
        // runs every frame, not only inside the dome.
        self.tick_muscle_hub();
    }

    fn on_minigame_enter(&mut self, game: ActiveGame) {
        self.minigame_ui.generation = self.minigame_ui.generation.wrapping_add(1);
        self.ensure_minigame_art();
        match game {
            ActiveGame::Slot => self.enter_slot_ui(),
            ActiveGame::Baka => self.enter_baka_ui(),
            ActiveGame::Muscle => self.enter_muscle_ui(),
            ActiveGame::Dance => {}
        }
    }

    fn on_minigame_exit(&mut self, game: ActiveGame) {
        match game {
            // The slot machine draws on the page's own 2D layer; the field
            // VRAM was never replaced.
            ActiveGame::Slot => {}
            ActiveGame::Baka | ActiveGame::Muscle | ActiveGame::Dance => {
                self.minigame_ui.vram_restore_pending = true;
            }
        }
        if game == ActiveGame::Muscle {
            self.exit_muscle_ui();
        }
    }

    /// Overlay quads (surface pixels) for the active in-world minigame,
    /// appended to `play_overlay_draws_json`'s lists. Empty outside one.
    /// Read-only by design (the composite holds the menu assets borrowed);
    /// any per-frame state moves in [`Self::tick_minigame_ui`].
    pub(crate) fn minigame_overlay_draws(
        &self,
        font: &legaia_font::Font,
        surface_w: u32,
        surface_h: u32,
    ) -> (Vec<SpriteDraw>, Vec<TextDraw>) {
        let Some(game) = self.minigame_ui.game else {
            return (Vec::new(), Vec::new());
        };
        let mut texts = match game {
            ActiveGame::Slot => self.slot_status_draws(font),
            ActiveGame::Baka => self.baka_status_draws(font),
            ActiveGame::Muscle => self.muscle_status_draws(font),
            ActiveGame::Dance => self.dance_status_draws(font),
        };
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        (Vec::new(), texts)
    }
}

/// Stage-space text rows at the native window's HUD pens: `(8, 44)` for a
/// contest line, `(8, 62)` for the status line, `(8, 80)` for the prompt.
pub(crate) const PEN_CONTEST: (i32, i32) = (8, 44);
pub(crate) const PEN_STATUS: (i32, i32) = (8, 62);
pub(crate) const PEN_PROMPT: (i32, i32) = (8, 80);
pub(crate) const PEN_EXTRA: (i32, i32) = (8, 98);
pub(crate) const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub(crate) const DIM: [f32; 4] = [0.65, 0.72, 0.8, 1.0];

/// One text row in stage space.
pub(crate) fn row(
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    ui::text_draws_for(&font.layout_ascii(text), pen, color)
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl LegaiaRuntime {
    /// Which in-world minigame owns the screen this frame, with the scene
    /// generation the page keys its uploads on:
    ///
    /// ```json
    /// { "game": "slot" | "baka" | "muscle" | "dance" | null, "gen": 3,
    ///   "art": true,
    ///   "muscle": { "monster_id": 170, "char_slot": 0 },
    ///   "baka": { "opponent": 5, "player_char": 0 } }
    /// ```
    ///
    /// `art` says whether the presentation bundle decoded; without it the
    /// page draws the text HUD alone and says so.
    pub fn play_mg_game_json(&self) -> String {
        let ui = &self.minigame_ui;
        serde_json::json!({
            "game": ui.game.map(ActiveGame::label),
            "gen": ui.generation,
            "art": ui.art.is_some(),
            "muscle": {
                "monster_id": ui.muscle.monster_id,
                "char_slot": ui.muscle.char_slot,
            },
            // The three party-side roster rows (`0..=2`) share the PROT 1204
            // party pack (retail folds them at `FIGHTER_PACK_FOLD`), so an
            // opponent drawn from one of them is a side-0 mesh; the ladder
            // rows `3..=16` carry their own packs (side 1).
            "baka": {
                "opponent": ui.baka.opponent,
                "opponent_side": u32::from(
                    ui.baka.opponent >= legaia_engine_core::baka_fighter::FIGHTER_PACK_FOLD
                ),
                "player_char": ui.baka.player_char,
            },
        })
        .to_string()
    }

    /// `true` once per minigame exit that replaced the VRAM texture: the
    /// page re-uploads `field_vram_bytes` on it. Clears the flag.
    pub fn play_mg_take_vram_restore(&mut self) -> bool {
        std::mem::take(&mut self.minigame_ui.vram_restore_pending)
    }

    /// Developer / test affordance: arm the mode-24 door warp for `sub_id`
    /// exactly as the field-VM `0x3E` arm does (`3` = slot machine, `4` =
    /// Baka Fighter, `5` = Muscle Dome, `6` = dance, `0` = fishing). The
    /// next `tick_frame` drains it through the scene host's own loader, so
    /// the session that results is the one a casino door installs. Returns
    /// `false` when no scene is loaded. The native window's `O` / `B` / `M`
    /// hotkeys are this affordance's counterpart.
    pub fn play_mg_debug_warp(&mut self, sub_id: u8) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        host.world.arm_minigame_warp();
        host.world.minigames.pending_warp = Some(sub_id);
        true
    }
}
