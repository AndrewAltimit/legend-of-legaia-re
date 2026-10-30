//! What a `play-window` run is launched with: the screenshot / key-script
//! harness config, the debug seeds, the cheat switches, the capture PNG writer
//! and the key-name tables the scripts and bindings are spelled in.

use super::*;
use legaia_engine_cheats::cheats::WorldCheats;

/// The launch's fidelity switches: which of the port's enhancements this
/// session runs with. One value, so "retail-faithful" has one definition on
/// the command line ([`Self::retail`], the `--retail` flag) instead of a
/// list of `--no-*` flags every caller has to keep complete.
#[derive(Debug, Clone, Copy)]
pub struct LaunchFidelity {
    /// Enhanced lighting forced on or off; `None` takes the persisted option.
    pub dynamic_lighting: Option<bool>,
    /// The enhanced point lights' shadows.
    pub dyn_shadows: bool,
    /// The scene-entry VDF pulse over packs retail never arms.
    pub entry_pulse: bool,
    /// Smarter monster targeting (`--smart-monsters`,
    /// `WorldToggles::smarter_monster_targeting`). Not retail.
    pub smart_monsters: bool,
    /// The Arts command opens the port's saved-chain list
    /// (`--arts-saved-list`, `WorldToggles::arts_saved_list`). Not retail.
    pub arts_saved_list: bool,
    /// The gameplay tweaks - the patcher features graduated into engine
    /// switches (Shiny Seru, run-away EXP, the EXP rate, the Fortress
    /// encounter fix, the jewel fix) and the battle HP-readout guard:
    /// `--no-shiny-seru`, `--flee-exp`, `--exp-rate`,
    /// `--no-fortress-encounter-fix`, `--jewel-fix`, `--no-hp-readout-guard`.
    pub tweaks: legaia_engine_session::session_switches::GameplayTweaks,
    /// The camera-occlusion fade around the player.
    pub occlusion_fade: bool,
    /// The volumetric ground fog.
    pub volumetric_fog: bool,
    /// Pin every persisted engine-only option to its retail value for the
    /// session (`OptionsState::pin_retail`): the framing, the movement
    /// decode, the photosensitivity slew, the tile windows.
    pub retail: bool,
    /// PSX-faithful rasterisation (vertex jitter + 15-bit dither,
    /// `Renderer::set_psx_mode`), `--psx-render`. Opt-in on both settings:
    /// rasterisation defaults to clean, and retail-faithful *shading* does
    /// not imply it.
    pub psx_render: bool,
    /// The window's fullscreen / size launch overrides. Presentation, not
    /// fidelity: carried here so the one launch value reaches the window,
    /// and left at its default by both constructors below.
    pub window: WindowLaunch,
}

/// `play-window`'s window launch flags: `--fullscreen` / `--windowed` and
/// `--window-scale`. `None` takes the persisted option
/// (`OptionsState::fullscreen` / `window_scale`). Window chrome only: the
/// renderer draws the same frame at any surface size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowLaunch {
    pub fullscreen: Option<bool>,
    pub scale: Option<u8>,
}

impl WindowLaunch {
    /// From the command line's three flags.
    pub fn from_flags(fullscreen: bool, windowed: bool, scale: Option<u8>) -> Self {
        Self {
            fullscreen: if fullscreen {
                Some(true)
            } else if windowed {
                Some(false)
            } else {
                None
            },
            scale,
        }
    }
}

impl LaunchFidelity {
    /// The play-window's interactive default: every enhancement at its
    /// persisted option.
    pub fn enhanced() -> Self {
        Self {
            dynamic_lighting: None,
            dyn_shadows: true,
            entry_pulse: true,
            smart_monsters: false,
            arts_saved_list: false,
            tweaks: legaia_engine_session::session_switches::GameplayTweaks::default(),
            occlusion_fade: true,
            volumetric_fog: true,
            retail: false,
            psx_render: false,
            window: WindowLaunch::default(),
        }
    }

    /// Retail-faithful: every enhancement off, and the persisted options
    /// pinned. What a parity capture and a replay run under.
    pub fn retail() -> Self {
        Self {
            dynamic_lighting: Some(false),
            dyn_shadows: false,
            entry_pulse: false,
            smart_monsters: false,
            arts_saved_list: false,
            tweaks: legaia_engine_session::session_switches::GameplayTweaks::retail(),
            occlusion_fade: false,
            volumetric_fog: false,
            retail: true,
            psx_render: false,
            window: WindowLaunch::default(),
        }
    }
}

