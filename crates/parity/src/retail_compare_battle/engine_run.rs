//! The engine side of a battle comparison: enter the same fight through the
//! real encounter path, seed it, drive it to the capture's phase and sample
//! the [`EngineBattle`].

use super::*;

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
    /// The engine's action-SM state when sampled.
    pub action_state: u8,
    /// The engine's active seat `ctx[+0x13]` when sampled.
    pub active_actor: u8,
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
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let Ok(mut session) = BootSession::open(extracted, &cfg) else {
        return Vec::new();
    };
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    let _landing = session.resume_save(save, &retail.scene, &opts);
    let world = &session.host.world;
    let Some(row) = matching_row(world, &battle.monster_ids) else {
        return Vec::new();
    };
    let mut out: Vec<u16> = world
        .scene_battle_entry_arms
        .iter()
        .filter(|a| u16::from(a.row) == row && held.contains(&a.flag))
        .map(|a| a.flag)
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
    let mut session = BootSession::open(extracted, &cfg).context("open boot session")?;
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
    for _ in 0..crate::retail_compare::SETTLE_TICKS {
        // The settle is seeding, not elapsed game time: the script timers
        // hold the counts retail's field had when the fight began (the battle
        // does not run the field VM, so the capture's slot table is the
        // field's), else a short timer runs out inside the window and fires
        // an arm retail had not reached (`jouind`'s spawn delay, 24 of 50).
        crate::retail_compare::hold_slot_table(&mut session, retail);
        session.tick()?;
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
            // formation, with the archive's stats for its ids.
            let rec = legaia_scene_formats::encounter_record::EncounterRecord::new(
                battle.monster_ids.len() as u8,
                &battle.monster_ids,
            )
            .context("formation cell does not fit a record")?;
            let mut def = legaia_engine_battle::encounter_formation::formation_def_from_record(
                &rec,
                retail.scene.clone(),
            );
            let mut id = def.formation_id;
            while world.tables.formation_table.formation(id).is_some() {
                id = id.wrapping_add(1);
            }
            def.formation_id = id;
            def.header_flags = u8::from(battle.scripted);
            world.tables.formation_table.insert(def);
            source = "synthesized".to_string();
            id
        }
    };
    let ids: Vec<u16> = battle.monster_ids.iter().map(|&i| u16::from(i)).collect();
    if ids
        .iter()
        .any(|id| session.host.world.tables.monster_catalog.get(*id).is_none())
    {
        let archive = session.host.index.entry_bytes_extended(867)?;
        let cat =
            legaia_engine_battle::monster_catalog::catalog_from_monster_archive(&archive, &ids);
        for def in cat.by_id.into_values() {
            session.host.world.tables.monster_catalog.insert(def);
        }
    }
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
        legaia_engine_menus::option_values::BattleCameraOpt::from_word(battle.camera_option);
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
                    c.hp
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
                })
            })
            .collect()
    };
    apply_bar_seeds(&mut session.host.world, &hp_seed);
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
    let mut driven = None;
    // A driven capture plays rounds the retail history did not (a Spirit
    // here, a strike there), so its combatant, bag, flag and track channels
    // are read at the first prompt, before the drive - the moment the seed
    // placed them - and only phase and camera at the phase itself.
    let mut pre_drive = None;
    let mut age_short = None;
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
            for t in 0..INFLIGHT_TICKS {
                if std::env::var_os("LEGAIA_RC_DRIVE_TRACE").is_some() {
                    let w = &session.host.world;
                    eprintln!(
                        "[cast] t={t} st=0x{:02X} act={} yb={:?} acc={:?} facing={:?} cam={:?}",
                        w.battle_ctx.action_state,
                        w.battle_ctx.active_actor,
                        w.battle.camera.as_ref().map(|c| c.action_yaw_base()),
                        w.battle.camera.as_ref().map(|c| c.close_up_accum()),
                        w.actors
                            .iter()
                            .take(8)
                            .map(|a| a.battle.facing_angle & 0xFFF)
                            .collect::<Vec<_>>(),
                        w.battle
                            .camera
                            .as_ref()
                            .map(|c| (c.phase(), w.battle_cam_pose())),
                    );
                }
                if gate.met(&session.host.world) {
                    phase_tick = Some(t);
                    break;
                }
                session.tick()?;
                session.fog_render_tick();
                session.host.route_bgm_events(&mut director)?;
            }
        }
        (SeedPlan::Menu { .. } | SeedPlan::Action { .. }, _) if prompt_tick.is_some() => {
            if let Some(drive) = battle.battle_drive() {
                let track = session.host.bgm_track_word.or(director.last);
                pre_drive = Some(combat_snapshot(&mut session.host.world, track));
                driven = Some(run_drive(
                    &mut session,
                    &mut director,
                    drive,
                    &mut age_short,
                )?);
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
    let mut cam_trace = None;
    if std::env::var_os("LEGAIA_RC_CAM_TRACE").is_some()
        && let Some(cam) = session.host.world.battle.camera.as_ref()
    {
        let p = |p: legaia_engine_battle_vm::battle_cam_script::BattleCamPose| {
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
    if matches!(driven, Some(Some(_)))
        && let Some(cam) = session.host.world.battle.camera.as_mut()
    {
        let mask = cam.align_glide_origin(battle.cam_tween.0, battle.cam_tween.1);
        if let Some(t) = cam_trace.as_mut() {
            t.push_str(&format!(" | aligned {mask:#010b}"));
        }
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
    let world = &session.host.world;
    // How far the drive moved each placed combatant: a capture of a running
    // action stands its combatants where the action had already moved them,
    // and the drive replays the action from there.
    let mut ground_drift = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
    if matches!(driven, Some(Some(_))) {
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
        action_state: world.battle_ctx.action_state,
        active_actor: world.battle_ctx.active_actor,
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
        cam_trace,
    })
}

/// The combatant-side observables a battle state is scored on.
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

/// The engine battle ordinal of retail pool slot `seat`: party slots are
/// shared, but retail's monsters sit at fixed pool slots `3..` whatever the
/// party size while the engine seats them straight after the party.
pub fn engine_seat(seat: u8, party_count: u8) -> u8 {
    if seat >= 3 {
        party_count + (seat - 3)
    } else {
        seat
    }
}
