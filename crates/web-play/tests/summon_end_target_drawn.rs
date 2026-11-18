//! Every Seru summon on the magic page's full-battle view leaves a monster it
//! did not kill drawn once the cast is over.
//!
//! The defect this pins: PROT 0908 (Zenoir, `0x86`) puts a seat its arm-11
//! hits strike into the defeat fade (`+0x21C = 2`) only when the hit empties
//! its HP (`lhu v0,0x14c` / `bne v0,zero` at `0x801F7D80..0x801F7D98`, and
//! the splash's twin at `0x801F7F94..0x801F7FB0`). The port had the test the
//! other way round, so a victim that survived Zenoir faded to a zero colour
//! word and fought on invisible - on both hosts, the tick being the shared
//! `zenoir_tick`.
//!
//! The pin, per summon `0x81..=0xA0` (bar `0x98`, whose module is a stub),
//! over the page's launch (`startPendingCast`) and the rest of the round: no
//! living body ever carries the defeat fade, and at the end every living
//! monster's colour word is not the hide's or a drained fade's zero.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

/// `+0x21C`'s defeat fade.
const DEFEAT_FADE: u8 = 2;

/// The frames run past the launch: the cast (Jedo's, the longest, runs past
/// 2600), the monsters' answer and the next round's opening.
const RUN: u32 = 4000;

fn disc_runtime() -> Option<LegaiaRuntime> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    let disc = std::fs::read(path).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).ok()?;
    Some(rt)
}

/// The magic page's launch (`startPendingCast` in site/_content/play.html).
fn launch(rt: &mut LegaiaRuntime, spell: u8) {
    let _ = rt.play_abandon_opening_chain();
    rt.enter_field("map01").expect("enter map01");
    let _ = rt.cheat_apply("level", 99.0, "");
    let lead: Vec<u8> = serde_json::from_str(&rt.active_party_json()).unwrap_or_default();
    let lead = lead.first().copied().unwrap_or(0);
    assert!(rt.set_active_party_spec(&lead.to_string()).is_empty());
    assert!(rt.debug_seed_inflight_cast(0, spell, 3));
    assert!(rt.debug_seed_formation(0));
    assert!(rt.debug_force_battle(-1), "no formation on map01");
    assert!(rt.debug_run_to_inflight(1800) > 0);
}

/// The living monsters' world slots.
fn living_monsters(rt: &LegaiaRuntime) -> Vec<usize> {
    let Some(host) = rt.scene_host.host() else {
        return Vec::new();
    };
    host.world
        .living_monster_slots()
        .into_iter()
        .filter(|&s| host.world.actors[s].battle.hp != 0)
        .collect()
}

#[test]
fn every_summon_leaves_a_surviving_target_drawn() {
    let Some(mut rt) = disc_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut failures = Vec::new();
    for spell in (0x81u8..=0xA0).filter(|&s| s != 0x98) {
        launch(&mut rt, spell);
        let mut faded_alive = None;
        for f in 0..RUN {
            rt.frame_ticks(1, 0, 0).expect("tick");
            if !rt.play_battle_active() {
                break;
            }
            let host = rt.scene_host.host().expect("host");
            for (i, a) in host.world.actors.iter().enumerate() {
                if a.active
                    && a.battle.hp != 0
                    && a.battle.render_flag == DEFEAT_FADE
                    && faded_alive.is_none()
                {
                    faded_alive = Some((f, i));
                }
            }
        }
        let mut unseen = Vec::new();
        if rt.play_battle_active() {
            let host = rt.scene_host.host().expect("host");
            for s in living_monsters(&rt) {
                let b = &host.world.actors[s].battle;
                if host.world.battle_body_hidden(s)
                    || (b.render_flag == DEFEAT_FADE && b.render_color & 0x00FF_FFFF == 0)
                {
                    unseen.push(s);
                }
            }
        }
        eprintln!(
            "[summon {spell:#04x}] living body in the defeat fade: {faded_alive:?}; \
             living monsters not drawn at the end: {unseen:?}"
        );
        if faded_alive.is_some() || !unseen.is_empty() {
            failures.push((spell, faded_alive, unseen));
        }
    }
    assert!(
        failures.is_empty(),
        "summons that leave a living body faded or hidden \
         (spell, first (frame, slot) in the defeat fade, slots unseen at the end): {failures:?}"
    );
}