/// Deterministic screenshot harness for `play-window`: render one frame
/// offscreen at [`Self::capture_tick`] and write it to [`Self::path`], then
/// exit - and/or a periodic sweep ([`Self::sweep`]) that captures every N
/// ticks into a directory. [`Self::pad_script`] maps a world-tick to a
/// pad mask (one-tick edges, or held ranges via `FIRST-LAST:BUTTON`) so a
/// capture run can auto-open + navigate a menu - or hold a direction to
/// walk somewhere - without `xdotool`.
pub struct ScreenshotConfig {
    /// Single-shot output (`--screenshot`). `None` when only the periodic
    /// sweep is requested.
    pub path: Option<std::path::PathBuf>,
    pub capture_tick: u64,
    /// `--screenshot-chrome`: keep the window's timed chrome (the start-up
    /// key hint, a toggle's notice line) in the capture. Off by default, so
    /// a parity capture shows only what the game draws.
    pub chrome: bool,
    /// Periodic capture sweep (`--screenshot-every` + `--screenshot-dir`).
    pub sweep: Option<ScreenshotSweep>,
    /// tick -> pad button mask pressed for exactly that tick.
    pub pad_script: std::collections::HashMap<u64, u16>,
    /// tick -> window **keys** pressed (and released again) on exactly that
    /// tick, in the listed order ([`Self::key_script`], `--key-script`).
    ///
    /// This is a different channel from [`Self::pad_script`], not a superset
    /// of it, and the split is structural rather than stylistic. `pad_script`
    /// writes the pad word directly and the keyboard handler never sees it;
    /// several of the window's surfaces are reachable *only* from the
    /// keyboard handler - every minigame entry (`K`/`U`/`L`/`O`/`M`/`B`), the
    /// fishing prize exchange (`P` + arrows + Enter) and the inline-dialogue
    /// option picker - so a pad-only script cannot open any of them. A
    /// key-script entry is delivered as a press followed immediately by a
    /// release, so a key that also maps to a pad button leaves no bit latched
    /// and the engine tick never sees an edge from it: drive pad input with
    /// `--pad-script`, dev/entry keys with this.
    pub key_script: std::collections::HashMap<u64, Vec<KeyCode>>,
    /// `LEGAIA_HUD_COUNTDOWN=<n>`: the field party HUD's idle countdown
    /// (retail's `_DAT_801F348C`) the capture must land on, so a stationary
    /// seat shows the readout exactly when the retail frame does
    /// ([`legaia_engine_field::world_map_panel_host::hud_phase_hold`]). Set by
    /// the retail-compare image channel from the state's own RAM.
    pub hud_countdown: Option<i16>,
    /// `LEGAIA_CAPTURE_TALK=<slot>@<tick>`: at that world tick, talk to the
    /// scene's placement `slot` through `World::trigger_field_interact` - the
    /// call the walk-up interaction probe makes - so a capture can open a
    /// merchant's or innkeeper's conversation without walking to it. The
    /// browser twin is `LegaiaRuntime::debug_talk_to_placement`.
    pub talk_at: Option<(u64, u8)>,
    /// `LEGAIA_CAPTURE_SHOP=<idx>@<tick>`: stage scene shop `idx` at `tick`
    /// (`World::debug_arm_scene_shop`), the browser page's
    /// `debug_arm_scene_shop` twin.
    pub shop_at: Option<(u64, usize)>,
    /// `LEGAIA_CAPTURE_GATE=state[,white|black,age]`: capture the first
    /// frame the battle's action SM holds the retail capture's phase
    /// ([`legaia_engine_capture::phase_gate::PhaseGate`]) instead of
    /// at a fixed tick; `capture_tick` becomes the deadline, past which the
    /// run exits without a PNG. Set by the retail-compare image channel.
    pub phase_gate: Option<legaia_engine_capture::phase_gate::PhaseGate>,
    /// `LEGAIA_CAPTURE_ARRIVAL=cue:<level>|gone`: capture an evolved-Cort
    /// prompt frame on the arrival columns' life
    /// ([`legaia_engine_capture::arrival_gate::ArrivalGate`]), with
    /// `capture_tick` the deadline. Set by the retail-compare image channel.
    pub arrival_gate: Option<legaia_engine_capture::arrival_gate::ArrivalGate>,
    /// `LEGAIA_BATTLE_ORBIT_YAW=<yaw>`: the battle idle orbit's azimuth the
    /// capture must show, held while the orbit owns yaw
    /// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::align_orbit_yaw`]).
    /// The orbit is a clock, so this is its phase alignment - the camera twin
    /// of [`Self::hud_countdown`]. Set by the retail-compare image channel
    /// from the battle state's own rotation global.
    pub battle_orbit_yaw: Option<f32>,
    /// `LEGAIA_BATTLE_BACKDROP_YAW=<angle>`: the backdrop draw's slot-1 Y
    /// angle the capture must show (`World::seed_battle_backdrop_slot_1_yaw`),
    /// held on every battle frame. The angle is time spent in keeping fights
    /// since boot, so this is its phase alignment - the backdrop twin of
    /// [`Self::battle_orbit_yaw`]. Set by the retail-compare image channel
    /// from the battle state's `0x800891D2`.
    pub battle_backdrop_yaw: Option<u16>,
    /// `LEGAIA_BATTLE_CAM_ALIGN=<16 numbers>`: retail's live battle camera
    /// and its tween endpoints (pitch, yaw, eye trio, focus trio each), for
    /// starting the captured phase's glide where retail's stood
    /// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::align_glide_origin`]).
    /// Set by the retail-compare image channel.
    pub battle_cam_align: Option<(
        legaia_engine_camera::projection::BattleCamPose,
        legaia_engine_camera::projection::BattleCamPose,
    )>,
    /// `LEGAIA_BATTLE_CAM_ALIGN_DISPLAYED=<16 numbers>`: the displayed
    /// frame's live pose and the same endpoints, taken over
    /// [`Self::battle_cam_align`]'s live pose when the engine's glide heads
    /// for those endpoints on every component
    /// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::glide_endpoint_matches`]).
    /// Set by the retail-compare image channel on a Done-band capture.
    /// `LEGAIA_BATTLE_GROUND_SNAP=slot:x:z[:facing],...`: the capture's
    /// combatants' live pairs and headings, by engine slot, which the captured
    /// frame stands them on before the camera is aligned
    /// ([`legaia_engine_capture::ground_snap`]). Set by the retail-compare
    /// image channel on an action drive.
    pub battle_ground_snap: Vec<legaia_engine_capture::ground_snap::GroundSnap>,
    /// `LEGAIA_CAST_RAND_SEED=st7:phase:seed`: a recorded cast's `rand()`
    /// seed on one cast key, set on the world stream before every tick under
    /// that key ([`legaia_engine_capture::cast_rand`]). Set by the
    /// cast-sequence oracle's `--rand-at`.
    pub cast_rand_seed: Option<legaia_engine_capture::cast_rand::CastRandSeed>,
    /// `LEGAIA_BATTLE_DISPLAYED_SEAT=slot:x:z,...`: live pairs the captured
    /// frame stands combatants on **after** the camera is aligned on
    /// [`Self::battle_ground_snap`] - the displayed frame's ground of a seat
    /// that moved between that frame and the RAM's. Set by the
    /// retail-compare image channel (`RetailBattle::engine_image_displayed_seats`).
    pub battle_displayed_seat: Vec<legaia_engine_capture::ground_snap::GroundSnap>,
    pub battle_cam_align_displayed: Option<(
        legaia_engine_camera::projection::BattleCamPose,
        legaia_engine_camera::projection::BattleCamPose,
    )>,
    /// `LEGAIA_BATTLE_MODULE_CAM_ALIGN=<16 numbers>`: the displayed frame's
    /// live pose and retail's tween endpoints under a slot-B module's shot
    /// still walking, which the captured frame's shot takes on every
    /// component whose endpoint agrees
    /// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::align_module_shot`]).
    /// Set by the retail-compare image channel on a replayed cast.
    pub battle_module_cam_align: Option<(
        legaia_engine_camera::projection::BattleCamPose,
        legaia_engine_camera::projection::BattleCamPose,
    )>,
    /// `LEGAIA_BATTLE_SUMMON_FOCUS=<x>,<z>`: the caster's body pair the
    /// displayed frame's summon close-up was framed on - the pose three
    /// passes behind the RAM's cursor - which the captured frame's close-up
    /// takes as its focus
    /// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::align_summon_focus`]).
    /// Set by the retail-compare image channel.
    pub battle_summon_focus: Option<[f32; 2]>,
    /// `LEGAIA_SEAT_HUD_GLIDES_LANDED=1`: the retail capture's HUD widget
    /// glides had all landed, so the captured frame lands the engine's too
    /// (`BattleState::land_battle_hud_glides`). Set by the retail-compare image
    /// channel.
    pub seat_hud_glides_landed: bool,
    /// `LEGAIA_SEAT_HUD_GLIDES=x:y:elapsed:total,...`: the retail capture's
    /// HUD widget glides still in flight, each at the elapsed its displayed
    /// frame shows; the captured frame seats the engine's on them
    /// (`World::seat_battle_hud_glide`, the combo cluster's age). Set by the
    /// retail-compare image channel.
    pub seat_hud_glides: Vec<legaia_engine_capture::hud_glide::HudGlideSeat>,
    /// `LEGAIA_SEAT_MENU_GLIDE_LEFT=<frames>`: a round-prompt capture whose
    /// far framing (case 9) was still walking, with the frames of its arm
    /// the displayed frame had left; the captured frame stands the engine's
    /// own arm as far along
    /// ([`legaia_engine_battle_vm::battle_cam_script::BattleCamera::align_menu_glide_frames_left`]).
    /// Set by the retail-compare image channel.
    pub seat_menu_glide_left: Option<f32>,
    /// `LEGAIA_SEAT_POPUP_AGE=<engine slot>:<age>`: the newest damage
    /// numeral's age as the retail capture's displayed frame drew it; the
    /// captured frame ages the engine's newest popup on that slot to it.
    /// Set by the retail-compare image channel.
    pub seat_popup_age: Option<(u8, u16)>,
    /// `LEGAIA_SEAT_POPUP_CELLS=x:y:w:digit,...`: the numeral cells the
    /// retail capture's displayed frame drew; the captured frame draws them
    /// in place of the engine's runs.
    pub seat_popup_cells: Vec<(i32, i32, u32, u8)>,
    /// Set on the frame the capture is due, for [`Self::seat_popup_cells`].
    pub popup_cells_due: std::cell::Cell<bool>,
    /// Whether [`Self::seat_popup_age`] has aged the popup yet: once, on the
    /// frame the capture is due.
    pub popup_age_seated: std::cell::Cell<bool>,
    /// `LEGAIA_SEAT_SPIRIT_GAUGE=<bar width>,<plate value>`: a Spirit turn's
    /// AP bar width and AP plate value as the retail capture's displayed
    /// frame shows them; the captured frame seats the engine's on them.
    /// Set by the retail-compare image channel.
    pub seat_spirit_gauge: Option<(i16, i16)>,
    /// `LEGAIA_SEAT_ARTS_LIST=<page>,<slide>`: the arts entry's Triangle
    /// list open on the retail capture's page with its window's slide
    /// counter; the captured frame seats the engine's entry on them
    /// (`BattleState::seat_arts_list`). Set by the retail-compare image channel.
    pub seat_arts_list: Option<(u8, u8)>,
    /// `LEGAIA_SCRIPT_GATE=<flat>:<head hex>:<pc>:<wait>`: the field twin of
    /// [`Self::phase_gate`] - capture the first frame the engine's context
    /// for a retail capture's running record holds its PC
    /// ([`legaia_engine_capture::script_gate::ScriptGate`]), resuming
    /// the record from its start when nothing runs it and paging its dialog
    /// boxes on the way. `capture_tick` is the deadline.
    pub script_gate: Option<legaia_engine_capture::script_gate::ScriptGate>,
    /// `LEGAIA_SEAT_LATCHES=59c,134` (hex): the system flags a captured
    /// record latched after the scene entry ran. The seed lands the save
    /// without them so the entry does not read them, and they are raised at
    /// the settle tick, where retail's state holds them
    /// (`RetailObs::seed_save`).
    pub seat_latches: Vec<u16>,
    /// `LEGAIA_SEAT_OBJECT_MODELS=<record>:<model>,..`: a retail state's
    /// drawn actors' live model ids, written over the placed objects' stream
    /// swaps (`World::object_live_models`) on the capture frame.
    pub seat_object_models: Vec<(usize, i16)>,
    /// `LEGAIA_SEAT_OBJECT_CLIPS=<record>:<clip>:<cursor>:<flags>:<rate>;..`:
    /// a retail state's drawn actors' clip words
    /// (`legaia_parity::retail_compare::retail_object_clips`), written over
    /// the matching placed props' clips on the capture frame
    /// (`World::seed_object_prop_clip`).
    pub seat_object_clips: Vec<legaia_engine_capture::field_seeds::ObjectClipSeed>,
    /// `LEGAIA_SEAT_MORPHS=<fields>`: a retail state's live VDF morph
    /// envelopes (`legaia_parity::retail_compare::retail_morphs`), written
    /// over the engine's on the capture frame (`World::seed_field_morph`).
    pub seat_morphs: Vec<legaia_engine_capture::field_seeds::MorphSeed>,
    /// `LEGAIA_SEAT_WALKERS=<fields>`: a retail state's ambient walkers'
    /// live seats (`legaia_parity::retail_compare::retail_walkers`), stood
    /// on the capture frame (`World::seed_ambient_walker`).
    pub seat_walkers: Vec<legaia_engine_capture::field_seeds::WalkerSeed>,
    /// `LEGAIA_SEAT_PLACEMENTS=<fields>`: placements a finished cutscene
    /// beat left on a script-literal tile
    /// (`legaia_parity::retail_compare::scripted_seats`), stood there,
    /// position only, on the capture frame (`World::seed_placement_seat`).
    pub seat_placements: Vec<legaia_engine_capture::field_seeds::WalkerSeed>,
    /// `LEGAIA_SEAT_PANEL=<fields>`: a retail state's live image-panel
    /// widget (`legaia_parity::retail_compare::retail_panel`), installed on
    /// the capture frame - the vignette record that spawned it ran before
    /// the record the seed resumes.
    pub seat_panel: Option<legaia_engine_effects::screen_fx::PanelWidget>,
    /// `LEGAIA_SEAT_CLEAR=r,g,b`: a retail state's frame clear colour (the
    /// draw environment's `r0 / g0 / b0`), written over the engine's on the
    /// capture frame - the system script's op-`4C 13` history.
    pub seat_clear: Option<[u8; 3]>,
    /// `LEGAIA_SEAT_CURVE_BIAS=rows`: a retail state's overworld curvature
    /// build offset (`legaia_parity::retail_compare::retail_curve_bias`),
    /// written over the engine's on the capture frame - `-6` in a state
    /// whose player had opened the map display since the scene loaded.
    pub seat_curve_bias: Option<i16>,
    /// `LEGAIA_SEAT_PAGE_MARK=frame`: which frame of the dialogue page
    /// mark's two-frame strip a retail state's displayed frame shows
    /// (`legaia_parity::retail_compare::retail_page_mark`), seated on the
    /// capture frame.
    pub seat_page_mark: Option<u8>,
    /// `LEGAIA_SEAT_FOG`: a retail state's live fog-pool records, installed
    /// over the pool on the frame the capture is taken
    /// (`FogPool::install_snapshot`); taken once.
    pub seat_fog: std::cell::Cell<Option<Vec<legaia_engine_field::fog_particles::FogParticle>>>,
    /// Raised on the capture frame by the seating pass and taken by the
    /// frame's fog draw, which installs `seat_fog` and re-runs the frame's
    /// render step over it (`BootSession::seat_field_fog`) where the draw
    /// prep resolves the camera.
    pub seat_fog_due: std::cell::Cell<bool>,
    /// `LEGAIA_SEAT_SPRITE_ARMS`: a retail state's live sprite-arm sheets,
    /// installed over the ambient pool's on the frame the capture is taken
    /// (`World::install_sprite_arm_snapshot`); taken once.
    pub seat_sprite_arms:
        std::cell::Cell<Option<Vec<legaia_engine_core::world::ambient::SpriteArmSeed>>>,
    /// `LEGAIA_SEAT_TURNING_PARTS`: the render banks of a retail state's
    /// turning ambient parts as its displayed frame drew them, written over
    /// the engine's on the frame the capture is taken
    /// (`World::seat_ambient_part_banks`); taken once.
    pub seat_turning_parts:
        std::cell::Cell<Option<Vec<legaia_engine_core::world::ambient::AmbientBankSeed>>>,
    /// `LEGAIA_SEAT_BALLOON`: the text balloon a retail state's displayed
    /// frame drew (the place-name banner a door leaves up, which a card
    /// load never raises), or none, written over the field's on the frame
    /// the capture is taken; taken once.
    pub seat_balloon:
        std::cell::Cell<Option<Option<legaia_engine_dialog::text_balloon::TextBalloon>>>,
    /// `LEGAIA_SEAT_FADE_LEAD=<vsyncs>`: on the frame the capture is taken,
    /// step every live screen fade that many vsyncs on
    /// (`World::lead_screen_fades`), for a capture whose displayed fade
    /// stands nearer the RAM than the rest of its frame; taken once. Set by
    /// the retail-compare image channel.
    pub seat_fade_lead: std::cell::Cell<Option<u16>>,
    /// `LEGAIA_BATTLE_DRIVE=menu,<flow>,<seat>` /
    /// `action,<seat>,<state>,<category>,<queued>`: walk the fight through
    /// its pad path to the retail capture's phase and capture the first
    /// frame that holds it
    /// ([`legaia_engine_capture::battle_drive::BattleDrive`]), the
    /// same driver the headless seed runs. `capture_tick` becomes the
    /// deadline. Set by the retail-compare image channel.
    pub battle_drive: Option<legaia_engine_capture::battle_drive::BattleDrive>,
    /// Where the drive's start and press clock stand: it starts where the
    /// headless seed's opening loop hands its drive over (past the entry
    /// sweep, on the first tick the command flow leaves `Idle`), and counts
    /// its presses from there
    /// ([`legaia_engine_capture::drive_clock::DriveClock`]), whatever the
    /// window's own tick count was.
    pub battle_drive_clock: std::cell::Cell<legaia_engine_capture::drive_clock::DriveClock>,
    /// Ticks the drive's phase has held so far; the capture waits for
    /// [`legaia_engine_capture::battle_drive::BattleDrive::hold_ticks`]
    /// of them, as the headless seed does.
    pub battle_drive_held: std::cell::Cell<u32>,
    /// `LEGAIA_CAPTURE_DISPLAY_LAG=<ticks>`: the frame a retail capture's
    /// display shows is this many vsyncs older than its RAM (two game
    /// frames). On a battle drive whose action state already held that
    /// many ticks before the gate, the child writes the earlier tick beside
    /// its frame (`<shot>.retake`) for the caller to take the frame on
    /// (`LEGAIA_DIAG_CAPTURE_TICK`). The caller sets it only for the span
    /// gates where the displayed frame measures closer
    /// (`RetailBattle::display_retake` in `legaia-parity`).
    pub display_lag: u64,
    /// `LEGAIA_CAPTURE_RETAKE_SHOT=1`: retake only when the battle camera
    /// armed the magic band's shot it stands on after the retake's tick, so
    /// the gate's frame shows a different shot than the display did
    /// (`RetailBattle::retake_on_shot_only` in `legaia-parity`).
    pub retake_shot_only: bool,
    /// The first tick of the drive's current run in its action state.
    pub state_since: std::cell::Cell<Option<u64>>,
    /// The framing the battle camera's law armed on the gate tick, kept for
    /// the `<shot>.retake` the gate writes beside the frame
    /// ([`Self::display_lag`]).
    pub gate_cam_target: std::cell::Cell<Option<legaia_engine_camera::projection::BattleCamPose>>,
    /// `LEGAIA_BATTLE_CAM_GATE_TARGET=<8 numbers>`: on a display-lag retake,
    /// the framing the first run's camera armed on the gate tick. Retail's
    /// tween endpoints (`LEGAIA_BATTLE_CAM_ALIGN`) are the RAM's, the gate
    /// tick's; the retake's own framing is two game frames older and may
    /// stand a step of yaw drift or depth ramp short of them. The agreement
    /// test is the gate's, so the retake aligns against this framing and
    /// draws the displayed live pose.
    pub battle_cam_gate_target: Option<legaia_engine_camera::projection::BattleCamPose>,
    /// `LEGAIA_BATTLE_BARS=slot:hp:mp,...`: the capture's mid-fight HP / MP,
    /// seeded on the first battle tick as the headless seed seeds them
    /// ([`legaia_engine_capture::bar_seeds::apply_bar_seeds`]).
    pub battle_bars: Vec<legaia_engine_capture::bar_seeds::BarSeed>,
    /// `LEGAIA_BATTLE_RESEAT=<engine slot>`: re-seat the bars' ground each
    /// time that slot's action seeds
    /// (`legaia_engine_capture::bar_seeds::arm_capture_reseat`).
    pub battle_reseat: Option<u8>,
    /// `LEGAIA_BATTLE_APPROACH_YAW` (`CaptureApproach::to_env`): the
    /// re-seated attacker's walk onto its target starts where retail's did
    /// (`legaia_engine_battle_loop::battle_state::BattleState::capture_approach_yaw`).
    pub battle_approach_yaw: Option<legaia_engine_battle_loop::battle_state::CaptureApproach>,
    /// `LEGAIA_BATTLE_BACKDROP_CUE=<weight>`: the backdrop pair's depth-cue
    /// weight a capture-band capture holds, stood on at the captured phase.
    pub battle_backdrop_cue: Option<u16>,
    /// Whether [`Self::battle_bars`] have been seeded.
    pub battle_bars_seeded: std::cell::Cell<bool>,
    /// `LEGAIA_BATTLE_MYSTIC_SHIELD=1`: the capture holds Cort's Mystic
    /// Shield up (`_DAT_8007BD84` non-null), raised on the first battle tick
    /// beside the bars as the headless seed raises it
    /// (`World::raise_mystic_shield`). Cleared once raised.
    pub mystic_shield: std::cell::Cell<bool>,
    /// `LEGAIA_BATTLE_MELEE_HITS=slot:amount,...`: the hits a captured
    /// party melee action had landed, dealt again at those amounts by the
    /// replay (`legaia_engine_capture::melee_hits`). Seeded with the drive.
    pub battle_melee_hits: legaia_engine_capture::melee_hits::MeleeHitSeed,
    /// `LEGAIA_SEAT_CAST_PARTS`: a captured cast's live slot-B parts, seated
    /// over the replay's parts of the same records on the frame the capture
    /// is taken (`SummonScene::install_part_snapshot`); taken once.
    pub seat_cast_parts: std::cell::Cell<Option<Vec<legaia_engine_effects::summon::CastPartSeed>>>,
    /// `LEGAIA_SEAT_ACTION_FX_PARTS`: a captured state's turning
    /// effect-script table parts, each its displayed rotation trio, seated
    /// over the replay's parts of the same records on the capture frame
    /// (`World::install_action_fx_part_snapshot`); taken once.
    pub seat_action_fx_parts:
        std::cell::Cell<Option<Vec<legaia_engine_effects::summon::CastPartSeed>>>,
    /// `LEGAIA_SEAT_EFFECT_CHILDREN`: a captured state's live effect-pool
    /// children as the displayed frame held them, stood in the engine's
    /// pool on the capture frame (`World::install_effect_child_snapshot`);
    /// taken once.
    pub seat_effect_children:
        std::cell::Cell<Option<Vec<legaia_engine_core::world::EffectChildSeed>>>,
    /// `LEGAIA_SEAT_BATTLE_CLIPS=slot:clip:cursor,...`: each body's playing
    /// clip cursor as the capture held it, landed on the capture frame
    /// ([`legaia_engine_capture::clip_cursors`]).
    pub seat_battle_clips: Vec<legaia_engine_capture::clip_cursors::ClipCursorSeed>,
    /// `LEGAIA_SEAT_GHOST_RING=slot:clip:depth:drawn:cursor:x:z,...`: each after-image
    /// ghost as the capture's history ring holds it, seated on the capture
    /// frame ([`legaia_engine_capture::ghost_ring`]); taken once.
    pub seat_ghost_ring:
        std::cell::Cell<Option<Vec<legaia_engine_capture::ghost_ring::GhostRingSeed>>>,
}

