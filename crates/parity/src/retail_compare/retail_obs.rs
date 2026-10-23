//! The retail side of a comparison: [`RetailObs`], everything read out of
//! one library state's RAM and VRAM that the engine is seeded with and scored
//! against, and [`read_retail`] which builds it.

use super::*;

/// The camera observables, both sides use the same shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraObs {
    pub pitch: i16,
    pub yaw: i16,
    pub h: i16,
    pub eye: [i32; 3],
    /// The focus X / Z words as retail stores them (negated world X / Z).
    /// Without them a frame aimed at the wrong place scores its camera whole:
    /// rotation and eye are all relative to the focus.
    pub focus: [i32; 2],
}

/// Everything read off one retail state.
pub struct RetailObs {
    pub scene: String,
    /// `0x80084540`: the raw CDNAME define of the scene actually loaded.
    pub loaded_define: u16,
    /// The label a scene-change packet had already written when the state
    /// was taken, when it names a scene other than the loaded one
    /// ([`RetailObs::settle_on_loaded_scene`]).
    pub pending_scene: Option<String>,
    pub game_mode: u8,
    pub class: StateClass,
    /// `(X, footing, Z)`.
    pub player: Option<[i16; 3]>,
    /// The height a script holds the player at, when one does: the
    /// player's `+0x8E` while `+0x10 & 0x20000000` is up. Op `0x4C` nibble-4
    /// sub-2 ramps `+0x8E` and, with that bit set, writes `world_y = -value`
    /// over the floor, so the `Y` above is the script's and not a floor
    /// sample (`docs/subsystems/script-vm.md`, the actor `+0x8E` row).
    pub script_height: Option<i16>,
    pub camera: CameraObs,
    pub bgm_id: u16,
    /// Whether the field BGM slot `0x8007052C` is attached and at a non-zero
    /// volume: the playing word `0x8007B708` (raised by the replay primitive
    /// `FUN_80026478`, cleared by the stop / pause / timed-release arms) and
    /// the slot's `SsSeqSetVol` word `+0x6` (`FUN_8002657C`, zeroed by the
    /// same arms and by the battle intro's field-voice stop).
    pub bgm_sounding: bool,
    /// `_DAT_8007B854 != 0`.
    pub fog_gate: bool,
    /// The region-type mask `_DAT_8007B8F4`: `1 << type` over every `.MAP`
    /// region holding the player's tile, rebuilt by the field's per-tile
    /// refresh (`FUN_800180EC`) and left alone by a fight - on every battle
    /// capture that holds a position snapshot it is the mask of that tile.
    pub region_mask: u32,
    /// The destination-entry operand `_DAT_80073EF4` / `_DAT_80073EF8`: where
    /// the scene's last entry seated the player.
    pub entry_operand: (i16, i16),
    /// The visible-tile window, scratchpad `0x1F8003E8..EB` as signed tile
    /// offsets `[near X, near Z, far X, far Z]`
    /// (`docs/formats/encounter.md`): the field primer's stamp, or whatever
    /// op `0x46` or a camera-region record has since written over it. Set by
    /// [`RetailObs::seat_view_window`] from the state's scratchpad, on a
    /// field state only - `None` when the state carries no scratchpad.
    pub view_window: Option<[i8; 4]>,
    /// The live game-state window lifted as a save.
    pub save: Option<legaia_save::SaveFile>,
    /// The field party HUD's idle countdown `_DAT_801F348C` (field-overlay
    /// data, so read on a [`StateClass::Field`] state only). The image
    /// channel lands the engine's countdown on it at the capture tick, so
    /// the readout is up in the engine frame exactly when it is in retail's.
    pub hud_countdown: Option<i16>,
    /// The field camera parameter block (`0x8007B607..0x8007B627`) on a
    /// walkable state: the camera-region record a script or a walk-on
    /// loader last installed. The seat composes its arrival snap from it
    /// rather than from a tile re-query, because which loader ran last is
    /// walk history the save does not carry (`kor5`'s tile-X 26 / 28 band
    /// loads `P2[1]` / `P2[0]`).
    pub camera_block: Option<legaia_engine_field::camera_zone::CameraZoneConfig>,
    /// The focus pair (`0x80089118` / `0x80089120`, stored form) on a
    /// walkable state whose focus is not on the player: history the seat
    /// cannot replay. The follow ease writes the focus only on a frame it
    /// runs and the player moved (`FUN_801DB510`'s stationary test at
    /// `0x801DB578..0x801DB5A4`), and the player tick skips the ease
    /// entirely while the player is movement-locked (`FUN_801D1344`,
    /// branch at `0x801D17DC`), so a player that was poked, or carried by a
    /// script while locked, stands away from a focus that stays where the
    /// last eased frame left it. The image child takes it as
    /// `LEGAIA_SEAT_FOCUS` so it frames what retail framed; the headless
    /// `camera` channel does not, so it keeps reporting the focus miss.
    pub seat_focus: Option<[i32; 2]>,
    /// The eye translation (`0x800840B8/BC/C0`) on a walkable state, which
    /// the image child lands after its seat's snap (`LEGAIA_SEAT_EYE`). The
    /// follow ease steps the eye only on a frame the player moved and not at
    /// all while the player is movement-locked, so the eye of a state whose
    /// player stopped short of the ease's close is walk history the snap
    /// cannot replay (`town01_npc16_dialogue_first_page`: eye Y `1117`, the
    /// pose the snap composes `1444`). The headless `camera` channel keeps
    /// the snap's eye, so it goes on scoring the compose.
    pub seat_eye: Option<[i32; 3]>,
    /// Pitch / yaw / `H` on a walkable state, which the image child lands
    /// after its seat's snap (`LEGAIA_SEAT_ANGLES`) for the reason it lands
    /// the eye: the follow ease steps all six descriptor-list globals on the
    /// same moved-frame legs, so a player who stopped mid-sweep leaves them
    /// short of the target (`rimelm_wall_press_left`: yaw `220`, the snap's
    /// `233`). The headless `camera` channel keeps the snap's angles.
    pub seat_angles: Option<[i32; 3]>,
    /// The displayed frame, when the state carries VRAM + display registers.
    pub frame: Option<Frame>,
    /// Battle observables for a battle-class state; `Err` names why the
    /// state cannot seed a battle.
    pub battle: Option<std::result::Result<crate::retail_compare_battle::RetailBattle, String>>,
    /// The pause-menu screen a menu-class state shows; `Err` names why the
    /// state is not one the seed can drive to.
    pub menu: Option<std::result::Result<RetailMenu, String>>,
    /// The field-VM contexts the state holds
    /// ([`crate::retail_compare_script`]).
    pub scripts: crate::retail_compare_script::RetailScripts,
    /// The field VM's script-counter slot table (`0x801C6460`, signed
    /// halfwords). It sits outside the save window, so the card-load seed
    /// cannot restore it; scripts keep timers in it (the door-fade
    /// handshake's frame count, `jouind`'s spawn delay), and a timer seeded
    /// at `0` instead of its captured count fires on a different frame.
    pub slot_table: Option<[i16; 256]>,
    /// The BIOS `rand()` seed word (`0x80009010`,
    /// [`legaia_engine_system::rand::BIOS_RAND_SEED_ADDR`]): the position of
    /// the one stream every `jal 0x80056798` draws from. Field and overworld
    /// classes only; a battle drive searches its own streams.
    pub rand_seed: Option<u32>,
    /// The live mode-3 CLUT-cell cyclers ([`retail_cell_fx`]), handed to the
    /// image child as `LEGAIA_SEAT_CLUT_FX` so the cycled palettes show the
    /// captured phase.
    pub cell_fx: Vec<legaia_engine_field::clut_cell_fx::ClutCellFx>,
    /// The live fog-pool records ([`retail_fog`]), handed to the image child
    /// as `LEGAIA_SEAT_FOG` and installed on the frame it captures.
    pub fog: Vec<legaia_engine_field::fog_particles::FogParticle>,
    /// The live draw-kind-4 sprite-arm sheets ([`retail_sprite_arms`]),
    /// handed to the image child as `LEGAIA_SEAT_SPRITE_ARMS` and installed on
    /// the frame it captures.
    pub sprite_arms: Vec<legaia_engine_core::world::ambient::SpriteArmSeed>,
    /// The live mode-4 scroller rects and their captured texels
    /// ([`retail_scroll_rects`]), handed to the image child as a
    /// `LEGAIA_SEAT_VRAM_RECTS` file.
    pub scroll_rects: Vec<SeededVramRect>,
    /// Drawn field actors' live model ids ([`retail_object_models`]), handed
    /// to the image child as `LEGAIA_SEAT_OBJECT_MODELS` for the placed
    /// objects a motion stream re-binds.
    pub object_models: Vec<(u16, i16)>,
    /// Drawn field actors' clip words ([`retail_object_clips`]), handed to
    /// the image child as `LEGAIA_SEAT_OBJECT_CLIPS`.
    pub object_clips: Vec<ObjectClipSeed>,
    /// Field actors' live VDF morph envelopes ([`retail_morphs`]), handed to
    /// the image child as `LEGAIA_SEAT_MORPHS`.
    pub morphs: Vec<MorphSeed>,
    /// Ambient walkers' live seats ([`retail_walkers`]), handed to the image
    /// child as `LEGAIA_SEAT_WALKERS`.
    pub walkers: Vec<WalkerSeed>,
    /// The live image-panel widget ([`retail_panel`]), handed to the image
    /// child as `LEGAIA_SEAT_PANEL`; the texels it shows ride the
    /// `LEGAIA_SEAT_VRAM_RECTS` file beside the scroller rects.
    pub panel: Option<legaia_engine_effects::screen_fx::PanelWidget>,
    /// The frame's clear colour - the draw environment's `r0 / g0 / b0`
    /// (`0x8007BF5D..5F`) - handed to the image child as `LEGAIA_SEAT_CLEAR`
    /// ([`retail_clear_rgb`]).
    pub clear_rgb: Option<[u8; 3]>,
    /// The overworld curvature table's build offset ([`retail_curve_bias`]),
    /// handed to the image child as `LEGAIA_SEAT_CURVE_BIAS` when the map
    /// display has left one.
    pub curve_bias: Option<i16>,
    /// The dialogue page mark's displayed strip frame ([`retail_page_mark`]),
    /// handed to the image child as `LEGAIA_SEAT_PAGE_MARK`.
    pub page_mark: Option<u8>,
    /// The player's heading `+0x26` (retail space, `0` = -Z), when it still
    /// equals the arrival facing `_DAT_80073EFC` the entry script's `4C 3A`
    /// gave it; `None` once the pad has turned the player.
    pub player_facing: Option<i16>,
    /// The player's heading `+0x26` (retail space), whatever turned it: the
    /// seed stands the player in it, as it seats the position.
    pub player_heading: Option<i16>,
    /// `_DAT_80073EFC`, the arrival facing.
    pub arrival_facing: i16,
    /// Every field-actor-ticked placement's heading ([`retail_actor_facings`]).
    pub actor_facings: Vec<ActorFacing>,
}

