//! Disc-gated: the intro style a scripted fight opens with.
//!
//! `vozz` `P2[13]` starts the Viguro pair with `3E FF 0A`. Row `10` carries a
//! non-zero `record[+0]`, so the entity SM's confirm state raises
//! `DAT_8007BD60 | 0x80` (`FUN_801DA51C`, `0x801DA5F8..0x801DA61C`), and the
//! intro init's selector (`FUN_801CE8CC`) takes the flags arm: monster `0x4A`
//! is outside `0x13..=0x15` and no override names it, so the style is the
//! spin-up particle field, the white burst the retail film shows.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_render_kernels::battle_intro_styles::{IntroStyle, select_intro_style};

#[test]
fn the_vozz_viguro_pair_opens_on_the_spin_up_field() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.set_entry_seat_tile(10, 10);
    host.enter_field_scene("vozz", 0).expect("enter");
    let inputs = host.battle_intro_style_inputs(10);
    assert_eq!(
        inputs.battle_flags & 0x80,
        0x80,
        "a scripted row: {inputs:?}"
    );
    assert_eq!(inputs.formation_slot0, 0x4A, "the Viguro: {inputs:?}");
    let choice = select_intro_style(&inputs);
    assert_eq!(choice.style, IntroStyle::SpinUpParticles, "{choice:?}");
}
