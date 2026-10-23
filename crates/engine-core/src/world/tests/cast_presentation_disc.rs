//! Disc-gated: the cast modules' presentation calls, decoded off the real
//! band images and fired through `World::fire_cast_presentation`.

use super::*;
use legaia_render_kernels::screen_prim::ScreenPrim;

fn extracted_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = std::path::PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

fn band_world() -> Option<World> {
    use legaia_overlay_images::cast_effect_pool::{
        CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
    };
    let dir = extracted_dir()?;
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let entry = archive.entries[idx as usize].clone();
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).expect("read entry");
        assert!(pool.insert(idx, &bytes));
    }
    let mut w = World {
        mode: SceneMode::Battle,
        ..World::default()
    };
    w.install_cast_effect_pool(std::sync::Arc::new(pool));
    // A capture module is resident: the washes draw.
    w.casting.capture_spell = Some(0x90);
    Some(w)
}

/// PROT 0918 (Kemaro): arm 5's pass writes the wash block
/// (`0x801F729C..0x801F7304`: bands of `0xFF` lines, A `(0x3F, 0x3F, 0xFF)`,
/// B `(0xBF, 0xBF, 0xFF)`) and starts the alpha ramp; arm 6 ramps it one per
/// tick up to its `0x80` cap (`sltiu v0, 0x81` / `li v0, 0x80` at
/// `0x801F7384..0x801F7390`), and the dispatchers' tail draws the bands.
#[test]
fn kemaro_ramps_its_colour_wash_to_the_cap() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    assert!(w.cast_wash_prims().is_empty(), "nothing armed yet");
    w.fire_cast_presentation(918, None, None, 5, 5, false);
    w.fire_cast_presentation(918, None, None, 5, 6, false);
    let wash = w.casting.module_pres.wash;
    assert_eq!(wash.mode, 0);
    assert_eq!(wash.height, 0xFF);
    assert_eq!(wash.colour_a, [0x3F, 0x3F, 0xFF]);
    assert_eq!(wash.colour_b, [0xBF, 0xBF, 0xFF]);
    assert_eq!(wash.alpha, 1, "arm 5's own ramp step");
    assert_eq!(w.cast_wash_prims().len(), 2);
    for _ in 0..300 {
        w.fire_cast_presentation(918, None, None, 6, 6, false);
    }
    assert_eq!(w.casting.module_pres.wash.alpha, 0x80, "held at the cap");
    let prims = w.cast_wash_prims();
    let ScreenPrim::Flat(q) = prims[0] else {
        panic!("a flat band")
    };
    assert_eq!(q.gouraud.unwrap()[0], [31, 31, 128, 255]);
    assert!(!q.semi_transparent);
    // Arm 8 ramps it back down and clears it.
    for _ in 0..300 {
        w.fire_cast_presentation(918, None, None, 8, 8, false);
    }
    assert_eq!(w.casting.module_pres.wash.alpha, 0);
    assert!(w.cast_wash_prims().is_empty());
}

/// PROT 0906 (Gizam) arm 12's pass spawns its fade (`0x801F72BC`): kind 1,
/// a `0x100`-vsync ramp from black to `(0xA0, 0xA0, 0xE0)`, held, and parks
/// it in `ctx[+0x102C]` (`0x801F72D0`). Arm 13's pass kills it through that
/// slot (`ori 0x8` at `0x801F74D0`) and spawns the fade back out of it.
#[test]
fn gizam_parks_its_held_fade_and_kills_it_next_arm() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    w.fire_cast_presentation(906, None, None, 12, 12, false);
    assert!(w.casting.module_pres.fades.is_empty(), "held: no fade yet");
    w.fire_cast_presentation(906, None, None, 12, 13, false);
    let fades = &w.casting.module_pres.fades;
    assert_eq!(fades.len(), 1);
    let f = fades[0].state;
    assert_eq!((f.kind, f.duration, f.mode), (1, 0x100, [0, -1, 1]));
    assert!(fades[0].slot.is_some(), "parked in ctx[+0x102C]");
    assert_eq!(w.screen_fade_draws().len(), 1, "both hosts draw it");
    // Held: it survives its ramp while the module is resident.
    for _ in 0..0x140 {
        w.step_cast_hook_fades(1);
    }
    assert_eq!(w.casting.module_pres.fades.len(), 1);
    w.fire_cast_presentation(906, None, None, 13, 13, false);
    w.fire_cast_presentation(906, None, None, 13, 14, false);
    let fades = &w.casting.module_pres.fades;
    assert_eq!(fades.len(), 1, "the held fade killed, the fade-out spawned");
    assert_eq!(fades[0].state.duration, 0x20);
}

