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

/// The armed entry context (`_DAT_8007B450`), which the walk tick points at
/// the field overlay's Incense notice record when the window runs out.
const ARMED_RECORD: u32 = 0x8007_B450;
/// The field overlay's Incense wear-off record (kind `0x0B`, `0x801F2278`).
const INCENSE_NOTICE_RECORD: u32 = 0x801F_2278;

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
    /// The follow camera's staging pose (`0x801F3580`) with the live focus,
    /// where the ease `FUN_801DB510` is heading, on a field or overworld
    /// state. The ease runs only on a frame the player moved, so a capture
    /// can hold the live pose anywhere short of it. The `camera` channel takes
    /// whichever of the two poses the engine stands nearer: the seat composes
    /// the target and has no walk to ease over, and a script's shot leaves
    /// this stale, with the engine on the live pose instead.
    pub camera_staging: Option<CameraObs>,
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
    /// The field menu lock, bit `0x0800_0000` of the scratchpad story word
    /// `_DAT_1F800394` ([`legaia_engine_core::world::FIELD_MENU_LOCK_BIT`]),
    /// on a field state. A record raises it with `2E 1B` and the word is not
    /// in the save, so a card load past the record comes up without it; the
    /// field party HUD `FUN_801D0D38` bails on it (`0x801D0D98..0x801D0DAC`).
    /// Set by [`RetailObs::seat_menu_lock`].
    pub menu_lock: bool,
    /// The packets and fog sheets in the table the state's frame is building
    /// ([`retail_frame_table`]); `None` without a scratchpad image.
    pub frame_table: Option<(usize, usize)>,
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
    /// walkable state, or a menu over one, whose focus is not on the player
    /// (the Retock shop captures poke the player beside the merchant, so the
    /// focus stays where the door left it): history the seat
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
    /// The player `(X, Z)` the image child stands on, when it is not the
    /// RAM's: a capture taken while the pad walked the player shows a frame
    /// two passes old, so the child stands one walk step behind the
    /// previous-pass pair and lands the follow camera two ease steps back
    /// (`seat_eye` / `seat_angles` carry the rewound pose,
    /// [`super::walk_rewind`]).
    pub image_seat: Option<[i16; 2]>,
    /// A walking capture's last two passes, which
    /// [`Self::seat_walk_rewind`] turns into the rewound seat.
    pub walk: Option<super::walk_rewind::WalkHistory>,
    /// The walk-region box (scratchpad `0x1F800384..87`) the rewind composed
    /// against, kept for [`Self::reseat_walk_rewind_on_floors`].
    pub walk_attr_box: Option<[u8; 4]>,
    /// The minigame a minigame-class state runs ([`super::minigame_run`]).
    pub minigame: Option<super::RetailMinigame>,
    /// The displayed frame, when the state carries VRAM + display registers.
    pub frame: Option<Frame>,
    /// A movie-class state's fmv id and stream frame word.
    pub movie: Option<super::movie_run::RetailMovie>,
    /// Battle observables for a battle-class state; `Err` names why the
    /// state cannot seed a battle.
    pub battle: Option<std::result::Result<crate::retail_compare_battle::RetailBattle, String>>,
    /// The pause-menu screen a menu-class state shows; `Err` names why the
    /// state is not one the seed can drive to.
    pub menu: Option<std::result::Result<RetailMenu, String>>,
    /// A menu-class state's raw sub-screen word `DAT_801E46A4`, read whether
    /// or not the seed drives to that screen (the capture-coverage census,
    /// [`crate::retail_coverage`]).
    pub menu_word: Option<u8>,
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
    /// The op-`0x35` sub-op `5` timed release as the state holds it
    /// (`FUN_800267A8`'s cells): armed (`gp+0x808`), deadline (`gp+0x814`)
    /// and elapsed vsyncs (`gp+0x81C`), and whether the field-BGM source's
    /// sequence plays (`0x8007052C` `+0xA`, its libsnd record's `+0x98`
    /// bit 0). Field and overworld classes only. [`seat_timed_release`]
    /// stands the engine's where retail's is when the script gate meets, as
    /// the glide in flight is.
    pub timed_release: Option<(bool, i32, i32, bool)>,
    /// The side-band request the state was captured under: the pair
    /// `_DAT_8007BABC` / `_DAT_8007BAA0` when it is settled on a bank
    /// (field and overworld classes). A record's request is history a card
    /// load does not replay: `conc3`'s entry requests `2116`, the inn's
    /// `P2[9]` `2070` (`36 01 80 16 08` at `+0xCD`), and the ceremony
    /// `P2[10]` keys its rows out of `2070`'s bank until its own `2116` at
    /// its end. Seated at the resume ([`seat_side_band`]).
    pub side_band: Option<i16>,
    /// The live mode-3 CLUT-cell cyclers ([`retail_cell_fx`]), handed to the
    /// image child as `LEGAIA_SEAT_CLUT_FX` so the cycled palettes show the
    /// captured phase.
    pub cell_fx: Vec<legaia_engine_field::clut_cell_fx::ClutCellFx>,
    /// The live fog-pool records ([`retail_fog`]), handed to the image child
    /// as `LEGAIA_SEAT_FOG` and installed on the frame it captures.
    pub fog: Vec<legaia_engine_field::fog_particles::FogParticle>,
    /// The same records with no step back to the displayed frame
    /// ([`retail_fog_live`]): what the walk film resumes from.
    pub fog_live: Vec<legaia_engine_field::fog_particles::FogParticle>,
    /// `_DAT_8007BCA8`, the live count the last render step wrote and the
    /// next pass's spawner tests against the cap. A state saved between a
    /// pass's spawns and its render step holds more alive records than this:
    /// the spawns are in, and the records the step is about to free are
    /// still up.
    pub fog_live_count: u16,
    /// `_DAT_8007BCAC`, the camera's eased vertical offset, which the sheets
    /// stand against (`FUN_8003F3FC` at `0x8003F448`): a clock of the steps the
    /// player walked, handed over as `LEGAIA_SEAT_FOG_OFFSET`.
    pub fog_offset: i32,
    /// The region roll's step counter `_DAT_8007B5FC` (`FUN_801D9E1C`): the
    /// steps left before the next random fight, walk history the card load
    /// does not carry. The walk film seats it, so a walk rolls where retail's
    /// rolled (`karisto_sol_pre_encounter` holds `14`).
    pub encounter_step_counter: i32,
    /// Vsyncs since the frame pacer's last `VSync(step)` returned: libetc's
    /// vblank counter `0x8007A894` less the count `FUN_8005FB84` stored on
    /// return (`0x800797D4`, `0x8005FC8C..0x8005FCA0`). The game frame opens
    /// on that return, so this says which vsync of retail's frame the state
    /// was saved on - the phase a walk film resumes on.
    pub vsync_phase: u32,
    /// The scene-change actor's countdown (`gp+0x710`, `0x8007BA28`) when a
    /// state was saved mid-departure: the actor's state 0 writes the screen
    /// byte `0x8007B648 = 0x80`, which reads `0x80` until mode `0x02`.
    /// `None` on a state with no departure in flight.
    pub departure_countdown: Option<i32>,
    /// The scene-change actor's own state (`actor+0x1A` of the live actor
    /// whose handler `+0x0C` is `FUN_80021934`), beside
    /// [`Self::departure_countdown`]: `3` while it waits on the CD queue for
    /// its chunk, `4` once it only waits on the countdown.
    pub departure_state: Option<u16>,
    /// The buttons libpad's port-1 buffer (`0x800840F8`, refreshed at every
    /// vblank) held at the save, in the host pad's layout (the buffer's
    /// active-low halfword at `+2`, inverted): what the frame pacer's pad
    /// build (`FUN_8001822C`) latched into the held word for the next frame.
    pub pad_buffer: u16,
    /// Vsyncs left on a field CD-XA one-shot's read at the save: `0` with the
    /// drive word `0x8007BC20` clear, else the sectors from the head the
    /// stop watch last polled (`gp+0x970`, stored at `0x8003D9DC`) to the
    /// clip's end (`gp+0x974`) at the drive's double speed, plus the vsync
    /// the stop `FUN_8003EE00` takes to clear the word. `None` for a seek's
    /// `1` ([`Self::xa_seek_parked`]).
    pub xa_drive_left: Option<u16>,
    /// The drive word reads `1`: a seek-ahead parked the head and no clip
    /// has started since.
    pub xa_seek_parked: bool,
    /// The frame pacer's history at the capture: the sixteen frame times of
    /// `DAT_80084098` and the write cursor `gp+0x440`
    /// ([`crate::retail_compare_battle::frame_cost_ring`]). `None` where the
    /// history does not set the step.
    pub frame_history: Option<([i16; 16], usize)>,
    /// The pause menu's Field Move word `0x800846CC` (Walk `0` / Run `1`):
    /// the run default the walk film stands the engine on.
    pub field_move_run: bool,
    /// The pacer's step bytes off the scratchpad, `(0x1F800392, 0x1F800393)`:
    /// the wait of the frame the state holds and the step resolved at its
    /// end. `None` without a scratchpad image. A history entry reading
    /// `0x2BC` is a clamped long frame or a raw `700`, and only the step
    /// byte says which ([`Self::frame_cost_seat`]).
    pub step_bytes: Option<(u8, u8)>,
    /// The live draw-kind-4 parts, sprite-arm sheets and others
    /// ([`retail_sprite_arms`]),
    /// handed to the image child as `LEGAIA_SEAT_SPRITE_ARMS` and installed on
    /// the frame it captures.
    pub sprite_arms: Vec<legaia_engine_core::world::ambient::SpriteArmSeed>,
    /// The live mesh-bearing ambient parts ([`retail_mesh_parts`]), handed
    /// to the image child as `LEGAIA_SEAT_MESH_PARTS` and installed on the
    /// frame it captures.
    pub mesh_parts: Vec<legaia_engine_core::world::ambient::SpriteArmSeed>,
    /// The live ambient parts that turn ([`retail_turning_parts`]), their
    /// render banks as the displayed frame drew them, handed to the image
    /// child as `LEGAIA_SEAT_TURNING_PARTS`.
    pub turning_parts: Vec<legaia_engine_core::world::ambient::AmbientBankSeed>,
    /// The live ambient parts' move-VM program words ([`retail_part_vm`]),
    /// which the walk film seats so the parts' waits run in retail's phase.
    pub part_vm: Vec<legaia_engine_core::world::ambient::AmbientVmSeed>,
    /// The text balloon the displayed frame drew ([`retail_balloon`]) - the
    /// place-name banner a door leaves up - for a field state, handed to the
    /// image child as `LEGAIA_SEAT_BALLOON` (`none` when it drew none).
    pub balloon: Option<Option<legaia_engine_dialog::text_balloon::TextBalloon>>,
    /// The Incense wear-off notice is up: the field overlay's notice record
    /// ([`INCENSE_NOTICE_RECORD`]) is the armed entry context
    /// `_DAT_8007B450`. Handed to the image child as
    /// `LEGAIA_SEAT_INCENSE_NOTICE`, which raises it.
    pub incense_notice: bool,
    /// The live mode-4 scroller rects and their captured texels
    /// ([`retail_scroll_rects`]), handed to the image child as a
    /// `LEGAIA_SEAT_VRAM_RECTS` file.
    pub scroll_rects: Vec<SeededVramRect>,
    /// The dance hall's captured palettes ([`retail_cell_sources`]), handed to
    /// the image child as a `LEGAIA_SEAT_CELL_SOURCES` file.
    pub cell_sources: Vec<SeededVramRect>,
    /// The source column of the effect palette row `(224, 476)` a battle
    /// state holds ([`retail_cue_clut_source`]): the swap an earlier action's
    /// cue group made, handed to the image child as `LEGAIA_SEAT_CUE_CLUT`.
    pub cue_clut_source: Option<u8>,
    /// Drawn field actors' live model ids ([`retail_object_models`]), handed
    /// to the image child as `LEGAIA_SEAT_OBJECT_MODELS` for the placed
    /// objects a motion stream re-binds.
    pub object_models: Vec<(u16, i16)>,
    /// Drawn field actors' semi-transparency flag bytes
    /// ([`retail_object_draw_flags`]), handed to the image child as
    /// `LEGAIA_SEAT_OBJECT_FLAGS`.
    pub object_flags: Vec<(u16, u8)>,
    /// Drawn field actors' clip words ([`retail_object_clips`]), handed to
    /// the image child as `LEGAIA_SEAT_OBJECT_CLIPS`.
    pub object_clips: Vec<ObjectClipSeed>,
    /// The same clip words as the RAM holds them
    /// ([`retail_object_clips_ram`]): the walk film's seat, which runs on
    /// from the RAM's pass rather than drawing the displayed one.
    pub object_clips_ram: Vec<ObjectClipSeed>,
    /// Each bind record's collision-exempt bits `+0x10 & 3`
    /// ([`retail_object_exempt_bits`]): the walk film's seat of a door the
    /// state holds open.
    pub object_exempt: Vec<(u16, u32)>,
    /// The player's clip words ([`retail_player_clip`]), handed to the image
    /// child as `LEGAIA_SEAT_PLAYER_CLIP`.
    pub player_clip: Option<ObjectClipSeed>,
    /// The player's clip words as the RAM holds them
    /// ([`retail_player_clip_ram`]): the walk film's seat.
    pub player_clip_ram: Option<ObjectClipSeed>,
    /// Field actors' live VDF morph envelopes ([`retail_morphs`]), handed to
    /// the image child as `LEGAIA_SEAT_MORPHS`.
    pub morphs: Vec<MorphSeed>,
    /// The `4C D8` morph-weight actors' envelopes ([`retail_morph_weights`]),
    /// handed to the image child as `LEGAIA_SEAT_MORPH_WEIGHTS`.
    pub morph_weights: Vec<legaia_engine_capture::field_seeds::MorphWeightSeed>,
    /// The floor-ladder oscillators' live words ([`retail_floor_bobs`]). The
    /// headless sample writes them as they stand, the image child takes them
    /// back by the display lag.
    pub floor_bobs: Vec<legaia_engine_capture::field_seeds::FloorBobSeed>,
    /// Ambient walkers' live seats ([`retail_walkers`]), handed to the image
    /// child as `LEGAIA_SEAT_WALKERS`.
    pub walkers: Vec<WalkerSeed>,
    /// Every ambient stream's live state in actor-list order
    /// ([`retail_ambient_streams`]): what a run resuming from the state, so
    /// its `rand()` stream can match retail's, seats after the settle.
    pub ambient_streams: Vec<RetailAmbientStream>,
    /// The placement a talk holds engaged and the pre-talk heading the touch
    /// saved on it ([`retail_talk_facing_save`]), which the walk film seats
    /// so the talk's end restores it.
    pub talk_facing_save: Option<(u16, u16)>,
    /// Placements a finished partition-2 beat left on a script-literal tile
    /// ([`scripted_seats`]): set by the corpus run, which holds the scene's
    /// MAN; seated, position only, by the headless seed and by the image
    /// child (`LEGAIA_SEAT_PLACEMENTS`).
    pub scripted_seats: Vec<WalkerSeed>,
    /// Placements a finished beat turned to a compass literal on a seat the
    /// card load reproduces ([`scripted_heading_seats`]): set by the corpus
    /// run, turned by the headless seed and by the image child
    /// (`LEGAIA_SEAT_HEADINGS`) where the engine stands them on retail's seat.
    pub scripted_headings: Vec<WalkerSeed>,
    /// The scenario's `kept_bgm`: the track a door kept sounding into the
    /// scene under its own word, which the seed starts before the card-load
    /// entry with the keep raised (`seed_and_settle`).
    pub kept_bgm: Option<u16>,
    /// The live image-panel widget ([`retail_panel`]), handed to the image
    /// child as `LEGAIA_SEAT_PANEL`; the texels it shows ride the
    /// `LEGAIA_SEAT_VRAM_RECTS` file beside the scroller rects.
    pub panel: Option<legaia_engine_effects::screen_fx::PanelWidget>,
    /// The scene's live scripted grade ([`retail_scene_grade`]), handed to
    /// the image child as `LEGAIA_SEAT_GRADE`, its clock taken back by the
    /// display lag.
    pub grade: Option<legaia_engine_field::actor_colour_tween::ColourTween>,
    /// The live attached lights ([`retail_attached_lights`]), handed to the
    /// image child as `LEGAIA_SEAT_LIGHTS`, their keyframe clocks taken back
    /// by the display lag.
    pub lights: Vec<legaia_engine_core::world::AttachedLightSeed>,
    /// Whether the state holds a live screen-effect sprite widget
    /// ([`retail_has_sprite_widget`], the ending crawl's lines): the image
    /// child runs those on to the record's RAM wait (`LEGAIA_SEAT_SPRITE_WAIT`).
    pub sprite_widgets: bool,
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
    /// The save-allow word `_DAT_8007B6A8` is raised: the scene is a kingdom
    /// overworld, or a capture probe poked it (the scenario's `ram_poked`
    /// names that). The seed lands the same bit, so the pause menu's Save
    /// row opens as it did in retail.
    pub save_allowed: bool,
    /// The memory-card image the capture ran with (the scenario's
    /// `memory_card`), which a card-screen seed mounts in port 1. Set by the
    /// corpus run, not read from RAM.
    pub memory_card: Option<PathBuf>,
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
    /// `+0x16`, the footing or a script-held height.
    pub y: i16,
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
    /// The stream's first heading op, for the `LEGAIA_RC_FACING_DUMP`
    /// diagnostic (a wander `0x18` is `rand()` history, a ramp `0x04` time).
    pub ambient_op: Option<u8>,
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
        let mut obs = Self {
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
            menu_lock: false,
            frame_table: None,
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
                (StateClass::Field | StateClass::WorldMap | StateClass::Menu, Some([x, _, z]))
                    if camera.focus != [-i32::from(x), -i32::from(z)] =>
                {
                    Some(camera.focus)
                }
                _ => None,
            },
            xa_seek_parked: game_anchors::u32_at(ram, 0x8007_BC20) == 1,
            camera_staging: (matches!(class, StateClass::Field | StateClass::WorldMap)
                && player.is_some())
            .then(|| super::walk_rewind::follow_staging_pose(ram, &camera)),
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
            image_seat: None,
            walk: None,
            walk_attr_box: None,
            minigame: (class == StateClass::Minigame).then(|| super::RetailMinigame::from_ram(ram)),
            frame,
            battle,
            menu,
            menu_word: (class == StateClass::Menu)
                .then(|| game_anchors::u8_at(ram, MENU_SUBSCREEN)),
            save_allowed: game_anchors::u32_at(ram, SAVE_ALLOW) != 0,
            memory_card: None,
            scripts: crate::retail_compare_script::RetailScripts::from_ram(ram),
            cell_fx: if matches!(
                class,
                StateClass::Field
                    | StateClass::WorldMap
                    | StateClass::Battle
                    | StateClass::Minigame
            ) {
                retail_cell_fx(ram)
            } else {
                Vec::new()
            },
            fog: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_fog(ram)
            } else {
                Vec::new()
            },
            fog_live: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_fog_live(ram)
            } else {
                Vec::new()
            },
            fog_live_count: game_anchors::u16_at(ram, 0x8007_BCA8),
            fog_offset: game_anchors::u32_at(ram, 0x8007_BCAC) as i32,
            encounter_step_counter: game_anchors::u32_at(ram, 0x8007_B5FC) as i32,
            vsync_phase: game_anchors::u32_at(ram, 0x8007_A894)
                .wrapping_sub(game_anchors::u32_at(ram, 0x8007_97D4)),
            departure_countdown: (game_anchors::u8_at(ram, 0x8007_B648) == 0x80)
                .then(|| game_anchors::u32_at(ram, 0x8007_BA28) as i32)
                .filter(|c| (-0x20..=0x46).contains(c)),
            pad_buffer: !game_anchors::u16_at(ram, 0x8008_40FA),
            xa_drive_left: match game_anchors::u32_at(ram, 0x8007_BC20) {
                0 => Some(0),
                2 => {
                    let cur = game_anchors::u32_at(ram, 0x8007_BC88) as i32;
                    let end = game_anchors::u32_at(ram, 0x8007_BC8C) as i32;
                    Some((((end - cur).max(0) * 2 / 5) + 1).min(0x7FFF) as u16)
                }
                _ => None,
            },
            departure_state: crate::retail_compare_script::actor_nodes(ram)
                .into_iter()
                .filter(|&n| game_anchors::u32_at(ram, n + 0x0C) == 0x8002_1934)
                .map(|n| game_anchors::u16_at(ram, n + 0x1A))
                .max(),
            frame_history: crate::retail_compare_battle::frame_cost_ring(ram)
                .map(|(e, c)| (e.map(|v| v.min(0x7FFF) as i16), usize::from(c))),
            step_bytes: None,
            field_move_run: game_anchors::u32_at(ram, 0x8008_46CC) & 1 != 0,
            sprite_arms: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_sprite_arms(ram, crate::retail_compare_battle::display_lag_vsyncs(ram))
            } else {
                Vec::new()
            },
            mesh_parts: if class == StateClass::Field {
                retail_mesh_parts(ram, crate::retail_compare_battle::display_lag_vsyncs(ram))
            } else {
                Vec::new()
            },
            balloon: (class == StateClass::Field)
                .then(|| retail_balloon(ram))
                .flatten(),
            incense_notice: class == StateClass::Field
                && game_anchors::u32_at(ram, ARMED_RECORD) == INCENSE_NOTICE_RECORD,
            turning_parts: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_turning_parts(ram, crate::retail_compare_battle::display_lag_vsyncs(ram))
            } else {
                Vec::new()
            },
            part_vm: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_part_vm(ram)
            } else {
                Vec::new()
            },
            scroll_rects: Vec::new(),
            cell_sources: if class == StateClass::Minigame {
                retail_cell_sources(ram)
            } else {
                Vec::new()
            },
            cue_clut_source: None,
            panel: None,
            sprite_widgets: false,
            grade: matches!(class, StateClass::Field)
                .then(|| retail_scene_grade(ram))
                .flatten(),
            lights: if matches!(class, StateClass::Field) {
                retail_attached_lights(ram)
            } else {
                Vec::new()
            },
            scripted_seats: Vec::new(),
            scripted_headings: Vec::new(),
            kept_bgm: None,
            movie: (class == StateClass::Cutscene)
                .then(|| super::movie_run::RetailMovie::from_ram(ram)),
            walkers: if matches!(class, StateClass::Field) {
                retail_walkers(ram)
            } else {
                Vec::new()
            },
            ambient_streams: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_ambient_streams(ram)
            } else {
                Vec::new()
            },
            talk_facing_save: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_talk_facing_save(ram)
            } else {
                None
            },
            morphs: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_morphs(ram)
            } else {
                Vec::new()
            },
            morph_weights: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_morph_weights(ram)
            } else {
                Vec::new()
            },
            floor_bobs: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_floor_bobs(ram)
            } else {
                Vec::new()
            },
            object_models: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_object_models(ram)
            } else {
                Vec::new()
            },
            object_flags: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_object_draw_flags(ram)
            } else {
                Vec::new()
            },
            player_clip: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_player_clip(ram)
            } else {
                None
            },
            player_clip_ram: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_player_clip_ram(ram)
            } else {
                None
            },
            object_clips: if matches!(class, StateClass::Field) {
                retail_object_clips(ram)
            } else {
                Vec::new()
            },
            // A kingdom map's placements tick through the same driver
            // (`FUN_8003BC08`), so their clips seat the same way (`map01`'s
            // parked flats `15..=17` hold cursor `132` in the state).
            object_clips_ram: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_object_clips_ram(ram)
            } else {
                Vec::new()
            },
            object_exempt: if matches!(class, StateClass::Field | StateClass::WorldMap) {
                retail_object_exempt_bits(ram)
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
            rand_seed: matches!(
                class,
                StateClass::Field | StateClass::WorldMap | StateClass::Minigame
            )
            .then(|| rd32(ram, legaia_engine_system::rand::BIOS_RAND_SEED_ADDR) as u32),
            side_band: matches!(class, StateClass::Field | StateClass::WorldMap)
                .then(|| (rd32(ram, SIDE_BAND_REQUESTED), rd32(ram, SIDE_BAND_ACKED)))
                .filter(|&(req, ack)| req == ack && req >= 0)
                .and_then(|(req, _)| i16::try_from(req).ok()),
            timed_release: matches!(class, StateClass::Field | StateClass::WorldMap).then(|| {
                (
                    rd32(ram, TIMED_RELEASE_ARMED) != 0,
                    rd32(ram, TIMED_RELEASE_DEADLINE),
                    rd32(ram, TIMED_RELEASE_ELAPSED),
                    field_bgm_playing(ram),
                )
            }),
        };
        // A walking capture's frame is two passes old ([`super::walk_rewind`]);
        // a focus off the player is a poked or script-carried seat, which
        // the follow ease never ran for.
        if matches!(class, StateClass::Field | StateClass::WorldMap) && obs.seat_focus.is_none() {
            obs.walk = super::walk_rewind::walk_history(ram, &obs.camera);
        }
        obs
    }
}

