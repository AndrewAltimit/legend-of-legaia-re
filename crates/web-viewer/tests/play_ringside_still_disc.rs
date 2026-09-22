//! Disc-gated oracle for the Muscle Dome hub's **ringside still** on the
//! browser play page: a leg played to a win by pad, then the re-entered hub's
//! draw list read back through the page's own export.
//!
//! What it pins, host-side:
//!
//! 1. The leg's end leaves a still resident - the engine's
//!    `World::exit_muscle_dome` runs the battle end's pick (`FUN_801F6B24`'s
//!    `0x4C7 + s0`, extraction `1221` / `1222`).
//! 2. The re-entered hub draws it as two quads addressed **by texture page**,
//!    `0x106` then `0x109` - the packets `FUN_801D00F8`'s still arm writes -
//!    ahead of every hub sprite, with a fade level that rises off zero.
//! 3. The sheet those quads resolve onto decodes off the disc at the still's
//!    320x256 VRAM extent.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

const CROSS: u16 = 0x4000;
const DIRECTIONS: [u16; 4] = [0x0080, 0x0020, 0x0010, 0x0040];
const SUB_MUSCLE: u8 = 5;
/// The door warp's stand-in swing cost (`FAVORED_COST` in the scene host).
const SWING_COST: i64 = 0x1E;
/// The page's sheet id for the still.
const STILL_SHEET: u64 = 8;

fn loaded() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("koin1").ok()?;
    Some(rt)
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).expect("json")
}

fn step(rt: &mut LegaiaRuntime, pad: u16) {
    rt.set_pad(pad);
    rt.tick_frame().expect("tick_frame");
}

/// Warp into the dome and play the leg by pad until it decides. Returns the
/// deciding phase (`"won"` / `"lost"`), with the Cross that reports it
/// already pressed.
fn play_one_leg(rt: &mut LegaiaRuntime) -> String {
    assert!(rt.play_mg_debug_warp(SUB_MUSCLE));
    step(rt, 0);
    step(rt, 0);
    assert_eq!(rt.scene_mode(), "MuscleDome");
    for frame in 0..40_000u32 {
        let st = json(&rt.play_mg_muscle_state_json());
        let phase = st["phase"].as_str().unwrap_or("").to_string();
        if phase == "won" || phase == "lost" {
            step(rt, 0);
            step(rt, CROSS);
            step(rt, 0);
            return phase;
        }
        let pad = if frame % 2 == 1 {
            0
        } else if phase == "select" {
            // `budget` is what is left of the turn's pool.
            if st["budget"].as_i64().unwrap_or(0) >= SWING_COST
                && st["queued"].as_u64().unwrap_or(0) < 4
            {
                DIRECTIONS[(frame as usize / 2) % 4]
            } else {
                CROSS
            }
        } else {
            0
        };
        step(rt, pad);
    }
    panic!("the leg never decided");
}

#[test]
fn a_won_leg_reenters_the_hub_over_the_ringside_still() {
    let Some(mut rt) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The door warp's stand-in fighters are evenly matched and the damage
    // seed is frame-keyed, so a leg can go either way; a lost leg ends the
    // contest, so each retry opens a fresh one.
    let mut won = false;
    for _ in 0..6 {
        if play_one_leg(&mut rt) == "won" {
            won = true;
            break;
        }
    }
    assert!(won, "no leg was won in six tries");
    assert_eq!(rt.scene_mode(), "Field", "the dome hands the field back");

    let mut seen: Option<Vec<serde_json::Value>> = None;
    let mut peak = 0u64;
    for _ in 0..600 {
        step(&mut rt, 0);
        let hub = json(&rt.play_mg_muscle_hub_quads_json());
        let rows = hub["quads"].as_array().cloned().unwrap_or_default();
        let still: Vec<_> = rows
            .iter()
            .filter(|q| q["sheet"].as_u64() == Some(STILL_SHEET))
            .cloned()
            .collect();
        if still.is_empty() {
            continue;
        }
        // The still leads the list: retail links it at the OT's far end.
        assert_eq!(
            rows[..still.len()],
            still[..],
            "the still must precede every hub sprite"
        );
        peak = peak.max(still[0]["bright"].as_u64().unwrap_or(0));
        seen.get_or_insert(still);
    }
    let still = seen.expect("the re-entered hub must draw the ringside still");
    assert_eq!(still.len(), 2, "one 320x240 image in two packets");
    let tpages: Vec<u64> = still.iter().map(|q| q["tpage"].as_u64().unwrap()).collect();
    assert_eq!(tpages, [0x106, 0x109], "addressed by 16-bit texture page");
    assert_eq!(
        (still[0]["x"].as_i64(), still[0]["dw"].as_u64()),
        (Some(0), Some(192))
    );
    assert_eq!(
        (still[1]["x"].as_i64(), still[1]["dw"].as_u64()),
        (Some(192), Some(128))
    );
    assert_eq!(
        still[1]["u"].as_u64(),
        Some(192),
        "page 9 is sheet column 192"
    );
    assert!(peak >= 0x80, "the level reaches neutral ({peak:#x})");

    let variant = still[0]["pal"].as_u64().unwrap() as u32;
    assert!(variant <= 1, "extraction 1221 or 1222");
    let dims = rt.play_mg_muscle_hub_sheet_dims(STILL_SHEET as u32);
    assert_eq!(dims, [320, 256]);
    assert_eq!(
        rt.play_mg_muscle_hub_sheet_rgba(STILL_SHEET as u32, variant)
            .len(),
        320 * 256 * 4
    );
    eprintln!("[ok] ringside still variant {variant}, peak level {peak:#x}");
}