/// The hand-ported fades stay the hand port's: PROT 0905's arm 4 pair is
/// Vera's director's, not the hook's.
#[test]
fn hand_ported_fades_are_not_doubled() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    w.fire_cast_presentation(905, None, None, 4, 4, false);
    w.fire_cast_presentation(905, None, None, 4, 5, false);
    assert!(w.casting.module_pres.fades.is_empty());
}

/// PROT 0948 (Cross Beam) arm 0's effect-CLUT copy `(0xD0, 0x1DC)` ->
/// `(0xE0, 0x1DC)`, 16x1, queued on the pass.
#[test]
fn cross_beam_queues_its_clut_copy() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    w.fire_cast_presentation(948, Some(0x801F_69F0), None, 0, 0, false);
    w.fire_cast_presentation(948, Some(0x801F_69F0), None, 0, 1, false);
    let m = w.battle.vram_moves.last().expect("a queued copy");
    assert_eq!(m.src, (0xD0, 0x1DC));
    assert_eq!(m.size, (16, 1));
    assert_eq!(m.dst, (0xE0, 0x1DC));
}

/// PROT 0958's (Blazing Slash's) arm-9 armed wash (`FUN_8004695C(0x202020)`
/// at `0x801F78D0`) arms on the pass, and no host composites it: retail's
/// drain draws it in the farthest bucket, before the frame's scene, so over a
/// frame built from scratch it would only darken the frame's own pixels.
#[test]
fn armed_wash_arms_without_a_composited_quad() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    w.fire_cast_presentation(958, Some(0x801F_6DD8), None, 9, 9, false);
    w.fire_cast_presentation(958, Some(0x801F_6DD8), None, 9, 10, false);
    assert_eq!(w.cast_armed_wash(), Some(0x0020_2020));
    assert!(
        w.cast_wash_prims().is_empty(),
        "the armed wash lands under the scene, not over it"
    );
    // Arm 10 re-arms it on every pass (`0x801F78E4`).
    w.fire_cast_presentation(958, Some(0x801F_6DD8), None, 10, 10, false);
    assert_eq!(w.cast_armed_wash(), Some(0x0020_2020));
    // One-shot: a tick on an arm that does not re-arm it leaves it unarmed.
    w.fire_cast_presentation(958, Some(0x801F_6DD8), None, 14, 14, false);
    assert_eq!(w.cast_armed_wash(), None);
}

/// A holdable arm the engine passes on its first tick is one the port does
/// not pace: its calls are skipped and counted. PROT 0906 arm 12 holds on
/// its countdown in retail.
#[test]
fn an_unpaced_arm_fires_nothing() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    w.fire_cast_presentation(906, None, None, 12, 13, false);
    assert!(w.casting.module_pres.fades.is_empty());
    assert_eq!(w.casting.module_pres.unpaced_arms, 1);
}

/// PROT 0930 (Horn): arm 3's pass spawns the held white (`-1` hold) and
/// parks it in the part pool's last word `0x801C92EC`; arm 4's pass flushes
/// the pool (`jal 0x80050E74` at `0x801F6EE8`) before it spawns its own
/// fade-out, and the flush is what ends the white. Without it the additive
/// white stayed up for the rest of the cast.
#[test]
fn horn_arm_4_flush_ends_the_held_white() {
    let Some(mut w) = band_world() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    w.casting.capture_spell = None;
    w.fire_cast_presentation(930, None, None, 3, 4, false);
    let pool = legaia_overlay_images::cast_presentation::HandleSlot::Abs(0x801C_92EC);
    assert!(
        w.casting
            .module_pres
            .fades
            .iter()
            .any(|f| f.slot == Some(pool)),
        "arm 3 parks its held white in the pool"
    );
    w.fire_cast_presentation(930, None, None, 4, 4, false);
    w.fire_cast_presentation(930, None, None, 4, 5, false);
    let fades = &w.casting.module_pres.fades;
    assert!(
        fades.iter().all(|f| f.slot != Some(pool)),
        "the flush halted it"
    );
    assert_eq!(fades.len(), 1, "arm 4's own fade-out is live");
}