/// Debug state `play-window` seeds before the first world tick, so a capture
/// run can reach a surface an ordinary boot would have to be played into.
///
/// Both members are **debug affordances**, not player surfaces: they write
/// game state a save would otherwise have to carry. They exist because the
/// two screens the parity sweeps most need - a Seru cast and a Muscle Dome
/// ring with a chip crossed out - are each gated on state no CLI flag could
/// reach. A cast needs the caster to have *learned* the spell (the Magic arm
/// parks on an unlearned action id), and the dome's magic ban is seeded at
/// arena entry from story flags `0x536` / `0x537` / `0x538`
/// (`legaia_engine_minigames::muscle_dome::COURSE_UNLOCK_FLAGS`), the last one set
/// winning with `0x321` - course 2 plus
/// `SPECIAL_MAGIC_FORBIDDEN`.
#[derive(Debug, Default, Clone)]
pub struct DebugSeeds {
    /// `--learn-spell <ID>`, repeatable: spell ids prepended (level 1) to the
    /// lead party member's learned list, the same path the pre-existing
    /// `LEGAIA_LEARN_SPELLS` env var takes.
    pub learn_spells: Vec<u8>,
    /// `--set-flag <N>`, repeatable: system/story-flag indices raised in the
    /// shared bank `DAT_80085758`.
    pub story_flags: Vec<u16>,
    /// `--resume-save <PATH>`: an LGSF file to land the way a card Load
    /// does ([`legaia_engine_session::native::NativeSession::resume_save`]) in
    /// place of the `--scene` door entry.
    pub resume_save: Option<std::path::PathBuf>,
    /// `--cheat-*`: the player cheats applied once the scene is entered.
    pub cheats: PlayCheats,
    /// `--play-record P:R`: a scene MAN record installed as a cutscene
    /// timeline once the scene is entered.
    pub play_record: Option<(usize, usize)>,
    /// `--play-cutscene ID`: an expansion-pack cutscene played once the
    /// scene is entered.
    pub play_cutscene: Option<String>,
    /// `--expansion PATH [--map ID [--spawn ID]]`: the expansion pack every
    /// entry plays with, and the pack map entered in place of the `--scene`
    /// door entry.
    pub expansion: Option<ExpansionLaunch>,
    // --- expansion game modes (`--mode`) ---
    /// `--mode <INSTANCE>`: an expansion mode instance entered once the
    /// scene is entered (`picker` opens the mode picker instead).
    pub pack_mode: Option<String>,
    // --- end expansion game modes ---
    /// `--host` / `--join` / `--room` / `--name`: a multiplayer room to open.
    pub netplay: super::NetLaunch,
}

