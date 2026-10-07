//! Disc-gated: the mist-attack Rim Elm (`town0b`) fights a human can start.
//!
//! The full-game ladder crosses `town0b` on its own pad stream - it flees the
//! random encounters and never talks to Tetsu - so these drive the paths a
//! player takes and the ladder does not:
//!
//! - **Tetsu is a conversation.** The later Rim Elm variants place Tetsu on
//!   the tile and with the model the engine pins as `town01`'s sparring
//!   partner, but only `town01`'s record carries the fight (`3E FF 04` in
//!   `P1[10]`). `town0b`'s `P1[13]` is talk-only, and retail enters a
//!   scripted fight only through that op. The engine used to arm the
//!   sparring carrier on tile + model alone, so talking to Tetsu in the
//!   mist-attack town opened the training row - a lone 999-HP Tetsu with no
//!   lesson to end it - and the party could only flee or be worn down.
//! - **Every formation resolves under a human's command mix**: Begin or Run,
//!   any ring arm, Auto or Command, arts strings, cancel - no battle parks
//!   with its digest frozen.
//!
//! Seeded from the `rim_elm_gimard_victory` library state (the ladder's
//! `mist_rim_elm` anchor). Skip-passes without `LEGAIA_DISC_BIN`, an extracted
//! tree or the save library.

use std::hash::Hasher;
use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn first_dir(env: &str, rel: &str, probe: &str) -> Option<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .into_iter()
        .chain([repo_root().join(rel)])
        .find(|d| d.join(probe).exists())
}

/// The `rim_elm_gimard_victory` state: the party in `town0b` at the Gimard
/// fight (the full-game ladder's `mist_rim_elm` anchor).
const ANCHOR_FP: &str = "eaa8d800571dd9c144bcac7c740d1f9dcbeae4b0f7787a4b3e07acd1e82f1a73";

/// `town0b`'s Tetsu: partition-1 record 13, on `town01`'s sparring tile.
const TOWN0B_TETSU: u8 = 13;

struct Inputs {
    extracted: PathBuf,
    sc: Vec<u8>,
}

fn inputs() -> Option<Inputs> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = first_dir("LEGAIA_EXTRACTED_DIR", "extracted", "PROT.DAT") else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return None;
    };
    let Some(library) = first_dir("LEGAIA_SAVES_LIBRARY", "saves/library", "pcsx-redux") else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return None;
    };
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: set before any save-state read, from the test thread; every
        // writer in this binary sets the same value.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let path = library
        .join("pcsx-redux")
        .join(format!("{ANCHOR_FP}.sstate"));
    let Ok(state) = legaia_pcsxr::SaveState::from_path(&path) else {
        eprintln!("[skip] anchor state {ANCHOR_FP} not in the library");
        return None;
    };
    let base = (0x8008_4140u32 & 0x1F_FFFF) as usize;
    let sc = state.main_ram()[base..base + 0x2000].to_vec();
    Some(Inputs { extracted, sc })
}

/// Resume the anchor's SC block into `town0b`, as a card load does. The
/// random encounter roll stays off: every battle here is one the test made.
fn resume_town0b(inputs: &Inputs) -> BootSession {
    let cfg = BootConfig {
        scene: "town0b".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&inputs.extracted, &cfg).expect("open BootSession");
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    let sf = legaia_save::SaveFile::from_retail_sc_block(
        &inputs.sc,
        legaia_save::RETAIL_SC_PARTY_RECORDS,
    )
    .expect("lift the SC block");
    session.host.world.load_full(sf.clone());
    assert!(
        session.resume_save(sf, "town0b", &opts).entered_scene(),
        "the anchor resumes into town0b"
    );
    session
}