/// One field actor's heading as a retail state holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorFacing {
    /// `+0x50`, the record's flat MAN index.
    pub flat: u16,
    /// `+0x26`, the yaw (retail space: `0` = -Z, 12-bit).
    pub facing: i16,
    /// `+0x14` / `+0x18`.
    pub x: i16,
    pub z: i16,
    /// `+0x64`, the live model id (bank-relative ids are not resolved).
    pub model: i16,
    /// `+0x10`.
    pub flags: u32,
    /// The heading is ambient history: the actor runs the ambient motion VM
    /// (`FUN_80038158`, [`retail_ambient_heading`]) on a stream that turns
    /// it, so `+0x26` is where the stream's ramps and `rand()`-picked
    /// wanders stood at the capture instant - time and stream history since
    /// the entry, which no seed replays.
    pub ambient: bool,
}

impl RetailObs {
    pub(super) fn from_ram(ram: &[u8], frame: Option<Frame>) -> Self {
        let scene = game_anchors::scene_name(ram);
        let game_mode = game_anchors::game_mode(ram);
        let player = game_anchors::player_ptr(ram).map(|p| {
            [
                rd16(ram, p + 0x14),
                rd16(ram, p + 0x16),
                rd16(ram, p + 0x18),
            ]
        });
        let script_height = game_anchors::player_ptr(ram)
            .filter(|&p| game_anchors::u32_at(ram, p + 0x10) & PLAYER_SCRIPT_HEIGHT != 0)
            .map(|p| rd16(ram, p + 0x8E));
        let class = StateClass::classify(game_mode, &scene, player.is_some());
        let menu = (class == StateClass::Menu).then(|| RetailMenu::from_ram(ram, &scene));
        let camera = CameraObs {
            pitch: rd16(ram, CAM_ROT),
            yaw: rd16(ram, CAM_ROT + 2),
            h: rd16(ram, GTE_H),
            eye: [
                rd32(ram, CAM_EYE),
                rd32(ram, CAM_EYE + 4),
                rd32(ram, CAM_EYE + 8),
            ],
            focus: [rd32(ram, CAM_FOCUS), rd32(ram, CAM_FOCUS + 8)],
        };
        let bgm_id = game_anchors::u16_at(ram, BGM_ID);
        let fog_gate = game_anchors::u32_at(ram, FOG_GATE) != 0;
        let lo = (LIVE_STATE_VA & 0x1F_FFFF) as usize;
        let mut save = ram.get(lo..lo + LIVE_STATE_LEN).and_then(|win| {
            let mut block = win.to_vec();
            block.resize(SC_BLOCK_LEN, 0);
            legaia_save::SaveFile::from_retail_sc_block(
                &block,
                legaia_save::RETAIL_SC_PARTY_RECORDS,
            )
            .ok()
        });
        let battle = (class == StateClass::Battle)
            .then(|| crate::retail_compare_battle::RetailBattle::from_ram(ram));
        // A capture on the results frame or after it holds the party with
        // the fight's rewards already applied; the seed replays the fight,
        // which applies them again.
        if let (Some(save), Some(Ok(b))) = (save.as_mut(), battle.as_ref()) {
            crate::retail_compare_battle::ungrant_results_rewards(save, b, ram);
            crate::retail_compare_battle::ungrant_magic_level_up(save, b);
        }
        Self {
            scene,
            loaded_define: game_anchors::u16_at(ram, LOADED_SCENE_DEFINE),
            pending_scene: None,
            game_mode,
            class,
            player,
            script_height,
            camera,
            bgm_id,
            bgm_sounding: game_anchors::u16_at(ram, BGM_PLAYING) != 0
                && game_anchors::u16_at(ram, BGM_SLOT + 6) != 0,
            fog_gate,
            region_mask: game_anchors::u32_at(ram, REGION_MASK),
            entry_operand: (
                rd32(ram, ENTRY_OPERAND) as i16,
                rd32(ram, ENTRY_OPERAND + 4) as i16,
            ),
            view_window: None,
            save,
            hud_countdown: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| rd16(ram, HUD_COUNTDOWN)),
            camera_block: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| camera_block_bytes(ram))
                .flatten()
                .and_then(|b| {
                    legaia_engine_field::camera_zone::CameraZoneConfig::from_retail_block(&b)
                })
                .filter(|b| camera_block_composed(ram, b)),
            seat_focus: match (class, player) {
                (StateClass::Field | StateClass::WorldMap, Some([x, _, z]))
                    if camera.focus != [-i32::from(x), -i32::from(z)] =>
                {
                    Some(camera.focus)
                }
                _ => None,
            },
            seat_eye: (matches!(class, StateClass::Field | StateClass::WorldMap)
                && player.is_some())
            .then_some(camera.eye),
            seat_angles: (matches!(class, StateClass::Field | StateClass::WorldMap)
                && player.is_some())
            .then_some([
                i32::from(camera.pitch),
                i32::from(camera.yaw),
                i32::from(camera.h),
            ]),
            frame,
            battle,
            menu,
            scripts: crate::retail_compare_script::RetailScripts::from_ram(ram),
            cell_fx: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_cell_fx(ram)
            } else {
                Vec::new()
            },
            fog: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_fog(ram)
            } else {
                Vec::new()
            },
            sprite_arms: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_sprite_arms(ram)
            } else {
                Vec::new()
            },
            scroll_rects: Vec::new(),
            panel: None,
            walkers: if matches!(class, StateClass::Field) {
                retail_walkers(ram)
            } else {
                Vec::new()
            },
            morphs: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_morphs(ram)
            } else {
                Vec::new()
            },
            object_models: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_object_models(ram)
            } else {
                Vec::new()
            },
            object_clips: if matches!(class, StateClass::Field) {
                retail_object_clips(ram)
            } else {
                Vec::new()
            },
            clear_rgb: matches!(class, StateClass::Field).then(|| retail_clear_rgb(ram)),
            curve_bias: matches!(class, StateClass::WorldMap)
                .then(|| retail_curve_bias(ram))
                .flatten(),
            page_mark: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| retail_page_mark(ram))
                .flatten(),
            // Only while the player still faces the way the entry stood it:
            // after a pad turn the heading is walk history the seat does
            // not replay.
            player_facing: game_anchors::player_ptr(ram)
                .map(|p| rd16(ram, p + 0x26))
                .filter(|&h| h == rd16(ram, ARRIVAL_FACING)),
            player_heading: game_anchors::player_ptr(ram).map(|p| rd16(ram, p + 0x26)),
            arrival_facing: rd16(ram, ARRIVAL_FACING),
            actor_facings: if matches!(class, StateClass::Field) {
                retail_actor_facings(ram)
            } else {
                Vec::new()
            },
            slot_table: {
                let lo = (SLOT_TABLE_VA & 0x1F_FFFF) as usize;
                ram.get(lo..lo + 0x200)
                    .map(|b| std::array::from_fn(|i| i16::from_le_bytes([b[i * 2], b[i * 2 + 1]])))
            },
            rand_seed: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| rd32(ram, legaia_engine_system::rand::BIOS_RAND_SEED_ADDR) as u32),
        }
    }
}