/// The side-band request / acknowledge pair (`_DAT_8007BABC` /
/// `_DAT_8007BAA0`).
const SIDE_BAND_REQUESTED: u32 = 0x8007_BABC;
const SIDE_BAND_ACKED: u32 = 0x8007_BAA0;

/// Stand the engine's side-band request on the state's, at the resume of the
/// gate's record: the cues the record keys read their rows' programs out of
/// that bank, and under the card load's request they key nothing. A record
/// that requests again on its way to the gate replaces it, as retail's did.
/// Harness seat: the request law is op `0x36` sub `1`'s.
pub(crate) fn seat_side_band(world: &mut legaia_engine_core::world::World, retail: &RetailObs) {
    if let Some(request) = retail.side_band {
        world.seat_side_band_request(request);
    }
}

/// `FUN_800267A8`'s cells: `gp+0x808` armed, `gp+0x814` deadline,
/// `gp+0x81C` elapsed (`gp = 0x8007B318`).
const TIMED_RELEASE_ARMED: u32 = 0x8007_BB20;
const TIMED_RELEASE_DEADLINE: u32 = 0x8007_BB2C;
const TIMED_RELEASE_ELAPSED: u32 = 0x8007_BB34;

/// Stand the engine's timed release where the state's is, at the instant
/// the script gate meets the capture's PC: the seed replays the record from
/// an earlier op, and a sub-op `5` it re-arms has run the engine's vsyncs
/// since, not retail's. A release retail has already spent fires now (its
/// stop reaches the hosts as the expiry's sub-op `2`); one still counting
/// takes retail's elapsed count. Harness seat: the release law is
/// `World::tick`'s.
pub(crate) fn seat_timed_release(world: &mut legaia_engine_core::world::World, retail: &RetailObs) {
    let Some((armed, deadline, elapsed, playing)) = retail.timed_release else {
        return;
    };
    world.seat_sound_release(armed, deadline, elapsed, !playing);
}

