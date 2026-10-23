//! The engine side of a battle comparison: enter the same fight through the
//! real encounter path, seed it, drive it to the capture's phase and sample
//! the [`EngineBattle`].

use super::*;
use legaia_engine_save_state::WorldSaveState;

/// What the engine shows after the battle seed.
pub struct EngineBattle {
    /// The mid-fight HP / MP this run seeded on its first battle tick
    /// ([`apply_bar_seeds`]); the image child seeds the same bars
    /// (`LEGAIA_BATTLE_BARS`).
    pub hp_seed: Vec<BarSeed>,
    pub scene: Option<String>,
    pub mode: SceneMode,
    /// How the formation was reached: `man row N` or `synthesized`.
    pub formation_source: String,
    /// The scene's MAN formation row the fight was entered through, when one
    /// carries the retail cell (what `play-window --battle` can name).
    pub man_row: Option<u16>,
    /// Ticks from the battle-mode flip to the first command prompt; `None`
    /// when the opening never reached one.
    pub prompt_tick: Option<u32>,
    /// `Some` when the capture's cast was replayed: the ticks from the
    /// dispatch to the capture's phase, `None` inside when the engine never
    /// reached it.
    pub inflight: Option<Option<u32>>,
    /// `Some` when the capture's menu state or in-flight action was driven
    /// to through the pad path ([`SeedPlan::Menu`] / [`SeedPlan::Action`]):
    /// the ticks from the first prompt to the capture's phase, `None` inside
    /// when the engine never reached it.
    pub driven: Option<Option<u32>>,
    /// The world stream's state at the encounter entry
    /// ([`BATTLE_RNG_SEEDS`]).
    pub rng_seed: u32,
    /// The world stream's state where the run stopped.
    pub rng_at_end: u32,
    /// The formation advantage the entry rolled (`ctx+0x290`) was a back
    /// attack or a pre-emptive strike. A capture of a running fight is not
    /// its opening round, so such an opening hands one side a round - the
    /// monsters' swings on the party, or the party's on the monsters - that
    /// is not in the history the capture's HP / MP were read from
    /// ([`RetailBattle::seed_plan`] other than the opening).
    pub surprise_opening: bool,
    /// For a [`SpanGate::Age`] drive whose state the engine left before it
    /// was as old as retail's: the engine accumulator on its last tick
    /// there, which a re-run gates on instead (and which the re-run's own
    /// result carries as the gate it was sampled on).
    pub age_short: Option<u16>,
    /// The win pose the engine's battle-end sequence picked, when it runs.
    pub win_pose: Option<u8>,
    /// The party melee hits a driven action pushed onto the damage ring
    /// between the drive's start and its sample
    /// (`BattleState::party_melee_pushes`); `None` without a drive.
    pub action_pushes: Option<u32>,
    /// The engine's action-SM state when sampled.
    pub action_state: u8,
    /// The engine's active seat `ctx[+0x13]` when sampled.
    pub active_actor: u8,
    /// The active seat's target byte (`+0x1DD`) when sampled.
    pub active_target: u8,
    pub monster_ids: Vec<Option<u16>>,
    pub party: Vec<Combatant>,
    pub monsters: Vec<Combatant>,
    pub flow: BattleFlowState,
    pub camera: CameraObs,
    pub bgm_id: Option<u16>,
    /// The engine's track word and `World::audio.current_bgm` just before
    /// the encounter was forced.
    pub field_word: Option<u16>,
    pub field_current: Option<u16>,
    /// The track the engine plays during the fight.
    pub battle_bgm: Option<u16>,
    pub save: legaia_save::SaveFile,
    /// Per engine slot, how far the replay moved a combatant the seed
    /// placed (`[x, z]`, the phase's ground pair less the seeded one), for
    /// a driven action that reached its phase ([`RetailBattle::undrift`]).
    pub ground_drift: [Option<[i32; 2]>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
    /// Per engine slot, how far (sixteenths of a keyframe) a replayed cast's
    /// idle loop advanced between the dispatch and the capture's phase -
    /// `None` for a slot that left its idle loop on the way
    /// ([`RetailBattle::with_inflight_idle`]).
    pub idle_advance: [Option<u32>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
    /// The camera's phase, glide target and origin-alignment mask set
    /// against the capture's tween endpoints - a diagnostic the `camera`
    /// detail carries under `LEGAIA_RC_CAM_TRACE`.
    pub cam_trace: Option<String>,
}

/// The smallest replay drift, on either axis, [`RetailBattle::undrift`]
/// takes back - under it the push is noise against the framings' own
/// tolerance and a second run would only re-sample the same frame.
pub const UNDRIFT_MIN: i32 = 16;

/// How far a run left its placed combatants from the capture's own ground
/// pairs at the phase, summed over the slots `seeded` placed (`seeded` is
/// the capture the run was seeded from - `captured` itself, or its
/// [`RetailBattle::undrift`]). `None` when the run measured no drift.
pub fn ground_residual(
    captured: &RetailBattle,
    seeded: &RetailBattle,
    run: &EngineBattle,
) -> Option<i64> {
    let want = captured.seeded_ground();
    let from = seeded.seeded_ground();
    let mut sum = None;
    for slot in 0..legaia_engine_core::world::INFLIGHT_GROUND_SLOTS {
        let (Some([wx, wz]), Some([fx, fz]), Some([dx, dz])) =
            (want[slot], from[slot], run.ground_drift[slot])
        else {
            continue;
        };
        let ex = i64::from(fx) + i64::from(dx) - i64::from(wx);
        let ez = i64::from(fz) + i64::from(dz) - i64::from(wz);
        *sum.get_or_insert(0) += ex.abs() + ez.abs();
    }
    sum
}

/// The first registered formation row whose monster list equals `ids`.
pub(super) fn matching_row(world: &legaia_engine_core::world::World, ids: &[u8]) -> Option<u16> {
    world.registered_formation_ids().into_iter().find(|fid| {
        world
            .tables
            .formation_table
            .formation(*fid)
            .is_some_and(|d| {
                d.slots.len() == ids.len()
                    && d.slots
                        .iter()
                        .zip(ids)
                        .all(|(s, &id)| s.monster_id == u16::from(id))
            })
    })
}

/// The system flags a scripted fight's capture holds only because the
/// record that entered the fight raised them on its way into the entry op:
/// every `SET` within the battle-entry window of the `3E FF <row>` that
/// names the capture's formation row
/// (`man_field_scripts::walk_battle_entry_arms`), where the capture's save
/// holds the flag.
///
/// Retail raised them **after** its scene entry ran, in the talk or
/// cutscene that staged the fight. A seed that lands the save with them
/// already up hands them to the entry script, and where the entry reads
/// one as "back from that fight" it runs the return instead: `town01`'s
/// init tests `0x23C`, clears it and engages Tetsu's post-fight scene,
/// whose hold keeps the system loop from re-selecting the region band;
/// `nilboa`'s picks its track off the duel-return markers and clears them.
/// The seed holds them back from the landing and raises them once the field
/// has settled ([`RetailBattle::entry_latches`]).
///
/// Lands the save once to read the scene's arms and match the row. The
/// pairing is the gate: a row no record enters has no arm. (The context's
/// scripted bit is not one - the sparring fight and the Rim Elm Gimard
/// read it clear.)
pub fn entry_latches(extracted: &Path, retail: &RetailObs, battle: &RetailBattle) -> Vec<u16> {
    let Some(save) = retail.save.clone() else {
        return Vec::new();
    };
    let held = crate::retail_compare::system_flag_ids(&save);
    // The report flag is the entry tail's own write (`FUN_801DA51C`,
    // `0x801DA6D8..0x801DA6E4`, under its arm), so a capture of the fight
    // holds it for this fight and no entry script had read it yet: `cave01`'s
    // install slice spawns Terra's after-battle record on it.
    let tail: Vec<u16> = {
        use legaia_engine_battle::battle_tutorial::{BATTLE_REPORT_ARM_FLAG, BATTLE_REPORT_FLAG};
        (held.contains(&BATTLE_REPORT_ARM_FLAG) && held.contains(&BATTLE_REPORT_FLAG))
            .then_some(BATTLE_REPORT_FLAG)
            .into_iter()
            .collect()
    };
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let Ok(mut session) = NativeSession::open(extracted, &cfg) else {
        return tail;
    };
    crate::retail_compare::pin_seed_retail(
        &mut session.host.world.toggles,
        battle.resident_shiny_seru,
    );
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    let _landing = session.resume_save(save, &retail.scene, &opts);
    let world = &session.host.world;
    let Some(row) = matching_row(world, &battle.monster_ids) else {
        return tail;
    };
    let mut out: Vec<u16> = world
        .scene_battle_entry_arms
        .iter()
        .filter(|a| u16::from(a.row) == row && held.contains(&a.flag))
        .map(|a| a.flag)
        .chain(tail)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Seed the engine into `retail`'s battle and sample it.
pub fn run_engine_battle(
    extracted: &Path,
    retail: &RetailObs,
    battle: &RetailBattle,
    rng_seed: u32,
    revive_victims: bool,
) -> Result<EngineBattle> {
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).context("open boot session")?;
    // The image child's `--retail` mode, so both sides deal the fight off
    // the same stream position; a shiny-Seru build's battle-entry draw is
    // kept on both (`RetailBattle::resident_shiny_seru`).
    crate::retail_compare::pin_seed_retail(
        &mut session.host.world.toggles,
        battle.resident_shiny_seru,
    );
    // Player-driven: retail parks every party turn on the command flow, so an
    // auto-resolving engine would spend the settle window fighting.
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    let mut save = retail
        .save
        .clone()
        .context("retail state has no liftable save window")?;
    // The flags the fight's own record raised are not the entry's to read
    // ([`entry_latches`]).
    crate::retail_compare::clear_system_flags(&mut save, &battle.entry_latches);
    let _landing = session.resume_save(save, &retail.scene, &opts);
    let mut director = crate::retail_compare::RecordingDirector::default();
    session.host.route_bgm_events(&mut director)?;
    // The fight is entered from a settled field, as retail's was: the
    // scene's entry scripts start its track and raise its entry state first.
    for _ in 0..legaia_engine_capture::SETTLE_TICKS {
        // The settle is seeding, not elapsed game time: the script timers
        // hold the counts retail's field had when the fight began (the battle
        // does not run the field VM, so the capture's slot table is the
        // field's), else a short timer runs out inside the window and fires
        // an arm retail had not reached (`jouind`'s spawn delay, 24 of 50).
        crate::retail_compare::hold_slot_table(&mut session, retail);
        session.tick()?;
        // The scripted VRAM effects, as both hosts drain them each tick: a
        // cross-context CLUT fade left unstepped keeps its context halted.
        session.field_vram_tick();
        session.fog_render_tick();
        session.host.route_bgm_events(&mut director)?;
    }
    for &idx in &battle.entry_latches {
        session.host.world.system_flag_set(idx);
    }
    let world = &mut session.host.world;
    let source;
    let man_row = matching_row(world, &battle.monster_ids);
    let fid = match man_row {
        Some(row) => {
            source = format!("man row {row}");
            // A scripted carrier's record raises its one-shot system-flag
            // arm on the way into the fight (the sparring tutorial's `50 19`);
            // the forced entry replays it, as `play-window --battle` does.
            world.replay_scripted_battle_arm(row);
            // The same record's op-0x35 words pick the fight's music: a
            // scripted boss's event starts its theme and selects the battle
            // sound set before the entry op (korb3's Gaza: `2028`, set
            // `-1`), so the forced entry replays them too.
            world.replay_scripted_battle_score(row);
            row
        }
        None => {
            // No row of this scene carries the retail cell: a scripted
            // fight installed from another table, or a scene whose rows the
            // engine does not register. Register the cell as its own
            // formation, with the archive's stats for its ids - the same
            // registration the image child runs for `--battle cell:...`.
            source = "synthesized".to_string();
            battle
                .formation_cell()
                .register(world, &retail.scene, || None)
                .context("formation cell does not fit a record")?
        }
    };
    let index = &session.host.index;
    legaia_engine_capture::formation_cell::ensure_monsters(
        &mut session.host.world,
        &battle.monster_ids,
        || index.entry_bytes_extended(867).ok(),
    );
    // The fight's own composition: retail's present list `0x8007BD10` is
    // the battle's seat table, not the save window's field party. They
    // differ on a guest seat (id `4`) and on a battle-id fight, whose init
    // (`FUN_80055B6C` with `DAT_8007B7FC != 0`) re-seeds the fallback trio
    // `{1, 2, 3}` through `FUN_80055B20` whatever the field party was. The
    // image side passes the same list as `play-window --party`.
    // The stage the fight is staged in: the region reader's variant for the
    // tile it started on. The capture's player actor is no longer the field
    // walker, so the seed cannot stand on that tile; it stamps the variant.
    session
        .host
        .world
        .seed_battle_stage_variant(battle.stage_variant);
    session
        .host
        .world
        .seed_battle_backdrop_keep_object_1(battle.keep_backdrop_object_1);
    session.host.world.toggles.battle_camera =
        legaia_engine_system::option_values::BattleCameraOpt::from_word(battle.camera_option);
    // Hand any replayed BGM words to the director before reading the word.
    session.host.route_bgm_events(&mut director)?;
    let seats = retail_roster_slots(battle);
    if !seats.is_empty() && seats != session.host.world.party.active_party {
        session.host.world.set_active_party(seats);
    }
    // The field's own track word and the world's current track, as the
    // battle swap will find them.
    let field_word = session.host.bgm_track_word.or(director.last);
    let field_current = session.host.world.audio.current_bgm;
    session.host.world.rng_state = rng_seed;
    // Held through the intro transition, so the field's own draws there do
    // not re-deal the fight (`EncounterState::rng_hold`).
    session.host.world.encounters.rng_hold = Some(rng_seed);
    if let Some((step, state)) = battle.frame_step_seed() {
        session.host.world.seed_battle_frame_step(step, state);
    }
    if !session.host.world.force_encounter(fid) {
        bail!("force_encounter({fid}) refused ({source})");
    }
    let mut entered = false;
    for _ in 0..ENTRY_TICKS {
        session.tick()?;
        session.field_vram_tick();
        session.fog_render_tick();
        session.host.route_bgm_events(&mut director)?;
        if session.host.world.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    if !entered {
        bail!(
            "forced encounter ({source}) never reached battle mode in {ENTRY_TICKS} ticks (mode {:?})",
            session.host.world.mode
        );
    }
    // A battle-id fight was not entered by the field entity SM, so retail
    // never ran the SM's entry tail: the flags the forced entry's tail just
    // wrote go back to the capture's own (the `jou` boss casts hold the
    // victory latch of an earlier fight right through theirs).
    if battle.battle_id != 0 {
        let held = retail
            .save
            .as_ref()
            .map(crate::retail_compare::system_flag_ids)
            .unwrap_or_default();
        for idx in crate::retail_compare_battle::BATTLE_ENTRY_TAIL_FLAGS {
            if held.contains(&idx) {
                session.host.world.system_flag_set(idx);
            } else {
                session.host.world.system_flag_clear(idx);
            }
        }
    }
    // Read on the latch as well: the entry's own round-one Begin may have
    // run already and moved `+0x290` into `+0x291`.
    let surprise_opening = {
        let w = &session.host.world;
        let none = legaia_engine_battle_vm::battle_formulas::FormationAdvantage::None;
        w.battle_formation() != none || w.battle_formation_latched() != none
    };
    // Reconstruct the mid-fight HP / MP (the engine seeds full bars).
    //
    // A capture taken on a party action in flight whose target already
    // reads `0` HP (`+0x14C`) is the swing that killed it: the drive replays
    // that action from the prompt, and a monster seeded dead would end the
    // fight at the first `0x5A` wipe gate before the swing ever started. Such
    // a victim is seeded at `1` HP, so the replayed swing makes the kill the
    // capture shows, and its HP is read at the phase rather than at the
    // prompt. The caller asks for it ([`RetailBattle::action_victims`]) only
    // after the capture's phase was not reached with the HP as read.
    let pc_seed = session.host.world.party.party_count.clamp(1, 3) as usize;
    let victims = if revive_victims {
        battle.action_victims()
    } else {
        Vec::new()
    };
    let ground = battle.seeded_ground();
    let facing = battle.seeded_facing();
    let defeat_lanes = battle.engine_defeat_lanes();
    let status = battle.engine_status();
    let spent = battle.engine_spent_keys();
    let hp_seed: Vec<BarSeed> = {
        let world = &session.host.world;
        let pc = world.party.party_count.clamp(1, 3) as usize;
        battle
            .party
            .iter()
            .enumerate()
            .chain(battle.monsters.iter().enumerate().map(|(m, c)| (pc + m, c)))
            .filter_map(|(slot, c)| {
                let c = c.as_ref()?;
                let hp = if slot >= pc && victims.contains(&(slot - pc)) {
                    1
                } else {
                    battle.pre_action_hp(retail_slot(pc, slot), c)
                };
                Some(BarSeed {
                    slot: u8::try_from(slot).ok()?,
                    hp,
                    mp: c.mp,
                    ground: ground.get(slot).copied().flatten(),
                    facing: facing.get(slot).copied().flatten(),
                    defeat_lanes: (hp == 0)
                        .then(|| defeat_lanes.get(slot).copied().flatten())
                        .flatten(),
                    status: (hp != 0)
                        .then(|| status.get(slot).copied().flatten())
                        .flatten()
                        .map_or(0, seedable_status_bits),
                    spent_key: spent.get(slot).copied().unwrap_or(false),
                })
            })
            .collect::<Vec<_>>()
    };
    // The empty party seats stand where the capture's structs stand: they
    // weigh in every recentre's box (`World::seat_empty_battle_seat`).
    let mut hp_seed = hp_seed;
    hp_seed.extend(battle.empty_seat_seeds());
    apply_bar_seeds(&mut session.host.world, &hp_seed);
    if battle.mystic_shield {
        session.host.world.raise_mystic_shield();
    }
    // Place the engine at the capture's phase ([`SeedPlan`]).
    //
    // An opening capture is sampled at retail's entry-sweep counter. Everything else
    // runs the opening (banner, intro camera, initiative) to the first round
    // prompt, the earliest point a retail capture of a running fight can
    // share with a fresh entry, and then:
    //
    // - a prompt capture settles there;
    // - a menu capture is driven to the same surface on the same seat
    //   through the pad path ([`BattleDrive::Menu`]);
    // - a mid-cast capture is replayed: the cast is seeded to dispatch the
    //   moment the prompt opens, and the session runs until it reaches the
    //   capture's phase ([`PhaseGate`]);
    // - any other action in flight is reached by playing the round out
    //   through the pad path until the action SM holds the capture's state
    //   on the capture's seat ([`BattleDrive::Action`]).
    let plan = battle.seed_plan();
    let seed = battle.inflight_cast();
    let mut prompt_tick = None;
    if plan != SeedPlan::Opening {
        // Retail's battle tick runs only once the frame driver's entry sweep
        // is over, so no capture of a running fight was taken under it. The
        // port's tick does not wait; the seed does, and a cast is seeded to
        // dispatch only from there.
        let mut seeded = false;
        for t in 0..OPENING_TICKS {
            if !seeded && entry_sweep_reached(&session.host.world, 0xFF) {
                session.host.world.battle.inflight_seed = seed;
                seeded = true;
            }
            let w = &session.host.world;
            let reached = seeded
                && match seed {
                    Some(_) => w.battle.inflight_seed.is_none(),
                    None => w.battle.flow != BattleFlowState::Idle,
                };
            if reached {
                prompt_tick = Some(t);
                break;
            }
            session.tick()?;
            session.fog_render_tick();
            session.host.route_bgm_events(&mut director)?;
        }
    }
    let mut phase_tick = None;
    let mut idle_advance = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
    let mut driven = None;
    // A driven capture plays rounds the retail history did not (a Spirit
    // here, a strike there), so its combatant, bag, flag and track channels
    // are read at the first prompt, before the drive - the moment the seed
    // placed them - and only phase and camera at the phase itself.
    let mut pre_drive = None;
    let mut age_short = None;
    let mut action_pushes = None;
    match (plan, battle.phase_gate()) {
        // The entry sweep owns retail's camera until the frame driver's
        // counter passes `0xC0`; run the engine's to the same count.
        (SeedPlan::Opening, _) => {
            for _ in 0..ENTRY_SWEEP_TICKS {
                if entry_sweep_reached(&session.host.world, battle.entry_counter) {
                    break;
                }
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
            // A sparring capture past the intro timer (`0x14`) holds the
            // side-band's caption and the close-up it selects; the engine
            // raises it on the round start its open holds back until the
            // enemy names have gone (`World::sparring_open_held`).
            if matches!(battle.flow, 0x0C | 0x14) {
                for _ in 0..SPARRING_OPEN_TICKS {
                    if !session.host.world.battle.sparring_round_pending {
                        break;
                    }
                    session.tick()?;
                    session.fog_render_tick();
                    session.host.route_bgm_events(&mut director)?;
                }
            }
        }
        (SeedPlan::Cast, Some(gate)) if prompt_tick.is_some() => {
            let mut idle = IdleClock::default();
            for t in 0..INFLIGHT_TICKS {
                idle.sample(&session.host.world);
                if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
                    let w = &session.host.world;
                    eprintln!(
                        "[cast] t={t} st=0x{:02X} act={} yb={:?} acc={:?} facing={:?} bodies={:?} tgt={:?} cam={:?}",
                        w.battle_ctx.action_state,
                        w.battle_ctx.active_actor,
                        w.battle.camera.as_ref().map(|c| c.action_yaw_base()),
                        w.battle.camera.as_ref().map(|c| c.close_up_accum()),
                        w.actors
                            .iter()
                            .take(8)
                            .map(|a| a.battle.facing_angle & 0xFFF)
                            .collect::<Vec<_>>(),
                        w.actors
                            .iter()
                            .take(8)
                            .map(|a| (a.move_state.world_x, a.move_state.world_z, a.battle.seat))
                            .collect::<Vec<_>>(),
                        w.battle.camera.as_ref().map(|c| c.glide_target().focus),
                        w.battle
                            .camera
                            .as_ref()
                            .map(|c| (c.phase(), w.battle_cam_pose())),
                    );
                }
                if gate.met(&session.host.world) {
                    gate.align_camera(&mut session.host.world);
                    if battle.cast_glide_aligned()
                        && let Some(cam) = session.host.world.battle.camera.as_mut()
                    {
                        cam.align_glide_origin(battle.cam_tween.0, battle.cam_tween.1);
                    }
                    if battle.module_cam_aligned()
                        && let Some(cam) = session.host.world.battle.camera.as_mut()
                    {
                        cam.align_module_shot(battle.cam_tween.0, battle.cam_tween.1);
                    }
                    phase_tick = Some(t);
                    idle_advance = idle.advance();
                    break;
                }
                gate.steer(&mut session.host.world);
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
        }
        (SeedPlan::Menu { .. } | SeedPlan::Action { .. }, _) if prompt_tick.is_some() => {
            if let Some(drive) = battle.battle_drive() {
                let track = session.host.bgm_track_word.or(director.last);
                let mut snap = combat_snapshot(&mut session.host.world, track);
                // The bars were seeded ahead of the action's landed hits
                // ([`RetailBattle::pre_action_hp`]); the channel scores the
                // seed against the capture, so it reads them landed.
                let dealt = |slot: u8, hp: u16| -> u16 {
                    let sum: u32 = battle
                        .action_hits
                        .iter()
                        .filter(|&&(s, _)| s == slot)
                        .map(|&(_, v)| u32::from(v))
                        .sum();
                    (u32::from(hp).saturating_sub(sum)) as u16
                };
                for (s, c) in snap.party.iter_mut().enumerate() {
                    if battle
                        .party
                        .get(s)
                        .copied()
                        .flatten()
                        .is_some_and(|r| r.hp != 0)
                    {
                        c.hp = dealt(s as u8, c.hp);
                    }
                }
                for (m, c) in snap.monsters.iter_mut().enumerate() {
                    if battle
                        .monsters
                        .get(m)
                        .copied()
                        .flatten()
                        .is_some_and(|r| r.hp != 0)
                    {
                        c.hp = dealt(3 + m as u8, c.hp);
                    }
                }
                pre_drive = Some(snap);
                legaia_engine_capture::melee_hits::seed_melee_hits(
                    &mut session.host.world,
                    &battle.melee_hit_seeds(),
                );
                let pushes_before = session.host.world.battle.party_melee_pushes;
                driven = Some(run_drive(
                    &mut session,
                    &mut director,
                    drive,
                    &mut age_short,
                )?);
                action_pushes = Some(
                    session
                        .host
                        .world
                        .battle
                        .party_melee_pushes
                        .wrapping_sub(pushes_before),
                );
            }
        }
        _ => {
            for _ in 0..BATTLE_SETTLE_TICKS {
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
        }
    }
    if matches!(driven, Some(Some(_)))
        && let Some(snap) = pre_drive.as_mut()
    {
        let world = &session.host.world;
        for &m in &victims {
            if let (Some(c), Some(a)) = (snap.monsters.get_mut(m), world.actors.get(pc_seed + m)) {
                c.hp = a.battle.hp;
            }
        }
    }
    // The victory latch is the results sequencer's own write (`FUN_8004E568`,
    // `0x8004EECC`), not a script's: on a drive that reached its phase it is
    // read there, as the victims' HP is, where every other flag keeps the
    // seed's reading.
    if matches!(driven, Some(Some(_)))
        && let Some(snap) = pre_drive.as_mut()
    {
        let latch = legaia_engine_battle::victory::VICTORY_STORY_FLAG;
        crate::retail_compare::set_system_flag(
            &mut snap.save,
            latch,
            session.host.world.system_flag_test(latch),
        );
    }
    let snap = match pre_drive.take() {
        Some(snap) => snap,
        None => {
            let track = session.host.bgm_track_word.or(director.last);
            combat_snapshot(&mut session.host.world, track)
        }
    };
    // The idle orbit's yaw is a clock (`-4` a camera step from whatever
    // azimuth the field left): on a capture whose flow byte hands the frame
    // to the orbit, phase-align it to retail's reading, as the image child
    // does (`LEGAIA_BATTLE_ORBIT_YAW`), so the channel scores the framing
    // rather than the instant. `align_orbit_yaw` refuses unless the orbit
    // really owns the yaw (the far framing, no yaw glide in flight).
    if battle.orbit_owns_yaw()
        && let Some(cam) = session.host.world.battle.camera.as_mut()
    {
        cam.align_orbit_yaw(f32::from(retail.camera.yaw));
    }
    // A capture inside an action reads its camera part-way along a glide
    // whose start is the previous framing's leftover - an earlier action's
    // side-and-tilt coin, the orbit's clock at the commit - which the replay
    // reached on its own stream. Where the engine heads for retail's
    // endpoint, start it from retail's live value, so the channel scores the
    // framing rather than that history (`BattleCamera::align_glide_origin`;
    // the image child does the same, `LEGAIA_BATTLE_CAM_ALIGN`). Pad drives
    // only: a replayed cast's close-ups and module shots measured worse
    // aligned (`flute_spikefish_midcast` image `.910` to `.828`).
    if std::env::var_os("LEGAIA_RC_FX_TRACE").is_some() {
        trace_effect_pool(&session.host.world);
    }
    let mut cam_trace = None;
    if std::env::var_os("LEGAIA_RC_CAM_TRACE").is_some()
        && let Some(cam) = session.host.world.battle.camera.as_ref()
    {
        let p = |p: legaia_engine_camera::projection::BattleCamPose| {
            format!(
                "{}/{} tr={:?} focus={:?}",
                p.pitch,
                p.yaw.rem_euclid(4096.0),
                p.tr,
                p.focus
            )
        };
        cam_trace = Some(format!(
            "{:?} gliding={} | engine pose {} | engine target {} | retail live {} | retail end {}",
            cam.phase(),
            cam.is_gliding(),
            p(cam.framing_pose()),
            p(cam.glide_target()),
            p(battle.cam_tween.0),
            p(battle.cam_tween.1)
        ));
    }
    if let Some(t) = cam_trace.as_mut() {
        let pc = usize::from(battle.party_count);
        let world = &session.host.world;
        for (slot, a) in world.actors.iter().enumerate() {
            let pool = if slot < pc { slot } else { 3 + (slot - pc) };
            let Some((live, body, clip, size)) = battle.captured.get(pool).copied().flatten()
            else {
                continue;
            };
            if slot >= pc + usize::from(battle.monster_count) {
                continue;
            }
            t.push_str(&format!(
                " | slot {slot}: retail {live:?} body {body:?} facing {:?} clip {clip:#04X} size {size}, engine ({}, {}) body {:?} facing {} clip {:#04X} size {:?}",
                battle.facing.get(pool).copied().flatten(),
                a.move_state.world_x,
                a.move_state.world_z,
                a.battle.seat,
                a.battle.facing_angle & 0xFFF,
                world.battle_current_anim(slot),
                a.battle_monster_id
                    .and_then(|id| world.tables.monster_catalog.get(id))
                    .map(|d| d.size_class)
            ));
        }
    }
    // How far the drive moved each placed combatant: a capture of a running
    // action stands its combatants where the action had already moved them,
    // and the drive replays the action from there.
    let mut ground_drift = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
    if matches!(driven, Some(Some(_))) {
        let world = &session.host.world;
        for (slot, d) in ground_drift.iter_mut().enumerate() {
            let (Some([gx, gz]), Some(a)) = (ground[slot], world.actors.get(slot)) else {
                continue;
            };
            *d = Some([
                a.move_state.world_x as i32 - i32::from(gx),
                a.move_state.world_z as i32 - i32::from(gz),
            ]);
        }
    }
    // With the drift measured, the combatants stand on the capture's own
    // ground for the camera, which is then aligned against the framing its
    // law arms there (`legaia_engine_capture::ground_snap`); the image child
    // does the same at its frame (`LEGAIA_BATTLE_GROUND_SNAP`).
    if matches!(driven, Some(Some(_))) {
        let world = &mut session.host.world;
        let target = legaia_engine_capture::ground_snap::snap_battle_ground(
            world,
            &battle.engine_ground_snaps(),
        );
        let mask = match (world.battle.camera.as_mut(), target) {
            (Some(cam), Some(target)) => {
                cam.align_glide_origin_to(battle.cam_tween.0, battle.cam_tween.1, target)
            }
            _ => 0,
        };
        if let Some(t) = cam_trace.as_mut() {
            t.push_str(&format!(" | aligned {mask:#010b}"));
        }
    }
    let world = &session.host.world;
    let pose = world.battle_cam_pose();
    let camera = CameraObs {
        pitch: pose.pitch.round() as i16,
        yaw: (pose.yaw.round() as i32).rem_euclid(4096) as i16,
        h: BATTLE_H,
        eye: pose.tr.map(|v| v.round() as i32),
        // The engine holds the focus un-negated; retail's words are negated.
        focus: [
            -(pose.focus[0].round() as i32),
            -(pose.focus[2].round() as i32),
        ],
    };
    Ok(EngineBattle {
        hp_seed,
        rng_seed,
        rng_at_end: world.rng_state,
        surprise_opening,
        age_short,
        scene: session.host.scene.as_ref().map(|s| s.name.clone()),
        mode: snap.mode,
        formation_source: source,
        man_row,
        prompt_tick,
        inflight: seed.map(|_| phase_tick),
        driven,
        win_pose: world.battle.victory.and_then(|v| v.pose_id),
        action_pushes,
        action_state: world.battle_ctx.action_state,
        active_actor: world.battle_ctx.active_actor,
        active_target: world
            .actors
            .get(usize::from(world.battle_ctx.active_actor))
            .map_or(0, |a| a.battle.active_target),
        monster_ids: snap.monster_ids,
        party: snap.party,
        monsters: snap.monsters,
        flow: world.battle.flow,
        camera,
        bgm_id: snap.bgm_id,
        field_word,
        field_current,
        battle_bgm: snap.battle_bgm,
        save: snap.save,
        ground_drift,
        idle_advance,
        cam_trace,
    })
}

/// The advance each battle slot's idle loop (entry `0`) makes over a
/// replayed cast, sampled once a tick: the cursor's forward steps summed
/// across its wraps. A slot seen off its idle loop - on another clip, on a
/// reaction, a one-shot - is dropped for the span.
#[derive(Default)]
struct IdleClock {
    last: [Option<(u16, u32)>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
    sum: [u32; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
    lost: [bool; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
}

impl IdleClock {
    fn sample(&mut self, w: &legaia_engine_core::world::World) {
        for slot in 0..legaia_engine_core::world::INFLIGHT_GROUND_SLOTS {
            if self.lost[slot] {
                continue;
            }
            let looping_idle = w.actors.get(slot).and_then(|a| {
                let p = a.battle_animation.as_ref()?;
                (a.battle_reaction.is_none() && w.battle_current_anim(slot) == 0 && p.is_looping())
                    .then(|| (p.cursor_sixteenths(), (p.frame_count() * 16).max(1)))
            });
            let Some((cur, span)) = looping_idle else {
                self.lost[slot] = true;
                continue;
            };
            if let Some((prev, _)) = self.last[slot] {
                let step = (u32::from(cur) + span - u32::from(prev)) % span;
                self.sum[slot] += step;
            }
            self.last[slot] = Some((cur, span));
        }
    }

    fn advance(&self) -> [Option<u32>; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS] {
        let mut out = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        for (slot, o) in out.iter_mut().enumerate() {
            if !self.lost[slot] && self.last[slot].is_some() {
                *o = Some(self.sum[slot]);
            }
        }
        out
    }
}

/// The combatant-side observables a battle state is scored on.
/// Retail's pool slot for the engine's bar slot `slot`: party seats share
/// the index, monster `m` sits at `pc + m` in the engine and `3 + m` in
/// retail's actor table.
fn retail_slot(pc: usize, slot: usize) -> u8 {
    if slot < pc {
        slot as u8
    } else {
        (3 + slot - pc) as u8
    }
}

pub(super) struct CombatSnapshot {
    pub(super) mode: SceneMode,
    pub(super) monster_ids: Vec<Option<u16>>,
    pub(super) party: Vec<Combatant>,
    pub(super) monsters: Vec<Combatant>,
    pub(super) bgm_id: Option<u16>,
    pub(super) battle_bgm: Option<u16>,
    pub(super) save: legaia_save::SaveFile,
}

pub(super) fn combat_snapshot(
    world: &mut legaia_engine_core::world::World,
    field_track: Option<u16>,
) -> CombatSnapshot {
    let pc = world.party.party_count.clamp(1, 3) as usize;
    // Battle ordinal `s` holds roster record `party_roster_slot(s)` (the
    // present-party list): a solo duel's Gala is ordinal 0 but record 2, and
    // retail's battle init copies the max from *that* record (`+0x108`,
    // `FUN_80053CB8` at `0x80053E50`).
    let roster = world.save_full().party;
    let party_maxes: Vec<(u16, u16)> = (0..pc)
        .map(|s| {
            roster
                .members
                .get(world.party_roster_slot(s))
                .map_or((0, 0), |m| {
                    let v = m.hp_mp_sp();
                    (v.hp_max, v.mp_max)
                })
        })
        .collect();
    let comb = |a: &legaia_engine_core::world::Actor, mp_max: u16| Combatant {
        hp: a.battle.hp,
        hp_max: a.battle.max_hp,
        mp: a.battle.mp,
        mp_max,
    };
    let party = (0..pc)
        .filter_map(|s| {
            world
                .actors
                .get(s)
                .map(|a| comb(a, party_maxes.get(s).map_or(0, |m| m.1)))
        })
        .collect();
    let mcount = world
        .battle
        .active_formation
        .as_ref()
        .map_or(0, |f| f.slots.len().min(5));
    let monster_ids = (0..mcount)
        .map(|m| world.actors.get(pc + m).and_then(|a| a.battle_monster_id))
        .collect();
    let monsters = (0..mcount)
        .filter_map(|m| {
            let a = world.actors.get(pc + m)?;
            let mp_max = a
                .battle_monster_id
                .and_then(|id| world.tables.monster_catalog.get(id))
                .map_or(0, |d| d.mp);
            Some(comb(a, mp_max))
        })
        .collect();
    CombatSnapshot {
        mode: world.mode,
        monster_ids,
        party,
        monsters,
        // Retail's track-select word keeps the field track through a fight:
        // the battle theme is started without the op-0x35 store, so the word
        // the fight holds is the one the field resumes. The engine routes its
        // battle swap through the op-0x35 start (which rewrites its copy of
        // the word), so the comparand is the track it stashed to resume.
        bgm_id: if world.audio.battle_bgm_active {
            world.audio.field_bgm_resume
        } else {
            field_track
        },
        battle_bgm: world.audio.current_bgm,
        save: world.save_full(),
    }
}

/// `LEGAIA_RC_FX_TRACE`: the engine's effect-VM pool (`FUN_801E0080`'s
/// masters and children) at the sampled tick, on stderr, in the units the
/// retail pool holds them (world position `>> 8`) - the engine half of a
/// side-by-side against the capture's `_DAT_8007BD30` pool.
fn trace_effect_pool(world: &legaia_engine_core::world::World) {
    for (i, a) in world.actors.iter().enumerate() {
        if !a.active {
            continue;
        }
        eprintln!(
            "[fx] actor {i} pos ({}, {}, {}) impact {} colour {:#x} monster {:?} flags {:#010x} clip {:#04x} cursor {:?}",
            a.move_state.world_x,
            a.move_state.world_y,
            a.move_state.world_z,
            a.battle.impact_state,
            a.battle.render_color,
            a.battle_monster_id,
            a.battle.flag_word,
            world.battle_current_anim(i),
            a.battle_animation.as_ref().map(|p| p.cursor_sixteenths())
        );
    }
    let pool = &world.effect_pool;
    for (i, m) in pool.master_slots.iter().enumerate() {
        if m.child_count == 0 && m.spawn_cursor == 0 {
            continue;
        }
        eprintln!(
            "[fx] master {i} total {} flags {:#04x} cur {} wait {} ang {} pos ({}, {}, {}) script {:#x}",
            m.child_count,
            m.flags,
            m.spawn_cursor,
            m.state,
            m.angle,
            m.pos_x >> 8,
            m.pos_y >> 8,
            m.pos_z >> 8,
            m.script_offset
        );
    }
    for (i, c) in pool.children.iter().enumerate() {
        if c.frame_count == 0 {
            continue;
        }
        eprintln!(
            "[fx] child {i} frames {} cur {} wait {} vel {:?} pos ({}, {}, {}) anim {}",
            c.frame_count,
            c.frame_cursor,
            c.wait,
            c.velocity,
            c.pos[0] >> 8,
            c.pos[1] >> 8,
            c.pos[2] >> 8,
            c.anim_id
        );
    }
}