/// The field VM's script-counter slot table
/// (`legaia_engine_core::world::FieldVmState::slot_table`).
pub(super) const SLOT_TABLE_VA: u32 = 0x801C_6460;

/// Re-assert the capture's script timers before a seeding tick.
///
/// The slot table is RAM outside the save window, so the card-load seed
/// cannot restore it, and the settle window is a warm-up, not elapsed game
/// time: a timer left to run would fire inside the window an arm retail is
/// still counting towards (a door-fade handshake 36 frames into its 46, a
/// spawn delay 24 into its 50) and score that as an engine miss. Holding the
/// captured counts keeps every timer where retail's was.
/// Re-assert the capture's `rand()` stream position before a seeding tick,
/// for the same reason as [`hold_slot_table`]: a field script that rolls
/// every frame (`map03`'s cart clock, `4E 00 41 01 ..` once its counter
/// holds `240`) would otherwise roll the warm-up's ticks as game time. Held,
/// each tick draws the value retail's next draw would.
pub(crate) fn hold_rand_seed(session: &mut NativeSession, retail: &RetailObs) {
    if let Some(seed) = retail.rand_seed {
        session.host.world.rng_state = seed;
    }
}

pub(crate) fn hold_slot_table(session: &mut NativeSession, retail: &RetailObs) {
    if let Some(slots) = retail.slot_table {
        session.host.world.field_vm.slot_table = slots;
    }
}