/// The presentation queues both play hosts drain after every tick (the soak
/// harness's `host_drains`), plus the frame-tail effect legs.
fn host_drains(session: &mut BootSession) {
    let w = &mut session.host.world;
    let _ = w.drain_field_events();
    let _ = w.drain_battle_events();
    let _ = w.drain_battle_hit_fx();
    let _ = w.drain_battle_hit_events();
    let _ = w.drain_battle_sfx_cues();
    let _ = w.drain_battle_shout_cues();
    let _ = w.drain_battle_xa_cues();
    let _ = w.drain_battle_xa_prestage();
    let _ = w.route_battle_effect_spawns();
    let _ = w.drain_battle_clut_stages();
    let _ = w.drain_minigame_sfx_cues();
    let _ = w.take_pending_summon_spawn();
    if let Some((move_id, origin)) = w.take_pending_move_fx_spawn()
        && w.spawn_move_fx(move_id, origin)
    {
        let _ = w.take_pending_move_fx_cue();
    }
    w.tick_effect_scene_graphs();
}

#[test]
fn rim_elm_variants_arm_the_sparring_carrier_only_in_town01() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir("LEGAIA_EXTRACTED_DIR", "extracted", "PROT.DAT") else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    for (scene, armed) in [
        ("town01", true),
        ("town0b", false),
        ("town0c", false),
        ("town0d", false),
    ] {
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut session = BootSession::open(&extracted, &cfg).expect("open BootSession");
        let opts = FieldLiveOpts {
            live_loop: false,
            player_battle: true,
            battle_bgm: None,
        };
        session
            .enter_field_live(scene, &opts)
            .unwrap_or_else(|e| panic!("enter {scene}: {e:#}"));
        let slots = &session.host.world.carriers.slots;
        eprintln!("[ran] {scene}: sparring carrier slots {slots:?}");
        assert_eq!(
            !slots.is_empty(),
            armed,
            "{scene}: the sparring carrier belongs to the record that carries `3E FF 04`"
        );
    }
}

#[test]
fn talking_to_tetsu_in_the_mist_town_starts_no_fight() {
    let Some(inputs) = inputs() else {
        return;
    };
    let mut session = resume_town0b(&inputs);
    for _ in 0..60 {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
        host_drains(&mut session);
    }
    session
        .host
        .world
        .trigger_field_interact(TOWN0B_TETSU, TOWN0B_TETSU);
    let mut prev = 0u16;
    for tick in 0..900 {
        // Page the conversation to its end, as a player does.
        let pad = if prev == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        prev = pad;
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
        host_drains(&mut session);
        let w = &session.host.world;
        assert_ne!(
            w.mode,
            SceneMode::Battle,
            "tick {tick}: talking to town0b's Tetsu entered formation {:?} - the \
             record carries no scripted-battle op",
            w.battle.active_formation.as_ref().map(|f| f.formation_id)
        );
    }
    eprintln!("[ran] town0b Tetsu talk: 900 ticks, no battle");
}

// ---------------------------------------------------------------------------
// The formation sweep
// ---------------------------------------------------------------------------

/// SplitMix64 - a seeded stand-in for a player's hands.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const DIRS: [PadButton; 4] = [
    PadButton::Up,
    PadButton::Down,
    PadButton::Left,
    PadButton::Right,
];

/// A human's battle hand: Begin or Run on the round prompt, any ring arm,
/// Auto or Command, arts strings, cancel now and then.
fn human_pad(session: &BootSession, rng: &mut Rng) -> u16 {
    use legaia_engine_core::battle_input::CommandPhase;
    let w = &session.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    let r = rng.below(100);
    match w.battle.command.as_ref().map(|c| &c.phase) {
        Some(CommandPhase::RoundPrompt { .. }) if r < 10 => PadButton::Right.mask(),
        Some(CommandPhase::RoundPrompt { .. }) => PadButton::Left.mask(),
        Some(CommandPhase::Menu { .. }) => match r {
            0..=69 => DIRS[rng.below(4) as usize].mask(),
            70..=89 => PadButton::Cross.mask(),
            _ => PadButton::Circle.mask(),
        },
        _ => match r {
            0..=59 => DIRS[rng.below(4) as usize].mask(),
            60..=89 => PadButton::Cross.mask(),
            90..=94 => PadButton::Circle.mask(),
            _ => PadButton::Triangle.mask(),
        },
    }
}

