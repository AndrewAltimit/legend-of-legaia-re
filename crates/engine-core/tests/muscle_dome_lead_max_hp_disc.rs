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