impl RetailObs {
    /// The save the seed lands: the lifted save with the gate record's own
    /// run latches cleared ([`legaia_engine_capture::script_gate::run_latches`]).
    ///
    /// Retail's record set those flags after the scene entry ran, so the
    /// entry never saw them; a card load runs the entry over the whole save
    /// and an entry that tests one takes the arm retail did not.
    /// `minigame_dance_pcsx` is caught on `koin3` `P2[6]` two ops past
    /// `55 9C`, the flag the entry (`P1[0]` `+0x178`) reads as "back from
    /// the dance floor": seeded with it up, the entry cleared it and spawned
    /// the judging record `P2[9]` over the frame. The comparand is
    /// [`Self::save`] unchanged.
    ///
    /// The seed raises them again at the settle tick
    /// ([`Self::seed_latches`]), once the entry has run: retail's state holds
    /// them, and a record the seed never reaches would otherwise leave the
    /// flags channel short. Raised straight after the landing, an entry still
    /// running read them anyway - the slot-machine floor's entry restarted
    /// its track on one.
    pub fn seed_save(&self) -> Option<legaia_save::SaveFile> {
        let mut save = self.save.clone()?;
        clear_system_flags(&mut save, &self.seed_latches());
        Some(save)
    }

    /// Read the field scrollers' captured rects ([`retail_scroll_rects`]).
    ///
    /// A battle state takes its live scrollers alone: a battle effect's
    /// mode-4 part rolls its rect on the same part tick, and its rotation
    /// count is time since the effect was seated - for the Mystic Shield's
    /// page scroller (`0x80078720`), an earlier turn the replay never runs.
    pub(super) fn seat_scroll_rects(&mut self, ram: &[u8], vram: Option<&[u8]>) {
        if let Some(v) = vram
            && self.class == StateClass::Battle
        {
            self.scroll_rects = retail_scroll_rects(ram, v);
            return;
        }
        if let Some(v) = vram
            && matches!(self.class, StateClass::Field | StateClass::WorldMap)
        {
            self.scroll_rects = retail_scroll_rects(ram, v);
            self.panel = retail_panel(ram);
            if let Some(p) = self.panel {
                self.scroll_rects.extend(panel_source_rects(&p, v));
            }
            // The cycled CLUT cells as the frame after the displayed one
            // uploaded them, written after the engine's own cycler output.
            self.scroll_rects
                .extend(retail_older_buffer_cell_rects(ram));
        }
    }