/// The `--expansion` / `--map` / `--spawn` operands.
#[derive(Debug, Clone)]
pub struct ExpansionLaunch {
    /// The pack: a directory or an `.lxpack` bundle.
    pub pack: std::path::PathBuf,
    /// The map to boot into; `None` boots as without a pack (title screen,
    /// `--scene`) with the pack active.
    pub map: Option<String>,
    /// Empty for the map's first spawn.
    pub spawn: String,
    /// `--expansion-watch`: reload the pack whenever its files change
    /// (`window::pack_reload`).
    pub watch: bool,
}

impl ExpansionLaunch {
    /// Read the pack, a directory or an `.lxpack` bundle
    /// (`legaia_expansion_format::read_files`).
    ///
    /// A `pack` that is not a path names an installed pack by id, looked up
    /// an installed or bundled pack (`crate::launcher::resolve_pack`).
    pub fn load(&self) -> Result<legaia_expansion_format::ExpansionPack> {
        let path = self.resolve_path()?;
        legaia_expansion_format::read_files(&path)
            .and_then(|files| legaia_expansion_format::ExpansionPack::from_files(&files))
            .with_context(|| format!("--expansion {}", path.display()))
    }

    /// The pack folder or `.lxpack` file `pack` names: a path, or the id of
    /// an installed or bundled pack (`crate::launcher::resolve_pack`).
    pub fn resolve_path(&self) -> Result<std::path::PathBuf> {
        let dirs = crate::launcher::pack_dirs();
        crate::launcher::resolve_pack(&self.pack, &dirs).with_context(|| {
            let searched: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
            format!(
                "--expansion {}: no such file or folder, and no installed or bundled pack has \
                 that id{}",
                self.pack.display(),
                if searched.is_empty() {
                    String::new()
                } else {
                    format!(" (searched {})", searched.join(", "))
                }
            )
        })
    }

    /// [`Self::load`] for a hot reload: the validator runs first, and a pack
    /// with an error is refused with its first lines, so the running pack
    /// stays (activation itself checks less).
    pub fn load_checked(&self) -> Result<legaia_expansion_format::ExpansionPack> {
        let path = self.resolve_path()?;
        let files = legaia_expansion_format::read_files(&path)
            .with_context(|| format!("--expansion {}", path.display()))?;
        let issues = legaia_expansion_format::validate::validate_files(&files, None);
        let errors = legaia_expansion_format::share::error_lines(&issues);
        if !errors.is_empty() {
            let more = match errors.len() {
                1 => String::new(),
                n => format!(" (and {} more)", n - 1),
            };
            anyhow::bail!("{}{more}", errors[0]);
        }
        legaia_expansion_format::ExpansionPack::from_files(&files)
            .with_context(|| format!("--expansion {}", path.display()))
    }
}

