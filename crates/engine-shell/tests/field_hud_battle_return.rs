//! Disc-gated: the field party readout and the post-load pad hold, through
//! the play window's own frame body.
//!
//! Every map install leaves `_DAT_8007B6B4 = 0x28` (`FUN_8003ADAC`, the tail
//! call of the MAN installer `FUN_8003AEB0`): the field frame pump keeps the
//! pad controller off until it drains, and the party HUD `FUN_801D0D38` stays
//! in its scene-entry arm while it is non-zero. The encounter hand-off
//! (`FUN_801DA51C`, `0x801DA704`) raises the short-idle marker
//! `_DAT_8007B5F4`, so after a fight that arm arms the countdown at `0` under
//! Immediate and the readout comes up on the frame the hold drains, where a
//! plain scene entry waits the full `0x28` after it. A retail capture of a
//! `vell` random encounter left by Run reads exactly that sequence.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_field::world_map_panel_actors::{HUD_IDLE_FRAMES_NEAR, HudDecision};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;

fn disc() -> Option<String> {
    std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
}

fn hold(w: &HeadlessPlayWindow) -> i32 {
    w.session().host.world.field_warp_words().1
}

fn drawn(w: &HeadlessPlayWindow) -> bool {
    matches!(
        w.field_party_hud().decision(),
        Some(HudDecision::Draw { .. })
    )
}

/// Ticks from the frame the hold reads `0` to the first drawn readout, with
/// the pad neutral.
fn ticks_from_hold_to_draw(w: &mut HeadlessPlayWindow, label: &str) -> u32 {
    let mut released = None;
    for t in 0..400u32 {
        w.ticks(0, 1);
        if released.is_none() && hold(w) == 0 {
            released = Some(t);
        }
        if let Some(r) = released
            && drawn(w)
        {
            return t - r;
        }
        assert!(
            released.is_some() || !drawn(w),
            "{label}: the readout came up under the hold"
        );
    }
    panic!("{label}: no readout within 400 ticks (hold released at {released:?})");
}

#[test]
fn the_readout_waits_out_the_load_hold_and_skips_its_idle_after_a_fight() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(&disc), "map01").expect("open");
    assert_eq!(w.session().host.world.mode, SceneMode::WorldMap);

    // The scene entry: the hold is up, the pad does not walk the player, and
    // once it drains the readout waits its full Immediate idle.
    assert_eq!(hold(&w), 0x28, "the map install leaves the post-load hold");
    let pos = |w: &HeadlessPlayWindow| {
        let world = &w.session().host.world;
        let a = &world.actors[world.player_actor_slot.expect("player") as usize];
        (a.move_state.world_x, a.move_state.world_z)
    };
    let start = pos(&w);
    for _ in 0..8 {
        w.ticks(PadButton::Up.mask(), 1);
    }
    assert_eq!(pos(&w), start, "the pad walked the player under the hold");
    let entry = ticks_from_hold_to_draw(&mut w, "scene entry");
    assert!(
        !w.field_party_hud().short_idle(),
        "no fight has raised the marker"
    );
    eprintln!("[hud] scene entry: readout {entry} ticks after the hold drained");
    assert!(
        (i64::from(entry) - i64::from(HUD_IDLE_FRAMES_NEAR)).abs() <= 1,
        "scene entry: the readout came up {entry} ticks after the hold, not 0x28"
    );

    // Walk until a fight opens, holding the region counter at its last step.
    let raises = w.session().host.world.encounters.hud_short_idle_raises;
    let mut t = 0u32;
    while w.session().host.world.mode != SceneMode::Battle {
        assert!(t < 2000, "no fight opened");
        if let Some(tr) = w.session_mut().host.world.world_map.region_tracker.as_mut() {
            tr.set_counter(1);
        }
        let dir = [
            PadButton::Up,
            PadButton::Right,
            PadButton::Down,
            PadButton::Left,
        ][(t as usize / 60) % 4];
        w.ticks(dir.mask(), 1);
        t += 1;
    }
    assert_eq!(
        w.session().host.world.encounters.hud_short_idle_raises,
        raises + 1,
        "the encounter hand-off raised the marker"
    );
    // Hold every monster at zero HP and mash through the prompts until the
    // wipe scan ends the fight.
    let mut f = 0u32;
    while w.session().host.world.mode == SceneMode::Battle {
        assert!(f < 8000, "the fight never ended");
        let world = &mut w.session_mut().host.world;
        let n = usize::from(world.party.party_count);
        for a in world.actors.iter_mut().skip(n) {
            if a.battle_monster_id.is_some() {
                a.battle.hp = 0;
                a.battle.liveness = 0;
            }
        }
        // Left only on the command prompts: a d-pad bit held on the field-back
        // frame is the HUD's suppress arm, which clears the marker in retail
        // too (`0x801D0D8C..0x801D0DBC`). Cross pages the rest.
        let pad = match (f % 2, world.battle.command.is_some()) {
            (0, true) => PadButton::Left.mask(),
            (0, false) => PadButton::Cross.mask(),
            _ => 0,
        };
        w.ticks(pad, 1);
        f += 1;
    }
    assert_eq!(w.session().host.world.mode, SceneMode::WorldMap);
    assert!(
        hold(&w) > 0,
        "the field-back load leaves the post-load hold (read {})",
        hold(&w)
    );
    assert!(
        w.field_party_hud().short_idle(),
        "the marker survives the battle to the field-back load"
    );
    let back = ticks_from_hold_to_draw(&mut w, "battle return");
    eprintln!("[hud] battle return: readout {back} ticks after the hold drained");
    assert!(
        back <= 1,
        "battle return: the readout waited {back} ticks after the hold"
    );
    assert!(
        !w.field_party_hud().short_idle(),
        "the first pass past the entry arm clears the marker"
    );
}