    /// The latches [`Self::seed_save`] holds back from the scene entry.
    pub fn seed_latches(&self) -> Vec<u16> {
        if self.menu.is_some() || !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            return Vec::new();
        }
        self.scripts
            .running
            .first()
            .map(|s| s.latches.clone())
            .unwrap_or_default()
    }

    /// The capture-alignment environment the image child takes on top of
    /// its seat: held-back latches, CLUT-cell phases, fog-pool records and
    /// scroller rects (the last through a file beside the child's frame).
    pub fn seat_env(&self, out_dir: Option<&Path>, label: &str) -> Vec<(&'static str, String)> {
        let mut env: Vec<(&'static str, String)> = self.seed_latches_env().into_iter().collect();
        if !self.cell_fx.is_empty() {
            env.push(("LEGAIA_SEAT_CLUT_FX", cell_fx_env(&self.cell_fx)));
        }
        if !self.fog.is_empty() {
            env.push(("LEGAIA_SEAT_FOG", fog_env(&self.fog)));
        }
        if !self.sprite_arms.is_empty() {
            env.push((
                "LEGAIA_SEAT_SPRITE_ARMS",
                sprite_arms_env(&self.sprite_arms),
            ));
        }
        if !self.object_models.is_empty() {
            env.push((
                "LEGAIA_SEAT_OBJECT_MODELS",
                self.object_models
                    .iter()
                    .map(|(r, m)| format!("{r}:{m}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if !self.object_clips.is_empty() {
            env.push((
                "LEGAIA_SEAT_OBJECT_CLIPS",
                object_clips_env(&self.object_clips),
            ));
        }
        if !self.morphs.is_empty() {
            env.push(("LEGAIA_SEAT_MORPHS", morphs_env(&self.morphs)));
        }
        if !self.walkers.is_empty() {
            env.push(("LEGAIA_SEAT_WALKERS", walkers_env(&self.walkers)));
        }
        if let Some([r, g, b]) = self.clear_rgb {
            env.push(("LEGAIA_SEAT_CLEAR", format!("{r},{g},{b}")));
        }
        if let Some([x, y, z]) = self.seat_eye {
            env.push(("LEGAIA_SEAT_EYE", format!("{x},{y},{z}")));
        }
        if let Some([p, y, h]) = self.seat_angles {
            env.push(("LEGAIA_SEAT_ANGLES", format!("{p},{y},{h}")));
        }
        if let Some(bias) = self.curve_bias.filter(|&b| b != 0) {
            env.push(("LEGAIA_SEAT_CURVE_BIAS", bias.to_string()));
        }
        env.push(("LEGAIA_SEAT_FRAME_STEP", self.frame_step().to_string()));
        if let Some(f) = self.page_mark {
            env.push(("LEGAIA_SEAT_PAGE_MARK", f.to_string()));
        }
        if let Some(p) = self.panel {
            env.push(("LEGAIA_SEAT_PANEL", panel_env(&p)));
        }
        // The frame child always stands in retail's heading: it seats no
        // arrival facing of its own, and the image channel scores no heading.
        if let Some(h) = self.player_heading {
            env.push(("LEGAIA_SEAT_HEADING", (h & 0x0FFF).to_string()));
        }
        if let Some(kv) = self.vram_rects_env(out_dir, label) {
            env.push(kv);
        }
        env
    }

    /// The captured scroller rects as a `LEGAIA_SEAT_VRAM_RECTS` file under
    /// the image work directory, and the variable naming it.
    pub fn vram_rects_env(
        &self,
        out_dir: Option<&Path>,
        label: &str,
    ) -> Option<(&'static str, String)> {
        if self.scroll_rects.is_empty() {
            return None;
        }
        let dir = crate::retail_compare_image::work_dir(out_dir);
        let path = dir.join(format!("{label}.vrect.bin"));
        (std::fs::create_dir_all(&dir).is_ok()
            && std::fs::write(&path, vram_rects_file(&self.scroll_rects)).is_ok())
        .then(|| ("LEGAIA_SEAT_VRAM_RECTS", path.display().to_string()))
    }

    /// The frame step retail was running at the capture instant, in vsyncs a
    /// game frame ([`crate::retail_compare_battle::frame_step`], half the
    /// display lag). It is load history the engine's fixed tick does not
    /// measure, so the seed hands it over (`World::seed_frame_step`).
    pub fn frame_step(&self) -> u8 {
        (self.scripts.display_lag / 2).clamp(1, 4) as u8
    }

    /// The player heading the seed stands the player in: retail's `+0x26`
    /// when the pad has turned it off the arrival facing, `None` while it
    /// still holds the arrival facing (the engine's own entry gives that).
    pub fn seat_heading(&self) -> Option<i16> {
        self.player_heading.filter(|&h| h != self.arrival_facing)
    }

    /// [`Self::seed_latches`] as `LEGAIA_SEAT_LATCHES` for `play-window`
    /// (hex flag ids, comma-separated), when there are any.
    pub fn seed_latches_env(&self) -> Option<(&'static str, String)> {
        let l = self.seed_latches();
        (!l.is_empty()).then(|| {
            (
                "LEGAIA_SEAT_LATCHES",
                l.iter()
                    .map(|i| format!("{i:x}"))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
    }

    /// Name the state by the scene it is **running**, not the one a door has
    /// queued. A walked crossing writes the destination label to
    /// `0x8007050C` with the scene-change packet, frames before the field
    /// init loads the block and stores its define to `0x80084540`; a
    /// field-run capture in that window shows the outgoing scene (its frame,
    /// its camera, its player, its track) under the incoming label. Scored
    /// under the label, every one of those channels compares the outgoing
    /// scene's retail values against a fresh entry of the incoming one.
    ///
    /// Only field-run states are re-named: a mode-`0x02` capture is the load
    /// itself, and the title / battle / menu modes hold other words there.
    pub fn settle_on_loaded_scene(&mut self, cdname: &legaia_prot::cdname::IndexMap) {
        if !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            return;
        }
        let label_define = cdname
            .iter()
            .find(|(_, name)| **name == self.scene)
            .map(|(&define, _)| define);
        let Some(loaded) = cdname.get(&u32::from(self.loaded_define)) else {
            return;
        };
        if label_define == Some(u32::from(self.loaded_define)) || *loaded == self.scene {
            return;
        }
        let pending = std::mem::replace(&mut self.scene, loaded.clone());
        self.pending_scene = Some(pending);
        self.class = StateClass::classify(self.game_mode, &self.scene, self.player.is_some());
        if !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            self.hud_countdown = None;
            self.camera_block = None;
            self.seat_focus = None;
            self.seat_eye = None;
            self.seat_angles = None;
        }
    }
}

impl RetailObs {
    /// Stand a battle capture that holds no position snapshot on the region
    /// its fight was entered from.
    ///
    /// The mode-change prologue `FUN_80016230` stores the snapshot only when
    /// the field run (mode `3`) is the mode handing over (`lh v1,0x494(gp)`
    /// / `bne v1,v0` at `0x800163E0..0x800163E8`), so a fight started another
    /// way leaves `(0, 0)`, and a card load of that window seats the scene's
    /// default arrival. The entry script then reads the default tile's
    /// region: `town01`'s loop selects `0x19D` there, where the capture holds
    /// `0x19F` - the type-4 arm, the east half of the village.
    ///
    /// What the capture does keep is the region-type mask
    /// ([`Self::region_mask`]). The seat taken is the tile nearest the entry
    /// operand whose `.MAP` regions rebuild that mask
    /// (`field_regions::region_type_mask`): the region the player stood in,
    /// not the spot - nothing in the state records that.
    ///
    /// Lands the save once to read the scene's region table. Returns whether
    /// it seated.
    pub fn seat_battle_on_region(&mut self, extracted: &Path) -> bool {
        use legaia_engine_field::field_regions::{RegionTable, region_type_mask};
        let Some(save) = self.save.clone() else {
            return false;
        };
        if self.class != StateClass::Battle
            || save.ext_v2.field_position.is_some()
            || self.region_mask == 0
        {
            return false;
        }
        let cfg = legaia_engine_session::session::BootConfig {
            scene: self.scene.clone(),
            enable_audio: false,
        };
        let Ok(mut session) = legaia_engine_session::native::NativeSession::open(extracted, &cfg)
        else {
            return false;
        };
        super::pin_seed_retail(&mut session.host.world.toggles, false);
        let opts = legaia_engine_session::session::FieldLiveOpts {
            live_loop: true,
            player_battle: true,
            ..Default::default()
        };
        let _landing = session.resume_save(save, &self.scene, &opts);
        let block = &session.host.world.terrain.map_region_block;
        let Some(table) = RegionTable::parse(block) else {
            return false;
        };
        let tile = |w: i16| (i32::from(w) - 0x40) >> 7;
        let (ox, oz) = (tile(self.entry_operand.0), tile(self.entry_operand.1));
        let seat = (0..0x80)
            .flat_map(|tx| (0..0x80).map(move |tz| (tx, tz)))
            .filter(|&(tx, tz)| region_type_mask(Some(&table), tx, tz) == self.region_mask)
            .min_by_key(|&(tx, tz)| ((tx - ox).pow(2) + (tz - oz).pow(2), tx, tz));
        let Some((tx, tz)) = seat else {
            return false;
        };
        let centre = |t: i32| ((t << 7) + 0x40) as i16;
        if let Some(save) = self.save.as_mut() {
            save.ext_v2.field_position = Some((centre(tx), centre(tz)));
        }
        true
    }

    /// Read the visible-tile window off a state's 1 KiB scratchpad image
    /// (index `0` = `0x1F800000`). Field states only: the overworld draws
    /// through its own terrain pass and a battle leaves the bytes stale.
    pub fn seat_view_window(&mut self, scratchpad: Option<&[u8]>) {
        self.view_window = (self.class == StateClass::Field)
            .then(|| scratchpad?.get(0x3E8..0x3EC))
            .flatten()
            .map(|b| [b[0] as i8, b[1] as i8, b[2] as i8, b[3] as i8]);
    }
}

/// Read a library state of either emulator. `scus` is `SCUS_942.54`, which a
/// PCSX-Redux state needs for its RAM anchor search.
pub fn read_retail(entry: &CorpusEntry, scus: &[u8]) -> Result<RetailObs> {
    match entry.emulator {
        "mednafen" => {
            let st = MednafenState::from_path(&entry.path)
                .with_context(|| format!("parse {}", entry.path.display()))?;
            let ram = st.main_ram()?;
            let gpu = PsxGpu::new(&st);
            let frame = match (gpu.vram_bytes(), gpu.regs().display_crop_rect()) {
                (Some(v), Some(rect)) => Frame::from_vram_display(v, rect),
                _ => None,
            };
            let mut obs = RetailObs::from_ram(ram, frame);
            obs.seat_scroll_rects(ram, gpu.vram_bytes());
            obs.seat_view_window(st.scratch_ram().ok());
            Ok(obs)
        }
        _ => {
            let (st, gpu) = legaia_pcsx_redux::gpu::load_with_scus(&entry.path, scus)?;
            let frame = gpu
                .as_ref()
                .and_then(|g| Frame::from_vram_display(&g.vram, g.display_crop_rect()));
            let mut obs = RetailObs::from_ram(st.main_ram(), frame);
            obs.seat_scroll_rects(st.main_ram(), gpu.as_ref().map(|g| g.vram.as_slice()));
            obs.seat_view_window(st.scratchpad());
            Ok(obs)
        }
    }
}
