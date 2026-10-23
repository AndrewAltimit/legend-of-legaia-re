//! The engine side of a comparison: seed a booted session from a
//! [`RetailObs`], settle it, and sample the [`EngineObs`] the scorers read.

use super::*;
use legaia_engine_save_state::WorldSaveState;

/// What the engine shows after seeding.
pub struct EngineObs {
    pub scene: Option<String>,
    pub mode: SceneMode,
    pub player: Option<[i16; 3]>,
    /// The engine's floor sample under retail's own `(X, Z)`.
    pub floor_at_retail: Option<i32>,
    pub camera: CameraObs,
    /// The engine's track-select word after the settle window
    /// ([`legaia_engine_scene_host::scene_host::SceneHost::bgm_track_word`]), else the
    /// last track the director started.
    pub bgm_id: Option<u16>,
    /// A control op stopped or paused the track after its last start.
    pub bgm_held: bool,
    /// The engine's fog-pool gate (`World::fog.gate`).
    pub fog_gate: bool,
    /// The zone camera's visible-tile window (`ZoneFollow::view_window`).
    pub view_window: [i8; 4],
    /// The same window as the decoration pass holds it between its clamp and
    /// its epilogue (`field_view_window::view_cells`, `FUN_801F7088` steps
    /// 1-2): clipped against the walk-region box around the focus. `None`
    /// before the camera has published a cull view.
    pub view_window_in_pass: Option<[i8; 4]>,
    pub save: legaia_save::SaveFile,
    /// On a menu-class seed: the sub-screen the engine's pause menu reached
    /// (the open row's retail id, `0x01` on the root list, `0x13` for the
    /// Equip candidate step), `None` when no menu opened.
    pub menu_subscreen: Option<u8>,
    /// On a capture inside a running script: the phase gate's outcome.
    pub script: Option<ScriptPhase>,
    /// The player's heading in retail space (`render_26 - 0x800`).
    pub player_facing: Option<i16>,
    /// Each partition-1 placement's heading (retail space) and position,
    /// keyed by its retail flat MAN index (`N0 + slot`).
    pub npc_facings: BTreeMap<u16, (i16, i16, i16)>,
}

/// Records which track the field VM starts.
#[derive(Default)]
pub(crate) struct RecordingDirector {
    pub(crate) last: Option<u16>,
    /// A control op has stopped or paused the track since the last start.
    pub(crate) held: bool,
}

impl BgmDirector for RecordingDirector {
    fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
        self.last = Some(bgm_id);
        self.held = false;
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.last = Some(bgm_id);
        self.held = false;
    }
    // The comparand is the id the scripts selected (retail's track-select
    // word is written by the op-0x35 start arms). A control op starts no
    // track, so it keeps `last` and only moves `held`.
    fn pause(&mut self) {
        self.held = true;
    }
    fn resume(&mut self) {
        self.held = false;
    }
    fn stop(&mut self) {
        self.held = true;
    }
    fn unhalt_pause(&mut self) {
        self.held = false;
    }
}

/// Seed the engine from a retail state and sample it after [`SETTLE_TICKS`].
/// How the retail save is applied around the scene entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SeedOrder {
    /// The engine's own card-load path ([`NativeSession::resume_save`]): seed
    /// the story flags, land the scene, then hydrate the whole save. The
    /// entry scripts see the saved flags but not the saved party, bag or
    /// gold.
    #[default]
    Resume,
    /// Hydrate the whole save, enter through `enter_scene_live` (no resume
    /// landing, no saved seat), hydrate again. A diagnostic arm - the
    /// difference between the two orders is what the landing and the rest
    /// of the save (party, bag, gold) contribute ahead of the entry.
    FlagsFirst,
}

/// Seed through the engine's own card-load order ([`SeedOrder::Resume`]).
pub fn run_engine(extracted: &Path, retail: &RetailObs) -> Result<EngineObs> {
    run_engine_with(extracted, retail, SeedOrder::Resume)
}