/// The `--cheat-*` operands - the native twin of the browser play page's
/// Cheats panel (its "No encounters" switch is `--no-live-loop` here, and
/// `F7` at runtime). Both hosts build the shared
/// `legaia_engine_cheats::cheats::PlayerCheat` list and apply it through
/// `WorldCheats::apply_cheat`.
#[derive(Debug, Default, Clone)]
pub struct PlayCheats {
    pub level: Option<u8>,
    pub gold: Option<u32>,
    pub coins: Option<u32>,
    /// `ITEM[:QTY]` specs, resolved against the disc's item names.
    pub items: Vec<String>,
    /// `--cheat-restore`: HP / MP refilled and the fallen revived at scene
    /// entry.
    pub restore: bool,
    pub max_ap: bool,
    pub seru_level: Option<u8>,
    pub arts: bool,
    pub max_items: bool,
    /// `--cheat-noclip`: walk through walls.
    pub noclip: bool,
    /// `--cheat-travel`: the Journal's Travel page lists every pack map.
    pub travel: bool,
}

impl PlayCheats {
    /// The requested cheats as the shared list
    /// ([`legaia_engine_cheats::cheats::PlayerCheat`]) - the values the play
    /// page's Cheats panel builds, in panel order.
    pub fn list(&self) -> Vec<legaia_engine_cheats::cheats::PlayerCheat> {
        use legaia_engine_cheats::cheats::PlayerCheat;
        let mut out = Vec::new();
        out.extend(self.level.map(PlayerCheat::PartyLevel));
        if self.restore {
            out.push(PlayerCheat::Restore);
        }
        if self.max_ap {
            out.push(PlayerCheat::MaxAp);
        }
        out.extend(self.seru_level.map(PlayerCheat::GrantSeru));
        if self.arts {
            out.push(PlayerCheat::LearnAllArts);
        }
        if self.max_items {
            out.push(PlayerCheat::MaxItems);
        }
        if self.noclip {
            out.push(PlayerCheat::WalkThroughWalls(true));
        }
        if self.travel {
            out.push(PlayerCheat::UnlockTravel(true));
        }
        out.extend(self.gold.map(|g| PlayerCheat::Gold(i64::from(g))));
        out.extend(self.coins.map(|c| PlayerCheat::Coins(u64::from(c))));
        out.extend(self.items.iter().map(|s| PlayerCheat::give_item_spec(s)));
        out
    }

    /// Apply every requested cheat to `world` through
    /// `WorldCheats::apply_cheat`, logging each outcome line. `templates` is the
    /// executable's New Game party template, which a level below the current
    /// one is rebuilt from.
    pub(crate) fn apply(
        &self,
        world: &mut legaia_engine_core::world::World,
        templates: Option<&legaia_game_tables::new_game::StartingParty>,
    ) {
        for cheat in self.list() {
            let line = world.apply_cheat(&cheat, templates);
            log::info!("play-window: --cheat-{}: {line}", cheat.key());
        }
    }
}

impl DebugSeeds {
    /// Attach the `--play-record P:R` operand (`partition:record`, decimal).
    pub fn with_play_record(mut self, spec: Option<&str>) -> Result<Self> {
        if let Some(spec) = spec {
            let (p, r) = spec
                .split_once(':')
                .with_context(|| format!("--play-record '{spec}' is not PARTITION:RECORD"))?;
            let p: usize = p
                .trim()
                .parse()
                .with_context(|| format!("--play-record partition '{p}'"))?;
            let r: usize = r
                .trim()
                .parse()
                .with_context(|| format!("--play-record record '{r}'"))?;
            self.play_record = Some((p, r));
        }
        Ok(self)
    }

    /// Attach the `--play-cutscene ID` operand.
    pub fn with_play_cutscene(mut self, id: Option<String>) -> Self {
        self.play_cutscene = id;
        self
    }

    /// Attach the `--expansion` / `--map` / `--spawn` operands (clap makes
    /// `--map` need `--expansion`).
    pub fn with_expansion(
        mut self,
        pack: Option<std::path::PathBuf>,
        map: Option<String>,
        spawn: Option<String>,
    ) -> Result<Self> {
        self.expansion = match (pack, map) {
            (Some(pack), map) => Some(ExpansionLaunch {
                pack,
                map,
                spawn: spawn.unwrap_or_default(),
                watch: false,
            }),
            (None, None) => None,
            (None, Some(_)) => anyhow::bail!("--map needs --expansion"),
        };
        Ok(self)
    }
    /// Attach `--expansion-watch` (clap makes it need `--expansion`).
    pub fn with_expansion_watch(mut self, watch: bool) -> Result<Self> {
        match self.expansion.as_mut() {
            Some(launch) => launch.watch = watch,
            None if watch => anyhow::bail!("--expansion-watch needs --expansion"),
            None => {}
        }
        Ok(self)
    }

    // --- expansion game modes (`--mode`) ---
    /// Attach the `--mode <INSTANCE>` operand.
    pub fn with_pack_mode(mut self, instance: Option<String>) -> Self {
        self.pack_mode = instance;
        self
    }

    /// Attach the `--host` / `--join` operands.
    pub fn with_netplay(mut self, netplay: super::NetLaunch) -> Self {
        self.netplay = netplay;
        self
    }
    // --- end expansion game modes ---

    /// Attach the `--cheat-*` operands.
    pub fn with_cheats(mut self, cheats: PlayCheats) -> Self {
        self.cheats = cheats;
        self
    }

    /// Parse the two repeatable operands. Each accepts decimal or `0x` hex.
    pub fn from_args(
        learn_spell: &[String],
        set_flag: &[String],
        resume_save: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        fn num(s: &str) -> Option<u32> {
            let s = s.trim();
            s.strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .map_or_else(
                    || s.parse::<u32>().ok(),
                    |h| u32::from_str_radix(h, 16).ok(),
                )
        }
        let mut seeds = Self {
            resume_save,
            ..Self::default()
        };
        for s in learn_spell {
            let v = num(s)
                .filter(|v| *v <= u32::from(u8::MAX))
                .with_context(|| format!("--learn-spell '{s}' is not a spell id (0..=0xFF)"))?;
            seeds.learn_spells.push(v as u8);
        }
        for s in set_flag {
            let v = num(s)
                .filter(|v| *v <= u32::from(u16::MAX))
                .with_context(|| format!("--set-flag '{s}' is not a flag index (0..=0xFFFF)"))?;
            seeds.story_flags.push(v as u16);
        }
        Ok(seeds)
    }
}

/// Periodic capture sweep for `play-window`: one PNG (`tick_%05d.png`) into
/// [`Self::dir`] every [`Self::every`] ticks; the run exits after the
/// capture at/past [`Self::last_tick`] (or when the window closes).
pub struct ScreenshotSweep {
    pub dir: std::path::PathBuf,
    pub every: u64,
    pub last_tick: Option<u64>,
}

impl ScreenshotConfig {
    /// `--screenshot-chrome` ([`Self::chrome`]).
    pub fn with_chrome(mut self, chrome: bool) -> Self {
        self.chrome = chrome;
        self
    }

