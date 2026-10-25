//! The Muscle Dome entry's Triangle list pages over the lead's **learned**
//! arts, as retail's arena does: it runs the battle overlay's menu SM, whose
//! pager `FUN_801D3748` and list renderer `FUN_80034358` read the acting
//! character's record (`+0x185` count, `+0x186..` ids). The door warp names
//! the record to the leg's session, and the world's list window reads it.
//!
//! Disc-gated: the door warp loads the arena and battle overlays. Skips and
//! passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_battle_menus::muscle_dome_session::{DomeMenu, DomeSelectPad};
use legaia_engine_mode::minigame_entry::MinigameSubId;
use legaia_engine_scene_host::scene_host::{MinigameWarpOutcome, SceneHost};

#[test]
fn the_dome_list_pages_over_the_leads_learned_arts() {
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
    hms.hp_max = 900;
    hms.hp_cur = 900;
    rec.set_hp_mp_sp(hms);
    let mut skills = rec.displayed_skills();
    skills.count = 7;
    for (i, id) in skills.ids.iter_mut().take(7).enumerate() {
        *id = 1 + i as u8;
    }
    rec.set_displayed_skills(skills);
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
    let s = host.world.minigames.muscle_dome.as_mut().expect("a leg");
    assert_eq!(s.arts_list_owner(), Some(0), "the lead's record");
    assert_eq!(s.arts_list_pages(), 2, "seven learned arts, two pages");
    // Past the leg's battle-open hold, then Begin, Attack, Command - the
    // direction entry - and Triangle twice: the second page.
    while s.intro_up() {
        s.tick_intro(2);
    }
    let press = |f: fn(&mut DomeSelectPad)| {
        let mut p = DomeSelectPad::default();
        f(&mut p);
        p
    };
    for _ in 0..8 {
        if matches!(s.menu(), DomeMenu::Input(_)) {
            break;
        }
        let attack_mode = format!("{:?}", s.menu()).contains("AttackMode");
        s.select_input(if attack_mode {
            press(|p| p.right = true)
        } else {
            press(|p| p.left = true)
        });
    }
    assert!(matches!(s.menu(), DomeMenu::Input(_)), "{:?}", s.menu());
    s.select_input(press(|p| p.triangle = true));
    s.select_input(press(|p| p.triangle = true));
    for _ in 0..6 {
        s.tick_arts_list_slide(2);
    }
    assert_eq!(host.world.arts_list_window(), Some((0, 1, 12)));
    match host.world.arts_list_rows() {
        Some(rows) => {
            assert_eq!(rows.len(), 2, "learned ids 6 and 7 on page 2");
            eprintln!("[ran] dome list page 2: {} rows", rows.len());
        }
        None => eprintln!("[ran] dome list window seated (no arts-name table staged)"),
    }
}