/// Seed the engine from a retail state in the given order and sample it
/// after [`SETTLE_TICKS`].
pub fn run_engine_with(
    extracted: &Path,
    retail: &RetailObs,
    order: SeedOrder,
) -> Result<EngineObs> {
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).context("open boot session")?;
    // The image child's `--retail` mode (`pin_seed_retail`). No field-side
    // switch draws differently under a resident patch.
    crate::retail_compare::pin_seed_retail(&mut session.host.world.toggles, false);
    // Field talk runs on the inline field-VM runner, as it does in both play
    // hosts (`play-window`'s default and the browser page) and in the image
    // child: without it an engaged placement opens the plain typewriter panel
    // and its record never advances, so a capture inside a conversation
    // could not be reached headlessly while the child reached it.
    session.host.world.toggles.use_vm_dialogue = true;
    let opts = FieldLiveOpts::default();
    let save = retail
        .seed_save()
        .context("retail state has no liftable save window")?;
    match order {
        // The engine's own card-load path: flags, landing, whole save.
        SeedOrder::Resume => {
            let _landing = session.resume_save(save, &retail.scene, &opts);
        }
        SeedOrder::FlagsFirst => {
            session.host.world.load_full(save.clone());
            session.enter_scene_live(&retail.scene, &opts)?;
            session.host.world.load_full(save);
        }
    }
    // Retail's measured frame step is load history (`World::seed_frame_step`).
    session.host.world.seed_frame_step(retail.frame_step());
    let mut director = RecordingDirector::default();
    // The entry itself may already have queued a start.
    session.host.route_bgm_events(&mut director)?;
    // A non-zero arrival facing is a door's (the card load the seed takes
    // zeroes it): history the seed cannot replay, so it is seated like the
    // position, and the player stands as the state's entry stood it.
    if retail.arrival_facing != 0 {
        session.host.world.locomotion.arrival_facing = retail.arrival_facing;
        session.host.world.apply_arrival_facing();
    }
    if let Some([x, _, z]) = retail.player
        && session.host.debug_seat_standing(x, z)
    {
        // A heading the pad turned is walk history exactly as the position
        // is: stand the player in retail's, as the arrival facing so an
        // entry script's `4C 3A` in the settle window hands over the same
        // heading. One still on retail's arrival facing is left to the
        // engine's own entry, which the facing channel scores.
        if let Some(h) = retail.seat_heading() {
            session.host.world.locomotion.arrival_facing = h;
            session.host.world.apply_arrival_facing();
        }
        match retail.camera_block {
            Some(block) => session.camera.zone.arm_arrival_over(block),
            None => session.camera.zone.arm_arrival(),
        }
        // A focus retail left behind (a poked or script-carried player the
        // follow ease never ran for) is walk history like the block: the
        // snap lands it, as the image child's `LEGAIA_SEAT_FOCUS` does, and
        // the engine's own ease leaves it until the player moves.
        if let Some(focus) = retail.seat_focus {
            session.camera.zone.seat_focus_after_snap(focus);
        }
    }
    // A capture inside a running script is compared at the script's phase,
    // not after a fixed window ([`legaia_engine_capture::script_gate::ScriptGate`]):
    // the session runs until its own context for the record holds retail's
    // PC and wait, up to a deadline. The settle-window sample is kept as the
    // fallback for a gate the engine never meets.
    let gate = (retail.menu.is_none()
        && matches!(retail.class, StateClass::Field | StateClass::WorldMap))
    .then(|| crate::retail_compare_script::script_gate_from_retail(&retail.scripts))
    .flatten();
    let deadline = if gate.is_some() {
        legaia_engine_capture::script_gate::SCRIPT_GATE_DEADLINE
    } else {
        SETTLE_TICKS
    };
    let mut at_settle = None;
    let mut met_at = None;
    let mut resumed = false;
    if let Some(g) = &gate
        && std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some()
    {
        eprintln!(
            "script gate {}: flat index {} pc {} op {:#04x} wait {} glide left {:?}",
            retail.scene, g.flat_index, g.pc, g.op, g.wait, g.glide_left
        );
    }
    for t in 1..=deadline {
        if let Some(g) = &gate {
            let pad = g.advance_pad(&session.host.world, t);
            session.host.world.input.set_pad(pad);
        }
        hold_slot_table(&mut session, retail);
        hold_rand_seed(&mut session, retail);
        session.tick()?;
        // The scripted VRAM effects, as both hosts drain them each tick
        // (`BootSession::field_vram_tick`): a cross-context CLUT fade left
        // unstepped keeps its context halted for the whole run.
        session.field_vram_tick();
        session.host.route_bgm_events(&mut director)?;
        if let Some(g) = &gate {
            if std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some()
                && (t % 25 == 0
                    || t < 5
                    || std::env::var_os("LEGAIA_RC_SCRIPT_TRACE_ALL").is_some())
            {
                eprintln!(
                    "script gate t={t}: sys pc {} (retail {:?}) {}",
                    session.host.world.field_pc,
                    retail.scripts.system_pc,
                    g.trace(&session.host.world)
                );
            }
            if g.met(&session.host.world) {
                if std::env::var_os("LEGAIA_RC_SCRIPT_TRACE").is_some() {
                    eprintln!("script gate met t={t}: {}", g.trace(&session.host.world));
                }
                met_at = Some(t);
                break;
            }
            if t == legaia_engine_capture::script_gate::SCRIPT_RESUME_TICK && !g.resumed_early.get()
            {
                // The record's own latches, held back from the entry, are
                // retail's state again once the entry has run - the
                // settle-window sample included; a resume takes them back
                // before it replays the record.
                for idx in retail.seed_latches() {
                    session.host.world.system_flag_set(idx);
                }
            }
            if t == SETTLE_TICKS {
                at_settle = Some(sample_engine(&mut session, retail, &director, None));
            }
            // The walkers stand where retail's left them before a talk is
            // engaged: the talk snap turns a placement to the bearing from
            // its seat, and a wanderer's seat is `rand()` history
            // (`town01_npc16_dialogue_first_page`'s `P1[16]` wandered off
            // its `4C 51` tile before the press).
            if t == legaia_engine_capture::script_gate::SCRIPT_RESUME_TICK {
                for w in &retail.walkers {
                    session
                        .host
                        .world
                        .seed_ambient_walker(w.flat, w.x, w.z, w.heading);
                }
            }
            if g.drive_resume(&mut session.host, t) {
                resumed = true;
            }
        }
    }
    session.host.world.input.set_pad(0);
    let script = gate.as_ref().map(|g| ScriptPhase {
        pc: g.pc,
        wait: g.wait,
        met_at,
        resumed,
    });
    if let (Some(_), None, Some(mut obs)) = (&gate, met_at, at_settle) {
        obs.script = script;
        return Ok(obs);
    }
    let menu_subscreen = match &retail.menu {
        Some(Ok(menu)) => drive_pause_menu(&mut session, menu)?,
        _ => None,
    };
    let mut obs = sample_engine(&mut session, retail, &director, menu_subscreen);
    obs.script = script;
    Ok(obs)
}