/// Everything a player could see move in a battle (the soak harness's
/// battle digest): the command session, the menus, the action context and
/// every combatant's HP / MP / liveness.
fn battle_digest(session: &BootSession) -> u64 {
    use std::fmt::Write;
    struct H(std::collections::hash_map::DefaultHasher);
    impl Write for H {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    let w = &session.host.world;
    let mut h = H(Default::default());
    let _ = write!(
        h,
        "{:?}|{:?}|{}|{:?}",
        w.battle.command,
        w.battle.item_menu,
        w.battle.tutorial_boxes.len(),
        w.battle_ctx
    );
    for a in w.actors.iter().take(8) {
        let _ = write!(h, "{},{},{}|", a.battle.hp, a.battle.mp, a.battle.liveness);
    }
    h.0.finish()
}

const BATTLE_TICKS: u32 = 40_000;
/// A frozen battle digest this long is a park.
const STALL_TICKS: u32 = 2_400;
const SEEDS: u64 = 3;

#[test]
fn every_town0b_formation_resolves_under_a_human_command_mix() {
    let Some(inputs) = inputs() else {
        return;
    };
    let ids: Vec<u16> = {
        let probe = resume_town0b(&inputs);
        let w = &probe.host.world;
        w.registered_formation_ids()
            .into_iter()
            .filter(|id| {
                w.tables
                    .formation_table
                    .formation(*id)
                    .is_some_and(|d| !d.slots.is_empty())
            })
            .collect()
    };
    assert!(!ids.is_empty(), "town0b registers its MAN formation rows");
    let mut parks = Vec::new();
    for &fid in &ids {
        for seed in 0..SEEDS {
            let mut session = resume_town0b(&inputs);
            assert!(session.host.world.force_encounter(fid));
            let mut rng = Rng(seed * 7919 + u64::from(fid));
            let mut prev = 0u16;
            let mut in_battle = false;
            let mut last = 0u64;
            let mut same = 0u32;
            let mut verdict = None;
            for tick in 0..BATTLE_TICKS {
                let want = if session.host.world.mode == SceneMode::Battle {
                    human_pad(&session, &mut rng)
                } else {
                    0
                };
                let pad = if prev == 0 { want } else { 0 };
                prev = pad;
                session.host.world.set_pad(pad);
                session.tick().expect("tick");
                host_drains(&mut session);
                let w = &session.host.world;
                if w.game_over || w.game_over_hold {
                    verdict = Some(format!("wipe at {tick}"));
                    break;
                }
                if w.mode == SceneMode::Battle {
                    in_battle = true;
                    let d = battle_digest(&session);
                    same = if d == last { same + 1 } else { 0 };
                    last = d;
                    if same >= STALL_TICKS {
                        parks.push(format!(
                            "F{fid} seed {seed}: parked at tick {tick}: action state {} actor {} \
                             command {:?}",
                            w.battle_ctx.action_state,
                            w.battle_ctx.active_actor,
                            w.battle.command.as_ref().map(|c| &c.phase)
                        ));
                        verdict = Some("parked".to_string());
                        break;
                    }
                } else if in_battle {
                    verdict = Some(format!("resolved after {tick}"));
                    break;
                }
            }
            let verdict = verdict.unwrap_or_else(|| {
                parks.push(format!(
                    "F{fid} seed {seed}: unresolved after {BATTLE_TICKS}"
                ));
                "unresolved".to_string()
            });
            eprintln!("[ran] town0b F{fid} seed {seed}: {verdict}");
        }
    }
    assert!(parks.is_empty(), "town0b battles parked: {parks:#?}");
}
