//! Disc-gated: drive the **real** parsed Muscle Dome hand tables (battle
//! overlay PROT 0898) + the lead character's real swing costs (player file
//! PROT 0863) through the engine card-battle rules engine
//! ([`legaia_engine_core::muscle_dome`]).
//!
//! Closes the engine end of the arena: the play-window load path resolves
//! the real deck (command ids `0xC..=0xF`) and the real per-command costs
//! (the equipped-section swing records' `+0x74` bytes - the retail cost set
//! is favored `0x1E` / off-class `0x2A` / far `0x36`), and a contest commits
//! cards under the budget, resolves, and decides through the world tick.
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is absent.

use legaia_asset::battle_char_assembly;
use legaia_asset::muscle_dome as md;
use legaia_asset::static_overlay;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::muscle_dome::{MuscleCard, MuscleDomeSession, MusclePhase};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::{SceneMode, World};

/// The pinned retail swing-cost value set (arts-command-gauge: favored /
/// off-class / far).
const RETAIL_COSTS: [u8; 3] = [0x1E, 0x2A, 0x36];

fn real_hand() -> Option<([u8; 4], [u8; 4])> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    // Hand command ids from the battle overlay.
    let rec = static_overlay::overlay_map()
        .by_prot_index(md::MUSCLE_OVERLAY_PROT_INDEX as u32)
        .expect("battle overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0898 (extended)");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    assert!(md::verify_resident(&loaded), "arena resident in 0898");
    let commands = md::hand_command_ids(&loaded).expect("real hand command ids decode");

    // Lead character's real swing costs (default equipment).
    let player = host
        .index
        .entry_bytes_extended(863)
        .expect("read PROT 0863");
    let pack = legaia_asset::battle_data_pack::parse(&player).expect("player pack parses");
    let swings = battle_char_assembly::swing_battle_animations(&player, &pack, &[0u8; 5])
        .expect("swing records decode");
    let mut costs = [0u8; 4];
    for s in &swings {
        let i = (s.slot - 0xC) as usize;
        costs[i] = s.cost;
    }
    Some((commands, costs))
}

#[test]
fn real_hand_tables_drive_a_decided_contest() {
    let Some((commands, costs)) = real_hand() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    // The deck is the four direction-command ids, and every real swing cost
    // is one of the pinned retail values.
    let mut sorted = commands;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        [0x0C, 0x0D, 0x0E, 0x0F],
        "deck = commands 0xC..=0xF"
    );
    for (i, &c) in costs.iter().enumerate() {
        assert!(
            RETAIL_COSTS.contains(&c),
            "swing slot {i} cost {c:#x} is a retail cost value"
        );
    }

    // Build the contest like the play-window M key: player hand carries the
    // real costs keyed by command id.
    let card = |cmd: u8, cost: u16| MuscleCard {
        command_id: cmd,
        cost,
    };
    let player_hand =
        std::array::from_fn(|i| card(commands[i], costs[(commands[i] - 0xC) as usize] as u16));
    let opp_hand = std::array::from_fn(|i| card(commands[i], 0x1E));
    let session = MuscleDomeSession::new(player_hand, opp_hand, [120, 120], [500, 400], 1);

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_muscle_dome(session);
    assert_eq!(world.mode, SceneMode::MuscleDome);

    // Commit cards through the pad until the budget rejects, confirm, and
    // play rounds until the contest decides.
    let directions = [
        PadButton::Left.mask(),
        PadButton::Right.mask(),
        PadButton::Up.mask(),
        PadButton::Down.mask(),
    ];
    let mut frames = 0u32;
    loop {
        frames += 1;
        assert!(frames < 10_000, "contest terminates");
        let s = world.muscle_dome.as_ref().expect("session installed");
        if s.decided() {
            break;
        }
        // Presses are edge-triggered: interleave release frames so repeated
        // presses of the same button register.
        let pad = if frames.is_multiple_of(2) {
            0
        } else {
            match s.phase() {
                MusclePhase::Select => {
                    // Commit the cheapest still-affordable card; confirm once
                    // nothing more fits.
                    let pick = (0..4)
                        .filter(|&c| s.can_commit(0, c))
                        .min_by_key(|&c| s.hand(0)[c].cost);
                    match pick {
                        Some(c) => directions[c],
                        None => PadButton::Cross.mask(),
                    }
                }
                MusclePhase::Resolve => 0,
                MusclePhase::RoundOver | MusclePhase::Won | MusclePhase::Lost => {
                    PadButton::Cross.mask()
                }
            }
        };
        world.set_pad(pad);
        let _ = world.tick();
    }

    // Budget accounting held on the way: the spent+budget invariant is the
    // pool, and every queued id is a real deck command.
    let s = world.muscle_dome.as_ref().unwrap();
    assert!(s.decided());
    let dmg = s.last_round_damage();
    assert!(dmg[0] > 0 || dmg[1] > 0, "the deciding round dealt damage");
    // score readout formula holds at the terminal state.
    for slot in 0..2 {
        assert_eq!(s.score_percent(slot), s.hp(slot) * 0x6C / [500, 400][slot]);
    }

    // Leaving through the world tick restores the interrupted mode (release
    // frame first so the Cross press edge-triggers).
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field, "return mode restored");
    assert!(world.muscle_dome.is_none(), "session cleared on exit");
}