/// How a script-gated seed went: retail's phase and the tick the engine
/// reached it (`None`: not within the deadline, so the settle-window sample
/// stands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPhase {
    pub pc: usize,
    pub wait: i16,
    pub met_at: Option<u64>,
    /// The record was not running in the engine and was started from its
    /// first opcode ([`legaia_engine_capture::script_gate::resume_record`]).
    pub resumed: bool,
}

pub(super) fn sample_engine(
    session: &mut NativeSession,
    retail: &RetailObs,
    director: &RecordingDirector,
    menu_subscreen: Option<u8>,
) -> EngineObs {
    let world = &mut session.host.world;
    let player = world.player_actor_slot.and_then(|s| {
        world.actors.get(s as usize).map(|a| {
            [
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            ]
        })
    });
    let floor_at_retail = retail
        .player
        .map(|[x, _, z]| world.sample_field_floor_height(i32::from(x), i32::from(z)));
    let g = &session.camera.globals.0;
    let camera = CameraObs {
        pitch: g[0] as i16,
        yaw: g[1] as i16,
        h: g[9] as i16,
        eye: [g[3], g[4], g[5]],
        focus: [g[6], g[8]],
    };
    let mode = world.mode;
    let fog_gate = world.fog.gate;
    let save = world.save_full();
    let scene = session.host.scene.as_ref().map(|s| s.name.clone());
    // The engine's `_DAT_8007BAC8`: a park-sentinel start (`0x1000`, the
    // ending scenes') reaches no director, but it is the word retail holds.
    let bgm_id = session.host.bgm_track_word.or(director.last);
    let player_facing = world.player_actor_slot.and_then(|s| {
        world
            .actors
            .get(s as usize)
            .map(|a| a.move_state.render_26.wrapping_sub(0x800))
    });
    let n0 = session
        .host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&session.host.index).ok().flatten())
        .and_then(|man| legaia_scene_formats::man_section::parse(&man).ok())
        .map(|mf| mf.header.partition_counts[0].max(0) as u16);
    let world = &session.host.world;
    let npc_facings = n0
        .map(|n0| {
            world
                .npcs
                .positions
                .iter()
                .map(|(&slot, &(x, z))| {
                    let h = world.npcs.heading(slot);
                    (n0 + u16::from(slot), (h.wrapping_sub(0x800), x, z))
                })
                .collect()
        })
        .unwrap_or_default();
    EngineObs {
        scene,
        mode,
        player,
        floor_at_retail,
        camera,
        bgm_id,
        bgm_held: director.held,
        fog_gate,
        view_window: session.camera.zone.view_window,
        view_window_in_pass: session
            .host
            .world
            .npcs
            .cull_view
            .as_ref()
            .map(|v| legaia_engine_scene::field_view_window::view_cells(v).window),
        save,
        menu_subscreen,
        script: None,
        player_facing,
        npc_facings,
    }
}