    /// Parse the CLI flags into a config. `None` when none of `--screenshot`,
    /// `--screenshot-every` and `--key-script` is present. Errors on an
    /// unparseable `--pad-script` / `--key-script` entry or a half-specified
    /// sweep.
    ///
    /// `--key-script` alone is enough to build a config: it is the scripted
    /// half of a capture run and dropping it silently when no PNG was asked
    /// for would make a scripted run look like it simply did nothing.
    pub fn from_args(
        path: Option<std::path::PathBuf>,
        capture_tick: u64,
        every: Option<u64>,
        dir: Option<std::path::PathBuf>,
        last_tick: Option<u64>,
        pad_script: Option<&str>,
        key_script: Option<&str>,
    ) -> Result<Option<Self>> {
        let sweep = match (every, dir) {
            (Some(every), Some(dir)) => {
                anyhow::ensure!(every > 0, "--screenshot-every must be > 0");
                Some(ScreenshotSweep {
                    dir,
                    every,
                    last_tick,
                })
            }
            (None, None) => None,
            _ => anyhow::bail!("--screenshot-every and --screenshot-dir must be set together"),
        };
        if path.is_none() && sweep.is_none() && key_script.is_none() {
            return Ok(None);
        }
        let mut script = std::collections::HashMap::new();
        if let Some(spec) = pad_script {
            for entry in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (tick, btn) = entry
                    .split_once(':')
                    .with_context(|| format!("pad-script entry '{entry}' is not TICK:BUTTON"))?;
                let button = legaia_engine_system::input::PadButton::from_name(btn.trim())
                    .with_context(|| format!("pad-script button '{btn}' is not a pad button"))?;
                // `TICK` (one-tick edge) or `FIRST-LAST` (held across the
                // inclusive range - walking somewhere needs a held direction).
                let tick = tick.trim();
                let (first, last) =
                    match tick.split_once('-') {
                        Some((a, b)) => {
                            let a: u64 = a.trim().parse().with_context(|| {
                                format!("pad-script tick '{a}' is not a number")
                            })?;
                            let b: u64 = b.trim().parse().with_context(|| {
                                format!("pad-script tick '{b}' is not a number")
                            })?;
                            anyhow::ensure!(a <= b, "pad-script range '{tick}' is reversed");
                            (a, b)
                        }
                        None => {
                            let t: u64 = tick.parse().with_context(|| {
                                format!("pad-script tick '{tick}' is not a number")
                            })?;
                            (t, t)
                        }
                    };
                for t in first..=last {
                    *script.entry(t).or_insert(0) |= button.mask();
                }
            }
        }
        let mut keys: std::collections::HashMap<u64, Vec<KeyCode>> =
            std::collections::HashMap::new();
        if let Some(spec) = key_script {
            for entry in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (tick, key) = entry
                    .split_once(':')
                    .with_context(|| format!("key-script entry '{entry}' is not TICK:KEY"))?;
                let code = key_from_name(key.trim())
                    .with_context(|| format!("key-script key '{key}' is not a window key name"))?;
                let t: u64 = tick
                    .trim()
                    .parse()
                    .with_context(|| format!("key-script tick '{tick}' is not a number"))?;
                keys.entry(t).or_default().push(code);
            }
        }
        Ok(Some(Self {
            path,
            capture_tick,
            chrome: false,
            sweep,
            pad_script: script,
            key_script: keys,
            talk_at: std::env::var("LEGAIA_CAPTURE_TALK").ok().and_then(|v| {
                let (slot, tick) = v.trim().split_once('@')?;
                Some((tick.trim().parse().ok()?, slot.trim().parse().ok()?))
            }),
            shop_at: std::env::var("LEGAIA_CAPTURE_SHOP").ok().and_then(|v| {
                let (idx, tick) = v.trim().split_once('@')?;
                Some((tick.trim().parse().ok()?, idx.trim().parse().ok()?))
            }),
            hud_countdown: std::env::var("LEGAIA_HUD_COUNTDOWN")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            arrival_gate: std::env::var("LEGAIA_CAPTURE_ARRIVAL")
                .ok()
                .and_then(|v| legaia_engine_capture::arrival_gate::ArrivalGate::from_env(&v)),
            phase_gate: std::env::var("LEGAIA_CAPTURE_GATE")
                .ok()
                .and_then(|v| legaia_engine_capture::phase_gate::PhaseGate::from_env(&v)),
            battle_orbit_yaw: std::env::var("LEGAIA_BATTLE_ORBIT_YAW")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            battle_backdrop_yaw: std::env::var("LEGAIA_BATTLE_BACKDROP_YAW")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            battle_cam_align: std::env::var("LEGAIA_BATTLE_CAM_ALIGN")
                .ok()
                .and_then(|v| legaia_engine_capture::camera_align::cam_align_from_env(&v)),
            cast_rand_seed: std::env::var("LEGAIA_CAST_RAND_SEED")
                .ok()
                .and_then(|v| legaia_engine_capture::cast_rand::cast_rand_seed_from_env(&v)),
            battle_ground_snap: std::env::var("LEGAIA_BATTLE_GROUND_SNAP")
                .map(|v| legaia_engine_capture::ground_snap::ground_snaps_from_env(&v))
                .unwrap_or_default(),
            battle_displayed_seat: std::env::var("LEGAIA_BATTLE_DISPLAYED_SEAT")
                .map(|v| legaia_engine_capture::ground_snap::ground_snaps_from_env(&v))
                .unwrap_or_default(),
            battle_cam_align_displayed: std::env::var("LEGAIA_BATTLE_CAM_ALIGN_DISPLAYED")
                .ok()
                .and_then(|v| legaia_engine_capture::camera_align::cam_align_from_env(&v)),
            battle_module_cam_align: std::env::var("LEGAIA_BATTLE_MODULE_CAM_ALIGN")
                .ok()
                .and_then(|v| legaia_engine_capture::camera_align::cam_align_from_env(&v)),
            battle_summon_focus: std::env::var("LEGAIA_BATTLE_SUMMON_FOCUS")
                .ok()
                .and_then(|v| {
                    let mut it = v.split(',').map(|n| n.trim().parse::<f32>().ok());
                    Some([it.next()??, it.next()??])
                }),
            gate_cam_target: std::cell::Cell::new(None),
            battle_cam_gate_target: std::env::var("LEGAIA_BATTLE_CAM_GATE_TARGET")
                .ok()
                .and_then(|v| legaia_engine_capture::camera_align::cam_pose_from_env(&v)),
            seat_hud_glides_landed: std::env::var_os("LEGAIA_SEAT_HUD_GLIDES_LANDED").is_some(),
            seat_spirit_gauge: std::env::var("LEGAIA_SEAT_SPIRIT_GAUGE")
                .ok()
                .and_then(|v| {
                    let (w, p) = v.split_once(',')?;
                    Some((w.trim().parse().ok()?, p.trim().parse().ok()?))
                }),
            seat_arts_list: std::env::var("LEGAIA_SEAT_ARTS_LIST").ok().and_then(|v| {
                let (page, slide) = v.split_once(',')?;
                Some((page.trim().parse().ok()?, slide.trim().parse().ok()?))
            }),
            seat_hud_glides: std::env::var("LEGAIA_SEAT_HUD_GLIDES")
                .map(|v| legaia_engine_capture::hud_glide::HudGlideSeat::list_from_env(&v))
                .unwrap_or_default(),
            seat_menu_glide_left: std::env::var("LEGAIA_SEAT_MENU_GLIDE_LEFT")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            seat_popup_cells: std::env::var("LEGAIA_SEAT_POPUP_CELLS")
                .map(|v| {
                    v.split(',')
                        .filter_map(|e| {
                            let f: Vec<&str> = e.trim().split(':').collect();
                            Some((
                                f.first()?.parse().ok()?,
                                f.get(1)?.parse().ok()?,
                                f.get(2)?.parse().ok()?,
                                f.get(3)?.parse().ok()?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            popup_cells_due: std::cell::Cell::new(false),
            seat_popup_age: std::env::var("LEGAIA_SEAT_POPUP_AGE").ok().and_then(|v| {
                let (s, a) = v.split_once(':')?;
                Some((s.trim().parse().ok()?, a.trim().parse().ok()?))
            }),
            script_gate: std::env::var("LEGAIA_SCRIPT_GATE")
                .ok()
                .and_then(|v| legaia_engine_capture::script_gate::ScriptGate::from_env(&v)),
            seat_fog: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_FOG")
                    .ok()
                    .map(|v| legaia_engine_capture::field_seeds::fog_from_env(&v)),
            ),
            seat_fog_due: std::cell::Cell::new(false),
            popup_age_seated: std::cell::Cell::new(false),
            seat_fade_lead: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_FADE_LEAD")
                    .ok()
                    .and_then(|v| v.trim().parse().ok()),
            ),
            seat_sprite_arms: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_SPRITE_ARMS")
                    .ok()
                    .map(|v| legaia_engine_capture::field_seeds::sprite_arms_from_env(&v)),
            ),
            seat_balloon: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_BALLOON")
                    .ok()
                    .and_then(|v| legaia_engine_capture::field_seeds::balloon_from_env(&v)),
            ),
            seat_turning_parts: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_TURNING_PARTS")
                    .ok()
                    .map(|v| legaia_engine_capture::field_seeds::turning_parts_from_env(&v)),
            ),
            seat_clear: std::env::var("LEGAIA_SEAT_CLEAR").ok().and_then(|v| {
                let c: Vec<u8> = v.split(',').filter_map(|e| e.trim().parse().ok()).collect();
                <[u8; 3]>::try_from(c).ok()
            }),
            seat_curve_bias: std::env::var("LEGAIA_SEAT_CURVE_BIAS")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            seat_page_mark: std::env::var("LEGAIA_SEAT_PAGE_MARK")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            seat_panel: std::env::var("LEGAIA_SEAT_PANEL")
                .ok()
                .and_then(|v| legaia_engine_capture::field_seeds::panel_from_env(&v)),
            seat_walkers: std::env::var("LEGAIA_SEAT_WALKERS")
                .map(|v| legaia_engine_capture::field_seeds::walkers_from_env(&v))
                .unwrap_or_default(),
            seat_placements: std::env::var("LEGAIA_SEAT_PLACEMENTS")
                .map(|v| legaia_engine_capture::field_seeds::walkers_from_env(&v))
                .unwrap_or_default(),
            seat_morphs: std::env::var("LEGAIA_SEAT_MORPHS")
                .map(|v| legaia_engine_capture::field_seeds::morphs_from_env(&v))
                .unwrap_or_default(),
            seat_object_clips: std::env::var("LEGAIA_SEAT_OBJECT_CLIPS")
                .map(|v| legaia_engine_capture::field_seeds::object_clips_from_env(&v))
                .unwrap_or_default(),
            seat_object_models: std::env::var("LEGAIA_SEAT_OBJECT_MODELS")
                .map(|v| {
                    v.split(',')
                        .filter_map(|e| {
                            let (r, m) = e.split_once(':')?;
                            Some((r.trim().parse().ok()?, m.trim().parse().ok()?))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            seat_latches: std::env::var("LEGAIA_SEAT_LATCHES")
                .map(|v| {
                    v.split(',')
                        .filter_map(|i| u16::from_str_radix(i.trim(), 16).ok())
                        .collect()
                })
                .unwrap_or_default(),
            battle_drive: std::env::var("LEGAIA_BATTLE_DRIVE")
                .ok()
                .and_then(|v| legaia_engine_capture::battle_drive::BattleDrive::from_env(&v)),
            battle_drive_clock: std::cell::Cell::new(Default::default()),
            battle_drive_held: std::cell::Cell::new(0),
            display_lag: std::env::var("LEGAIA_CAPTURE_DISPLAY_LAG")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0),
            retake_shot_only: std::env::var("LEGAIA_CAPTURE_RETAKE_SHOT")
                .is_ok_and(|v| v.trim() == "1"),
            state_since: std::cell::Cell::new(None),
            battle_reseat: std::env::var("LEGAIA_BATTLE_RESEAT")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            battle_approach_yaw: std::env::var("LEGAIA_BATTLE_APPROACH_YAW")
                .ok()
                .and_then(|v| {
                    legaia_engine_battle_loop::battle_state::CaptureApproach::from_env(&v)
                }),
            battle_backdrop_cue: std::env::var("LEGAIA_BATTLE_BACKDROP_CUE")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            battle_bars: std::env::var("LEGAIA_BATTLE_BARS")
                .map(|v| legaia_engine_capture::bar_seeds::bar_seeds_from_env(&v))
                .unwrap_or_default(),
            battle_bars_seeded: std::cell::Cell::new(false),
            seat_battle_clips: std::env::var("LEGAIA_SEAT_BATTLE_CLIPS")
                .map(|v| legaia_engine_capture::clip_cursors::clip_cursors_from_env(&v))
                .unwrap_or_default(),
            battle_melee_hits: std::env::var("LEGAIA_BATTLE_MELEE_HITS")
                .map(|v| legaia_engine_capture::melee_hits::melee_hits_from_env(&v))
                .unwrap_or_default(),
            mystic_shield: std::cell::Cell::new(
                std::env::var("LEGAIA_BATTLE_MYSTIC_SHIELD").is_ok_and(|v| v.trim() == "1"),
            ),
            seat_cast_parts: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_CAST_PARTS")
                    .ok()
                    .map(|v| legaia_engine_capture::cast_parts::cast_parts_from_env(&v)),
            ),
            seat_action_fx_parts: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_ACTION_FX_PARTS")
                    .ok()
                    .map(|v| legaia_engine_capture::cast_parts::cast_parts_from_env(&v)),
            ),
            seat_effect_children: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_EFFECT_CHILDREN")
                    .ok()
                    .map(|v| legaia_engine_capture::effect_children::effect_children_from_env(&v)),
            ),
            seat_ghost_ring: std::cell::Cell::new(
                std::env::var("LEGAIA_SEAT_GHOST_RING")
                    .ok()
                    .map(|v| legaia_engine_capture::ghost_ring::ghost_ring_from_env(&v)),
            ),
        }))
    }
}

