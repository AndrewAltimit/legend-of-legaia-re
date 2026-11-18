//! A summon creature's object that a cast module moves onto another channel
//! (`CastFxState::creature_channel_object`) draws on the browser page, posed by
//! that channel.
//!
//! Jedo's scythe is object 22 of a 23-object creature whose clips have 22
//! parts; from clip 4 on, channel 6 draws it (PROT 0931's tail
//! `0x801F89A0..0x801F89F0`). Meta's sword is object 16 over 16-part clips,
//! moved onto channel 14 (PROT 0932 `0x801F7738..0x801F7758`). Neither object
//! has a pose entry of its own, so the channel's placement had nowhere to land:
//! the page (which poses an object only from its own entry and collapses one
//! past the pose) drew neither weapon, and the native window drew each unposed
//! at the body's origin. The pin: every object the page draws has a pose entry,
//! over each summon's whole cast, and each weapon is drawn at some point.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

fn disc_runtime() -> Option<LegaiaRuntime> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    let disc = std::fs::read(path).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).ok()?;
    Some(rt)
}

#[test]
fn a_summons_channel_moved_weapon_is_posed_and_drawn() {
    let Some(mut rt) = disc_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // (spell, the weapon object)
    for (spell, weapon) in [(0x9Du8, 22u32), (0x9E, 16)] {
        // The magic page's launch (`startPendingCast`).
        rt.enter_field("map01").expect("enter map01");
        let _ = rt.cheat_apply("level", 99.0, "");
        let lead: Vec<u8> = serde_json::from_str(&rt.active_party_json()).unwrap_or_default();
        let lead = lead.first().copied().unwrap_or(0);
        assert!(rt.set_active_party_spec(&lead.to_string()).is_empty());
        assert!(rt.debug_seed_inflight_cast(0, spell, 3));
        assert!(rt.debug_seed_formation(0));
        assert!(rt.debug_force_battle(-1));
        assert!(rt.debug_run_to_inflight(1800) > 0);
        let mut unposed = std::collections::BTreeSet::new();
        let mut weapon_drawn = 0u32;
        for _ in 0..2400 {
            rt.frame_ticks(1, 0, 0).expect("tick");
            if !rt.play_battle_active() {
                break;
            }
            for a in 0..rt.play_battle_actor_count() {
                let ids = rt.play_battle_actor_object_ids(a);
                let n_objects = ids.iter().max().map_or(0, |m| m + 1);
                let parts = (rt.play_battle_actor_pose(a).len() / 6) as u32;
                if parts == 0 {
                    continue;
                }
                let hidden = rt.play_battle_actor_hidden(a);
                for o in (0..n_objects).filter(|o| !hidden.contains(o)) {
                    if o >= parts {
                        unposed.insert((a, o));
                    } else if n_objects == weapon + 1 && o == weapon {
                        weapon_drawn += 1;
                    }
                }
            }
        }
        eprintln!("[summon {spell:#04x}] weapon drawn on {weapon_drawn} frames");
        assert!(
            unposed.is_empty(),
            "spell {spell:#04x}: drawn objects with no pose entry (actor, object): {unposed:?}"
        );
        assert!(
            weapon_drawn > 0,
            "spell {spell:#04x}: object {weapon} never drew"
        );
    }
}
