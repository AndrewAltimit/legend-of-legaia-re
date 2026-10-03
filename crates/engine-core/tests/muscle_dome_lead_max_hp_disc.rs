//! A Muscle Dome leg entered hurt keeps the lead record's **maximum** HP.
//!
//! The between-leg restore raises the record's current HP `+0x106` (capped at
//! `+0x104`), so a contest's later legs open with `hp < max`. Retail's battle
//! init seeds the fighter's current HP `+0x14C` and maximum `+0x14E` from two
//! different record fields (`FUN_80053CB8`, the maximum off `+0x104` at
//! `0x80053DD4..0x80053DDC`), so the status plate reads `hp / max`. The door
//! warp used to hand the session the entry HP alone, which made the plate's
//! maximum the HP the fighter walked in with.
//!
//! Disc-gated: the door warp loads the arena and battle overlays. Skips and
//! passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::minigame_entry::MinigameSubId;
use legaia_engine_core::scene::{MinigameWarpOutcome, SceneHost};
use legaia_engine_core::world::SceneMode;

#[test]
fn a_hurt_lead_enters_the_next_leg_with_its_record_maximum() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let mut party = legaia_save::Party::zeroed(1);
    let rec = &mut party.members[0];
    let mut hms = rec.hp_mp_sp();
    hms.hp_max = 3691;
    hms.hp_cur = 2404;
    rec.set_hp_mp_sp(hms);
    host.world.load_party(party);

    host.world.minigames.pending_warp = Some(MinigameSubId::MuscleDome.sub_id());
    let outcome = host.drain_minigame_warp();
    assert!(
        matches!(
            outcome,
            Some(MinigameWarpOutcome::Entered(MinigameSubId::MuscleDome))
        ),
        "the door warp must stage a leg: {outcome:?}"
    );
    assert_eq!(host.world.mode, SceneMode::MuscleDome);
    let s = host
        .world
        .minigames
        .muscle_dome
        .as_ref()
        .expect("a leg session");
    assert_eq!(s.hp(0), 2404, "entry HP is the record's live HP");
    assert_eq!(
        s.hp_max(0),
        3691,
        "the plate's maximum is the record's +0x104, not the entry HP"
    );
    eprintln!("[ran] dome lead {} / {}", s.hp(0), s.hp_max(0));
}

/// The whole leg boundary through the world, not a hand-seeded record: a
/// leg fought down from full HP, decided, confirmed, scored by the
/// between-legs hub (whose recovery raises the record's current HP and caps
/// it at `+0x104`), and the next leg staged by the hub's own hand-off. The
/// second leg's plate reads the record's maximum, never the HP the fighter
/// walked back in with.
#[test]
fn the_second_leg_plate_keeps_the_record_maximum_through_the_hub() {
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::muscle_dome::MusclePhase;

    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    const MAX: u16 = 180;
    const HIT: i32 = 54;
    let mut party = legaia_save::Party::zeroed(1);
    let rec = &mut party.members[0];
    let mut hms = rec.hp_mp_sp();
    hms.hp_max = MAX;
    hms.hp_cur = MAX;
    rec.set_hp_mp_sp(hms);
    host.world.load_party(party);

    host.world.minigames.pending_warp = Some(MinigameSubId::MuscleDome.sub_id());
    assert!(matches!(
        host.drain_minigame_warp(),
        Some(MinigameWarpOutcome::Entered(MinigameSubId::MuscleDome))
    ));
    {
        let s = host.world.minigames.muscle_dome.as_mut().expect("leg 1");
        assert_eq!((s.hp(0), s.hp_max(0)), (i32::from(MAX), i32::from(MAX)));
        // Turn 1: the opponent lands one hit; turn 2: the fighter wins.
        s.ai_commit_all(0);
        s.ai_commit_all(1);
        s.end_selection();
        let mut landed = false;
        s.resolve_turn(|attacker, _| {
            if attacker == 1 && !landed {
                landed = true;
                HIT
            } else {
                0
            }
        });
        assert_eq!(s.phase(), MusclePhase::TurnOver);
        s.next_turn();
        s.ai_commit_all(0);
        s.ai_commit_all(1);
        s.end_selection();
        s.resolve_turn(|attacker, _| if attacker == 0 { 99_999 } else { 0 });
        assert_eq!(s.phase(), MusclePhase::Won);
        assert_eq!(s.hp(0), i32::from(MAX) - HIT);
    }
    // Confirm the decided leg through the world's own arm.
    host.world.set_pad(0);
    let _ = host.world.tick();
    host.world.set_pad(PadButton::Cross.mask());
    let _ = host.world.tick();
    host.world.set_pad(0);
    assert!(
        host.world.muscle_hub_between_legs(),
        "a won leg on an open course re-enters the hub"
    );
    let rec = host.world.party.roster.members[0].hp_mp_sp();
    assert_eq!(
        rec.hp_max, MAX,
        "the leg end leaves the record maximum alone"
    );
    assert!(rec.hp_cur <= MAX);
    host.world.begin_next_muscle_leg();
    assert!(matches!(
        host.drain_minigame_warp(),
        Some(MinigameWarpOutcome::Entered(MinigameSubId::MuscleDome))
    ));
    let s = host.world.minigames.muscle_dome.as_ref().expect("leg 2");
    assert_eq!(
        s.hp(0),
        i32::from(rec.hp_cur),
        "leg 2 enters on the record HP"
    );
    assert_eq!(s.hp_max(0), i32::from(MAX), "the plate's maximum is +0x104");
    eprintln!(
        "[ran] leg 2 lead {} / {} (record {} / {})",
        s.hp(0),
        s.hp_max(0),
        rec.hp_cur,
        rec.hp_max
    );
}