/// Window key name -> winit `KeyCode`, the inverse of [`keycode_to_name`] over
/// the whole physical-key surface `play-window` listens on (not just the
/// pad-bindable subset that function covers).
///
/// Names are case-insensitive. Every dev / entry key the window's keyboard
/// handler tests for is a plain letter, so the letter rows are what make
/// `--key-script` able to open a minigame at all.
pub(super) fn key_from_name(name: &str) -> Option<KeyCode> {
    Some(match name.trim().to_ascii_uppercase().as_str() {
        "UP" => KeyCode::ArrowUp,
        "DOWN" => KeyCode::ArrowDown,
        "LEFT" => KeyCode::ArrowLeft,
        "RIGHT" => KeyCode::ArrowRight,
        "ENTER" | "RETURN" => KeyCode::Enter,
        "SPACE" => KeyCode::Space,
        "RSHIFT" => KeyCode::ShiftRight,
        "A" => KeyCode::KeyA,
        "B" => KeyCode::KeyB,
        "C" => KeyCode::KeyC,
        "D" => KeyCode::KeyD,
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "I" => KeyCode::KeyI,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "M" => KeyCode::KeyM,
        "N" => KeyCode::KeyN,
        "O" => KeyCode::KeyO,
        "P" => KeyCode::KeyP,
        "Q" => KeyCode::KeyQ,
        "R" => KeyCode::KeyR,
        "S" => KeyCode::KeyS,
        "T" => KeyCode::KeyT,
        "U" => KeyCode::KeyU,
        "V" => KeyCode::KeyV,
        // Window-toggle band. F-keys can never collide with a pad binding,
        // which is why the three arms that used to sit on `E` / `V` / `C`
        // live here (see `keycode_to_name`).
        // Every F-key the keyboard handler has an arm for, so the
        // enhancement toggles (`F4` occlusion fade, `F8` time of day, `F9`
        // ground fog) and the cheats (`F6` / `F7`) are scriptable like the
        // rest - a capture comparing a toggle against the play page's
        // checkbox needs the native half reachable from a script.
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        // `F10` bloom.
        "F10" => KeyCode::F10,
        // The hotkey help panel (`help_key`): `F12`, or the `?` key.
        "F12" => KeyCode::F12,
        // The pack reload (`window::pack_reload`).
        "END" => KeyCode::End,
        "/" | "?" | "SLASH" => KeyCode::Slash,
        // The multiplayer room screen (`window::netplay`).
        "TAB" => KeyCode::Tab,
        // The expansion game-mode picker (`` ` ``) and the card menu (`\`).
        "`" | "BACKQUOTE" => KeyCode::Backquote,
        "\\" | "BACKSLASH" => KeyCode::Backslash,
        "W" => KeyCode::KeyW,
        "X" => KeyCode::KeyX,
        "Y" => KeyCode::KeyY,
        "Z" => KeyCode::KeyZ,
        "0" => KeyCode::Digit0,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "3" => KeyCode::Digit3,
        "4" => KeyCode::Digit4,
        "5" => KeyCode::Digit5,
        "6" => KeyCode::Digit6,
        "7" => KeyCode::Digit7,
        "8" => KeyCode::Digit8,
        "9" => KeyCode::Digit9,
        _ => return None,
    })
}

/// Write a [`CaptureImage`] (RGBA8, row-major) to a PNG file. Used by the
/// `--screenshot` harness in the redraw path.
pub fn write_capture_png(path: &Path, img: &CaptureImage) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating screenshot dir {}", dir.display()))?;
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("creating screenshot {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .context("png header")?
        .write_image_data(&img.rgba)
        .context("png data")?;
    Ok(())
}

/// Map a winit `KeyCode` to the user-friendly key name used in
/// [`legaia_engine_system::input::Mapping`]. Returns `""` for keys outside
/// the default set.
/// Physical key -> the key **name** the engine's binding table is keyed by
/// ([`legaia_engine_system::input::Mapping`]).
///
/// This function is a filter, and it was silently narrower than the table it
/// feeds: `C`, `V`, `D` and `E` are bound (Triangle, Square, Right, R1) and
/// none of them was listed, so `pad_button_for_key` got `""` and the native
/// window could not press **Triangle or Square at all**. Nothing surfaced
/// that - a key that resolves to no button is indistinguishable from a key
/// the player did not press - and it takes two of the four face buttons off
/// the keyboard, which is most of the dance minigame's input and the
/// three-actor talk's leader switch.
///
/// Every name here must appear in `Mapping::web_default`, and every key that
/// table binds must appear here. Two of the four repaired rows are still
/// shadowed *upstream*, by a window key arm in `event_handler/keyboard.rs`
/// that returns before the pad lookup: `C` (camera vantage) and `D`
/// (occlusion fade, named in the user guide). Moving those two arms onto
/// non-pad keys is what finishes the set.
pub(super) fn keycode_to_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyC => "C",
        KeyCode::KeyV => "V",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyD => "D",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyW => "W",
        KeyCode::KeyE => "E",
        KeyCode::Enter => "Enter",
        KeyCode::Space => "Space",
        KeyCode::ShiftRight => "RShift",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        _ => "",
    }
}