/// Whether the field-BGM source `0x8007052C` is open on a playing libsnd
/// sequence: `+0x8` open, `+0xA` the sequence, whose record
/// (`*(0x801CD2C0 + seq * 4)`) holds the play bit `0x1` at `+0x98`.
fn field_bgm_playing(ram: &[u8]) -> bool {
    const SOURCE: u32 = 0x8007_052C;
    if game_anchors::u16_at(ram, SOURCE + 8) == 0 {
        return false;
    }
    let seq = u32::from(game_anchors::u16_at(ram, SOURCE + 0xA)) % 32;
    let rec = game_anchors::u32_at(ram, 0x801C_D2C0 + seq * 4);
    (0x8000_0000..0x8020_0000).contains(&rec) && game_anchors::u32_at(ram, rec + 0x98) & 1 != 0
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
/// Stand every ambient stream where the capture holds it and put the world
/// `rand()` stream on the capture's seed: after this a tick draws what
/// retail's next tick draws. The ambient-stream analogue of
/// [`hold_rand_seed`], for a run that lets the stream run on past the seat
/// (the walk film and its test).
pub fn seat_ambient_streams(session: &mut NativeSession, retail: &RetailObs) {
    let world = &mut session.host.world;
    let order: Vec<u16> = retail.ambient_streams.iter().map(|s| s.seat.flat).collect();
    world.seed_ambient_order(&order);
    for s in &retail.ambient_streams {
        world.seed_ambient_walker(s.seat.flat, s.x, s.z, s.heading);
        world.seed_ambient_stream(&s.seat);
    }
    if let Some((flat, heading)) = retail.talk_facing_save {
        world.seed_talk_facing_save(flat, heading);
    }
    if let Some(seed) = retail.rand_seed {
        world.rng_state = seed;
    }
}

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
            self.cue_clut_source = retail_cue_clut_source(v);
            return;
        }
        if let Some(v) = vram
            && matches!(
                self.class,
                StateClass::Field | StateClass::WorldMap | StateClass::Minigame
            )
        {
            self.scroll_rects = retail_scroll_rects(ram, v);
            self.panel = retail_panel(ram);
            self.sprite_widgets = retail_has_sprite_widget(ram);
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
        let Some(first) = self.scripts.running.first() else {
            return Vec::new();
        };
        // A set the straight-line walk passes can sit on an arm the record
        // jumped over: `town0e` `P2[5]` reaches `+0x8B` past `55 F5` at
        // `+0x89`, the target of the Triangle test `42 01 0B` at `+0x81`,
        // by the jump `26 04 00` at `+0x86`. Retail's flags say which arm
        // ran; a latch the state does not hold was not set, and raised at
        // the settle tick it would be a bit retail lacks.
        let mut out: Vec<u16> = first
            .latches
            .iter()
            .copied()
            .filter(|&l| self.save.as_ref().is_none_or(|s| system_flag_held(s, l)))
            .collect();
        // The system pass that spawned the record ended on the spawn, so the
        // entry retail ran saw its latch down ([`crate::retail_compare_script::system_spawn_arm`]). Held
        // back, the seed's system script takes the same arm - it spawns the
        // record and parks after the `21` where retail's stands - instead of
        // jumping the latched arm into a loop body retail has not run.
        if let Some((record, latch)) = self.scripts.system_spawn
            && record == first.flat_index
            && !out.contains(&latch)
        {
            out.push(latch);
        }
        out
    }

    /// The capture-alignment environment the image child takes on top of
    /// its seat: held-back latches, CLUT-cell phases, fog-pool records and
    /// scroller rects (the last through a file beside the child's frame).
    pub fn seat_env(&self, out_dir: Option<&Path>, label: &str) -> Vec<(&'static str, String)> {
        let mut env: Vec<(&'static str, String)> = self.seed_latches_env().into_iter().collect();
        env.extend(self.menu_lock_env());
        if !self.cell_fx.is_empty() {
            env.push(("LEGAIA_SEAT_CLUT_FX", cell_fx_env(&self.cell_fx)));
        }
        if !self.fog.is_empty() {
            env.push(("LEGAIA_SEAT_FOG", fog_env(&self.fog)));
            env.push(("LEGAIA_SEAT_FOG_OFFSET", self.fog_offset.to_string()));
        }
        // A field or overworld state that holds no sheet says so: the child
        // ran the scene from its entry, and an emitter that spawned on the way
        // (`bubu2`'s breath puff) leaves sheets the captured frame no longer
        // draws.
        if !self.sprite_arms.is_empty() {
            env.push((
                "LEGAIA_SEAT_SPRITE_ARMS",
                sprite_arms_env(&self.sprite_arms),
            ));
        } else if matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            env.push(("LEGAIA_SEAT_SPRITE_ARMS", "none".to_string()));
        }
        if !self.mesh_parts.is_empty() {
            env.push(("LEGAIA_SEAT_MESH_PARTS", sprite_arms_env(&self.mesh_parts)));
        }
        if let Some(b) = &self.balloon {
            env.push(("LEGAIA_SEAT_BALLOON", balloon_env(b.as_ref())));
        }
        if self.incense_notice {
            env.push(("LEGAIA_SEAT_INCENSE_NOTICE", "1".to_string()));
        }
        if !self.turning_parts.is_empty() {
            env.push((
                "LEGAIA_SEAT_TURNING_PARTS",
                turning_parts_env(&self.turning_parts),
            ));
        }
        if !self.object_flags.is_empty() {
            env.push((
                "LEGAIA_SEAT_OBJECT_FLAGS",
                self.object_flags
                    .iter()
                    .map(|(r, b)| format!("{r}:{b}"))
                    .collect::<Vec<_>>()
                    .join(","),
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
        if let Some(c) = self.player_clip {
            env.push(("LEGAIA_SEAT_PLAYER_CLIP", object_clips_env(&[c])));
        }
        if !self.morphs.is_empty() {
            env.push(("LEGAIA_SEAT_MORPHS", morphs_env(&self.morphs)));
        }
        if !self.morph_weights.is_empty() {
            env.push((
                "LEGAIA_SEAT_MORPH_WEIGHTS",
                legaia_engine_capture::field_seeds::morph_weights_env(&self.morph_weights),
            ));
        }
        if !self.floor_bobs.is_empty() {
            let back = usize::from(self.frame_step())
                * crate::retail_compare_battle::DISPLAY_LAG_PASSES as usize;
            let shown: Vec<_> = self
                .floor_bobs
                .iter()
                .map(|&b| rewind_floor_bob(b, back))
                .collect();
            env.push((
                "LEGAIA_SEAT_FLOOR_BOBS",
                legaia_engine_capture::field_seeds::floor_bobs_env(&shown),
            ));
        }
        if !self.walkers.is_empty() {
            env.push(("LEGAIA_SEAT_WALKERS", walkers_env(&self.walkers)));
        }
        if !self.scripted_seats.is_empty() {
            env.push(("LEGAIA_SEAT_PLACEMENTS", walkers_env(&self.scripted_seats)));
        }
        if !self.scripted_headings.is_empty() {
            env.push(("LEGAIA_SEAT_HEADINGS", walkers_env(&self.scripted_headings)));
        }
        if let Some([r, g, b]) = self.clear_rgb {
            env.push(("LEGAIA_SEAT_CLEAR", format!("{r},{g},{b}")));
        }
        if let Some(pairs) = &self.scripts.glide_pairs {
            env.push((
                "LEGAIA_SEAT_GLIDE_PAIRS",
                legaia_engine_capture::script_gate::glide_pairs_env(pairs),
            ));
        }
        if let Some([x, y, z]) = self.seat_eye {
            env.push(("LEGAIA_SEAT_EYE", format!("{x},{y},{z}")));
        }
        if let Some([p, y, h]) = self.seat_angles {
            env.push(("LEGAIA_SEAT_ANGLES", format!("{p},{y},{h}")));
        }
        // The script counters the seed holds through its settle window
        // (`hold_slot_table`): RAM outside the save window, so the child's
        // card load cannot bring them either.
        if let Some(slots) = &self.slot_table {
            env.push((
                "LEGAIA_SEAT_SLOT_TABLE",
                legaia_engine_capture::field_seeds::slot_table_env(slots),
            ));
        }
        if let Some(bias) = self.curve_bias.filter(|&b| b != 0) {
            env.push(("LEGAIA_SEAT_CURVE_BIAS", bias.to_string()));
        }
        env.push(("LEGAIA_SEAT_FRAME_STEP", self.seat_step().to_string()));
        // ... and holds it: the frame-cost model is play's estimate, the
        // state's step a measurement (`run_engine`'s seat).
        env.push(("LEGAIA_FIELD_FRAME_COST", "0".to_string()));
        if let Some(f) = self.page_mark {
            env.push(("LEGAIA_SEAT_PAGE_MARK", f.to_string()));
        }
        if let Some(mut t) = self.grade {
            let back =
                u16::from(self.frame_step()) * crate::retail_compare_battle::DISPLAY_LAG_PASSES;
            t.clock = (t.clock as i16).saturating_sub(back as i16).max(0) as u16;
            env.push((
                "LEGAIA_SEAT_GRADE",
                legaia_engine_capture::field_seeds::grade_env(&t),
            ));
        }
        if !self.lights.is_empty() {
            let back = i16::from(self.frame_step())
                * crate::retail_compare_battle::DISPLAY_LAG_PASSES as i16;
            let lights: Vec<_> = self
                .lights
                .iter()
                .cloned()
                .map(|mut l| {
                    l.rewind = back;
                    l
                })
                .collect();
            env.push((
                "LEGAIA_SEAT_LIGHTS",
                legaia_engine_capture::field_seeds::lights_env(&lights),
            ));
        }
        if let Some(p) = self.panel {
            // The display shows the tween two game frames back.
            let back = i16::from(self.frame_step())
                * crate::retail_compare_battle::DISPLAY_LAG_PASSES as i16;
            env.push(("LEGAIA_SEAT_PANEL", panel_env(&rewind_panel(p, back))));
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

    /// The hall cyclers' captured palettes as a `LEGAIA_SEAT_CELL_SOURCES` file
    /// under the image work directory, and the variable naming it.
    pub fn cell_sources_env(
        &self,
        out_dir: Option<&Path>,
        label: &str,
    ) -> Option<(&'static str, String)> {
        if self.cell_sources.is_empty() {
            return None;
        }
        let dir = crate::retail_compare_image::work_dir(out_dir);
        let path = dir.join(format!("{label}.csrc.bin"));
        (std::fs::create_dir_all(&dir).is_ok()
            && std::fs::write(&path, vram_rects_file(&self.cell_sources)).is_ok())
        .then(|| ("LEGAIA_SEAT_CELL_SOURCES", path.display().to_string()))
    }

    /// The frame step retail was running at the capture instant, in vsyncs a
    /// game frame ([`crate::retail_compare_battle::frame_step`], half the
    /// display lag). It is load history the engine's fixed tick does not
    /// measure, so the seed hands it over (`World::seed_frame_step`).
    pub fn frame_step(&self) -> u8 {
        (self.scripts.display_lag / 2).clamp(1, 4) as u8
    }

    /// The step the seed and its image child run at: the pacer's step byte
    /// `0x1F800393` where the state holds the scratchpad, else the step the
    /// display lag implies ([`Self::frame_step`]).
    ///
    /// The lag is rebuilt from the frame-time ring, which stores a frame of
    /// `0x2D0` hsyncs or more as the clamp `0x2BC`; [`Self::frame_step`]
    /// reads a clamped newest entry as step `4`, and the byte settles the
    /// states the ring cannot (`field_walled_collision_pin` holds the clamp
    /// newest under step byte `3`). `conc3_ceremony_plaza` (the Conkram
    /// ceremony, forty-odd actors) holds sixteen clamps under step `4`;
    /// seated at `3`, the ceremony record's walks and its `B8 23 84 0A` turn
    /// drifted apart, and flat `35` stood nine budget units into the turn
    /// where retail's `+0x54` reads four.
    ///
    /// Off the field and the overworld the history's step stands
    /// ([`Self::frame_step`]): a battle's display lag is confirmed against its
    /// ordering tables, and the rule that reads a clamped newest entry as
    /// step `4` stays off the battle half for the same reason.
    pub fn seat_step(&self) -> u8 {
        if !matches!(self.class, StateClass::Field | StateClass::WorldMap) {
            return self.frame_step();
        }
        self.step_bytes
            .map(|(_, step)| step)
            .filter(|&s| (1..=4).contains(&s))
            .unwrap_or_else(|| self.frame_step())
    }

    /// Read the pacer's step bytes off a state's scratchpad image.
    pub fn seat_step_bytes(&mut self, scratchpad: Option<&[u8]>) {
        self.step_bytes = step_bytes(scratchpad);
    }

    /// What a film hands `World::seed_frame_cost_history`: the history, its
    /// cursor, the step the next frame runs at and the frame time retail's
    /// frames cost from here on, taken as the newest sample's. `None` without
    /// a history or a step byte.
    ///
    /// The newest sample reading the clamp `0x2BC` under a step of `4` is a
    /// frame past the `0x2D0` rung (the resolver takes the raw time over the
    /// ring's longest only there, `0x800170F8`), so the cost is the first
    /// time that steps `4`; under a step of `3` it is a raw `700`.
    pub fn frame_cost_seat(&self) -> Option<([i16; 16], usize, u8, i32)> {
        let (history, cursor) = self.frame_history?;
        let (_, step) = self.step_bytes?;
        let newest = i32::from(history[(cursor + 15) & 0xF]);
        let cost = if newest == 0x2BC && step >= 4 {
            0x2D1
        } else {
            newest
        };
        (newest > 0 && step > 0).then_some((history, cursor, step, cost))
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
            if self.class != StateClass::Menu {
                self.seat_focus = None;
            }
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

    /// Land a walking capture's displayed-frame pose
    /// ([`super::walk_rewind::WalkHistory::rewind`]) as the image child's
    /// seat, eye and angles. `scratchpad` gives the walk-region box the
    /// composer aims from.
    pub fn seat_walk_rewind(&mut self, scratchpad: Option<&[u8]>) {
        let Some(walk) = self.walk else {
            return;
        };
        let attr_box = scratchpad
            .and_then(|s| s.get(0x384..0x388))
            .map(|b| [b[0], b[1], b[2], b[3]]);
        self.walk_attr_box = attr_box;
        let r = walk.rewind(attr_box);
        self.image_seat = Some(r.seat);
        self.seat_eye = Some(r.eye);
        self.seat_angles = Some(r.angles);
    }

    /// [`Self::seat_walk_rewind`] again with the composer's floor under the
    /// walk's two passes, which the engine's seat sampled
    /// (`EngineObs::walk_floors`). The previous pass's target sits on the
    /// floor tier under that pass's player, which the footing reaches only
    /// as it conforms: `kor_field_route_walk`'s player stood at footing
    /// `-56` on the pass before the RAM's, where the composer's floor
    /// `-64` put the eye-Y target on `768`, and retail's walk film reads
    /// that pass's staging eye Y `768`.
    pub fn reseat_walk_rewind_on_floors(&mut self, floors: Option<[i32; 2]>) {
        let (Some(walk), Some(floors), Some(_), Some(_)) =
            (self.walk, floors, self.image_seat, self.seat_eye)
        else {
            return;
        };
        let r = walk.with_floors(floors).rewind(self.walk_attr_box);
        self.image_seat = Some(r.seat);
        self.seat_eye = Some(r.eye);
        self.seat_angles = Some(r.angles);
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

    /// Read the field menu lock off a state's scratchpad story word
    /// (`0x1F800394`, index `0x394`). Field states only.
    pub fn seat_menu_lock(&mut self, scratchpad: Option<&[u8]>) {
        self.menu_lock = self.class == StateClass::Field
            && scratchpad
                .and_then(|s| s.get(0x394..0x398))
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .is_some_and(|w| w & legaia_engine_core::world::FIELD_MENU_LOCK_BIT != 0);
    }

    /// [`Self::menu_lock`] as `LEGAIA_SEAT_MENU_LOCK` for `play-window`.
    pub fn menu_lock_env(&self) -> Option<(&'static str, String)> {
        self.menu_lock
            .then(|| ("LEGAIA_SEAT_MENU_LOCK", "1".to_string()))
    }
}

/// The displayed frame. A minigame state may be in the slot machine's
/// 640-wide mode ([`Frame::from_vram_display_hires`]); every other class
/// keeps the 320-wide reader, so no state outside the minigames gains a
/// frame it was not scored on.
fn display_frame(ram: &[u8], vram: &[u8], rect: (u32, u32, u32, u32)) -> Option<Frame> {
    let mode = game_anchors::game_mode(ram);
    if mode == 0x19 {
        Frame::from_vram_display_hires(vram, rect)
    } else if matches!(mode, 0x1A | 0x1B)
        && super::movie_run::RetailMovie::from_ram(ram).is_24bit(ram)
    {
        Frame::from_vram_display_24(vram, rect)
    } else {
        Frame::from_vram_display(vram, rect)
    }
}

/// Read a library state of either emulator. `scus` is `SCUS_942.54`, which a
/// PCSX-Redux state needs for its RAM anchor search.
impl RetailObs {
    /// Take the follow camera's staging pose from the capture a menu state
    /// was opened from ([`CorpusEntry::staging_source`]).
    ///
    /// A pause menu's overlay loads over the field overlay's window, so a
    /// menu state's RAM holds the live pose but reads zero at the staging
    /// descriptor `0x801F3580`. The ease `FUN_801DB510` runs only on a frame
    /// the player moved, so a menu opened over a player standing mid-ease
    /// holds that ease's pose for as long as it is open: the target it was
    /// heading for is the source capture's descriptor. It is taken only when
    /// the source stands on this state's live pose and player position, so a
    /// source that moved first is not read as this state's.
    ///
    /// Returns the report note, `None` when nothing was looked up.
    pub fn seat_source_staging(&mut self, entry: &CorpusEntry, scus: &[u8]) -> Option<String> {
        if self.camera_staging.is_some() || self.class != StateClass::Menu {
            return None;
        }
        let (emulator, path) = entry.staging_source.clone()?;
        let source = CorpusEntry {
            emulator,
            path,
            staging_source: None,
            ..entry.clone()
        };
        let src = match read_retail(&source, scus) {
            Ok(src) => src,
            Err(e) => return Some(format!("staging source unreadable: {e:#}")),
        };
        let Some(staging) = src.camera_staging else {
            return Some("staging source holds no staging pose".into());
        };
        if src.camera != self.camera || src.player != self.player {
            return Some(format!(
                "staging source stands elsewhere (live {:?}, player {:?}); not taken",
                src.camera, src.player
            ));
        }
        self.camera_staging = Some(CameraObs {
            focus: self.camera.focus,
            ..staging
        });
        Some(format!(
            "staging pose {}/{}/{} eye={:?} from the source capture, on this state's live pose",
            staging.pitch, staging.yaw, staging.h, staging.eye
        ))
    }
}

/// The pacer's step bytes `(0x1F800392, 0x1F800393)` off a scratchpad image.
fn step_bytes(scratchpad: Option<&[u8]>) -> Option<(u8, u8)> {
    scratchpad
        .and_then(|s| s.get(0x392..0x394))
        .map(|b| (b[0], b[1]))
}

pub fn read_retail(entry: &CorpusEntry, scus: &[u8]) -> Result<RetailObs> {
    match entry.emulator {
        "mednafen" => {
            let st = MednafenState::from_path(&entry.path)
                .with_context(|| format!("parse {}", entry.path.display()))?;
            let ram = st.main_ram()?;
            let gpu = PsxGpu::new(&st);
            let frame = match (gpu.vram_bytes(), gpu.regs().display_crop_rect()) {
                (Some(v), Some(rect)) => display_frame(ram, v, rect),
                _ => None,
            };
            let mut obs = crate::retail_compare_battle::with_step_bytes(
                step_bytes(st.scratch_ram().ok()),
                || RetailObs::from_ram(ram, frame),
            );
            obs.seat_scroll_rects(ram, gpu.vram_bytes());
            obs.seat_step_bytes(st.scratch_ram().ok());
            obs.seat_view_window(st.scratch_ram().ok());
            obs.seat_menu_lock(st.scratch_ram().ok());
            obs.seat_walk_rewind(st.scratch_ram().ok());
            obs.frame_table = st
                .scratch_ram()
                .ok()
                .and_then(|s| retail_frame_table(ram, s));
            obs.kept_bgm = entry.kept_bgm;
            Ok(obs)
        }
        _ => {
            let (st, gpu) = psx_pcsx_redux::gpu::load_with_scus(&entry.path, scus)?;
            let frame = gpu
                .as_ref()
                .and_then(|g| display_frame(st.main_ram(), &g.vram, g.display_crop_rect()));
            let mut obs =
                crate::retail_compare_battle::with_step_bytes(step_bytes(st.scratchpad()), || {
                    RetailObs::from_ram(st.main_ram(), frame)
                });
            obs.seat_scroll_rects(st.main_ram(), gpu.as_ref().map(|g| g.vram.as_slice()));
            obs.seat_step_bytes(st.scratchpad());
            obs.seat_view_window(st.scratchpad());
            obs.seat_menu_lock(st.scratchpad());
            obs.seat_walk_rewind(st.scratchpad());
            obs.frame_table = st
                .scratchpad()
                .and_then(|s| retail_frame_table(st.main_ram(), s));
            obs.kept_bgm = entry.kept_bgm;
            Ok(obs)
        }
    }
}