#[cfg(test)]
mod cheat_list_host_tests {
    /// Every row of the shared cheat list has a control on both hosts: a
    /// `--cheat-<key>` flag here and a `cheat(rt, '<key>'` call on the play
    /// page. "No encounters" is the one switch each host already had under
    /// another name (`--no-live-loop` / `F7`, the page's checkbox).
    #[test]
    fn every_listed_cheat_has_a_control_on_both_hosts() {
        let cli = include_str!("../bin/legaia-engine/cli.rs");
        let page = include_str!("../../../../site/_content/play.html");
        for (key, _) in legaia_engine_cheats::cheats::PLAYER_CHEAT_LIST {
            if key == "encounters" {
                assert!(cli.contains("no_live_loop"));
                assert!(page.contains("play-cheat-no-encounters"));
                continue;
            }
            let flag = format!("cheat_{}:", key.replace('-', "_"));
            assert!(cli.contains(&flag), "native flag for cheat '{key}'");
            let call = format!("cheat(rt, '{key}'");
            assert!(page.contains(&call), "play page control for cheat '{key}'");
        }
        assert!(page.contains("cheat_apply_codes"), "page codes box");
        assert!(cli.contains("cheat_file:"), "native --cheat-file");
    }
}

#[cfg(test)]
mod key_script_tests {
    use super::{ScreenshotConfig, key_from_name};
    use winit::keyboard::KeyCode;

    /// Every key the window's own handler tests for has to be nameable, or
    /// `--key-script` silently cannot reach that arm.
    ///
    /// The minigame entries are the reason this exists: they are the only way
    /// into the dance / fishing / casino / dome / duel surfaces, `--pad-script`
    /// cannot open any of them, and a missing name here fails as "the script
    /// parsed but nothing happened" rather than as an error.
    #[test]
    fn every_minigame_entry_key_is_nameable() {
        for (name, want) in [
            ("K", KeyCode::KeyK),
            ("U", KeyCode::KeyU),
            ("L", KeyCode::KeyL),
            ("P", KeyCode::KeyP),
            ("O", KeyCode::KeyO),
            ("M", KeyCode::KeyM),
            ("B", KeyCode::KeyB),
            ("Down", KeyCode::ArrowDown),
            ("Enter", KeyCode::Enter),
        ] {
            assert_eq!(key_from_name(name), Some(want), "key name '{name}'");
            assert_eq!(
                key_from_name(&name.to_lowercase()),
                Some(want),
                "key names are case-insensitive"
            );
        }
        assert_eq!(key_from_name("NotAKey"), None);
    }

    /// Every `KeyCode` the keyboard handler has an arm for is reachable from
    /// `--key-script`, read off the handler's own source so a new arm is
    /// covered the day it lands.
    ///
    /// The F-key band was the hole: `F4` / `F8` / `F9` - the occlusion fade,
    /// the time of day and the ground fog, the three enhancement toggles a
    /// side-by-side against the play page's checkboxes has to flip - had arms
    /// and no names, so a script could not drive the native half of the
    /// comparison at all.
    #[test]
    fn every_handler_key_is_nameable() {
        // The handler is `handle_key` plus its arms, one file per key family.
        let src = [
            include_str!("event_handler/keyboard.rs"),
            include_str!("event_handler/keyboard/debug_spawn_keys.rs"),
            include_str!("event_handler/keyboard/minigame_keys.rs"),
            include_str!("event_handler/keyboard/option_keys.rs"),
        ]
        .concat();
        let mut names: Vec<String> = ["Up", "Down", "Left", "Right", "Enter", "Space", "RShift"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        names.extend((b'A'..=b'Z').map(|c| (c as char).to_string()));
        names.extend((0..=9).map(|d| d.to_string()));
        names.extend((1..=12).map(|f| format!("F{f}")));
        names.push("/".to_string());
        names.push("Tab".to_string());
        names.push("End".to_string());
        names.push("`".to_string());
        names.push("\\".to_string());
        let reachable: std::collections::BTreeSet<String> = names
            .iter()
            .filter_map(|n| key_from_name(n))
            .map(|k| format!("{k:?}"))
            .collect();
        let mut handled = std::collections::BTreeSet::new();
        for (i, _) in src.match_indices("KeyCode::") {
            let tail = &src[i + "KeyCode::".len()..];
            let end = tail
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(tail.len());
            handled.insert(tail[..end].to_string());
        }
        // The option keys are not spelt as `KeyCode::*` any more: the window
        // resolves them through the shared hotkey table, whose codes are the
        // `KeyCode` variant names.
        for (_, code) in legaia_engine_session::hotkeys::HOST_HOTKEYS {
            handled.insert(code.to_string());
        }
        // `Escape` quits through the event loop (`handle_keyboard`), which a
        // script deliberately never reaches; `--screenshot-last-tick` ends a
        // scripted run instead.
        handled.remove("Escape");
        // So does the fullscreen toggle (`fullscreen_key`: `F11`, or
        // `Alt+Enter` incl. the keypad's): window chrome a capture must never
        // flip, resolved ahead of `handle_key`.
        handled.remove("F11");
        handled.remove("NumpadEnter");
        assert!(
            handled.len() >= 20,
            "the handler reads its keys as `KeyCode::*`"
        );
        let missing: Vec<&String> = handled.iter().filter(|k| !reachable.contains(*k)).collect();
        assert!(
            missing.is_empty(),
            "handler keys `--key-script` cannot name: {missing:?}"
        );
    }

    /// Every key the engine's default pad table binds must survive
    /// `keycode_to_name`, or the native window cannot press that button.
    ///
    /// The filter was narrower than the table and nothing said so: a key that
    /// resolves to no button looks exactly like a key nobody pressed, so
    /// **Triangle and Square were unreachable from the keyboard** while every
    /// gate stayed green. The assertion runs in the direction that catches
    /// that - from the table to the filter, not the reverse - so adding a
    /// binding without a name fails here rather than in a player's hands.
    #[test]
    fn every_bound_key_survives_the_window_name_filter() {
        use legaia_engine_system::input::Mapping;
        let mapping = Mapping::web_default();
        let mut checked = 0;
        let mut names: Vec<&String> = mapping.bindings.keys().collect();
        names.sort();
        for name in names {
            let Some(code) = super::key_from_name(name) else {
                panic!("bound key '{name}' has no KeyCode - `--key-script` cannot reach it");
            };
            assert_eq!(
                super::keycode_to_name(code),
                name.as_str(),
                "bound key '{name}' does not survive keycode_to_name - the native window \
                 resolves it to no pad button"
            );
            checked += 1;
        }
        assert!(checked >= 16, "the default table binds at least 16 keys");
    }

    /// `--key-script` alone builds a config. Returning `None` unless a PNG was
    /// also asked for would drop the whole script, and a scripted run that
    /// dropped its script looks exactly like one that had nothing to do.
    #[test]
    fn a_key_script_alone_is_enough_to_build_a_config() {
        let cfg = ScreenshotConfig::from_args(None, 0, None, None, None, None, Some("40:L,40:P"))
            .expect("parses")
            .expect("a key script alone builds a config");
        assert_eq!(
            cfg.key_script.get(&40).map(Vec::as_slice),
            Some([KeyCode::KeyL, KeyCode::KeyP].as_slice()),
            "same-tick keys keep their listed order"
        );
        assert!(
            ScreenshotConfig::from_args(None, 0, None, None, None, None, None)
                .expect("parses")
                .is_none(),
            "no capture and no script is still no config"
        );
    }

    /// A capture leaves the window's timed chrome out unless
    /// `--screenshot-chrome` asks for it.
    #[test]
    fn captures_leave_timed_chrome_out_by_default() {
        let cfg = ScreenshotConfig::from_args(
            Some(std::path::PathBuf::from("x.png")),
            60,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("parses")
        .expect("a capture builds a config");
        assert!(!cfg.chrome);
        assert!(cfg.with_chrome(true).chrome);
    }

    /// An unparseable entry is an error, not a silently empty script.
    #[test]
    fn a_bad_key_script_entry_errors() {
        for bad in ["40", "40:NotAKey", "x:L"] {
            assert!(
                ScreenshotConfig::from_args(None, 0, None, None, None, None, Some(bad)).is_err(),
                "'{bad}' must not parse"
            );
        }
    }
}

#[cfg(test)]
mod key_binding_tests {
    use super::keycode_to_name;
    use legaia_engine_system::input::{Mapping, PadButton};
    use winit::keyboard::KeyCode;

    /// Start answers to Enter and Space on the desktop build too.
    ///
    /// The native path has two halves that can disagree silently: this file
    /// turns a winit `KeyCode` into a key *name*, and `Mapping` turns that name
    /// into a button. A name missing from `keycode_to_name` resolves to `""`,
    /// which binds to nothing - so the binding can be correct in the shared
    /// table and still dead in the window, with neither file looking wrong.
    #[test]
    fn start_answers_to_both_enter_and_space() {
        let m = Mapping::default();
        for code in [KeyCode::Enter, KeyCode::Space] {
            let name = keycode_to_name(code);
            assert!(!name.is_empty(), "{code:?} must resolve to a key name");
            assert_eq!(
                m.pad_button_for_key(name),
                Some(PadButton::Start),
                "{code:?} -> {name} must be Start"
            );
        }
    }
}
