//! Save-state-gated: the GTE projection constants retail runs the whole game
//! under.
//!
//! `OFX` / `OFY` are the screen-space centre every `RTPS` / `RTPT` adds after
//! the perspective divide, and nothing in the disassembly names them - they
//! are written once into the GTE control file by the draw-environment setup,
//! so the only way to read them is off a machine that has run that setup.
//! They are exactly the input a screen-space emitter port needs and cannot
//! derive, which is why they get an oracle of their own.
//!
//! The claim under test is that they are **global**, not per-phase: the same
//! pair holds across field, battle, battle-load and minigame states. That is
//! what lets a port treat them as constants. `H` is included precisely
//! because it is the counter-example - it is phase-dependent, so a test that
//! found everything constant would not be measuring anything.
//!
//! Skips when the save library is absent.

use legaia_mednafen::container::SaveState;
use std::path::PathBuf;

/// Screen-space x centre added after the perspective divide, in 16.16.
pub const EXPECT_OFX: i32 = 160 << 16;
/// Screen-space y centre, in 16.16. Note this is **114**, not the 120 a
/// naive 320x240 centre would give.
pub const EXPECT_OFY: i32 = 114 << 16;
/// Depth-cue interpolation slope (`DQA`).
pub const EXPECT_DQA: i16 = -64;
/// Depth-cue interpolation offset (`DQB`), in 16.16.
pub const EXPECT_DQB: i32 = 320 << 16;

/// Scenarios by the fingerprint prefix their library file is named for, with
/// the phase each one is in. Loaded by fingerprint, never by an emulator slot
/// number.
const STATES: &[(&str, &str)] = &[
    ("7269ae2b694a6a2760", "battle (overworld, orbit angle a)"),
    ("32257e93ca63ebea09", "battle (overworld, orbit angle b)"),
    ("5649c05e0b96b87758", "battle load"),
    ("4c650a3bb872b22029", "field (pre-battle)"),
    ("42845402539bfda659", "battle (command menu)"),
    ("1b1d645f8f8827c97f", "field (post-battle)"),
    ("21a55afe042ca93a24", "battle (Terra party)"),
    ("5461e7a0f28d55f805", "field (Rim Elm)"),
    ("5214a97ff7cac6d12c", "minigame (dance)"),
];

fn library() -> Option<Vec<PathBuf>> {
    for d in ["saves/library/mednafen", "../../saves/library/mednafen"] {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(
                std::fs::read_dir(&p)
                    .ok()?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .collect(),
            );
        }
    }
    None
}

fn i32le(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn i16le(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}

/// `(OFX, OFY, H, DQA, DQB)` out of a state's GTE control file.
fn gte(path: &PathBuf) -> Option<(i32, i32, i16, i16, i32)> {
    let s = SaveState::from_path(path).ok()?;
    let g = s.find_section_by_name("GTE")?;
    let get = |n: &str| -> Option<&[u8]> {
        let e = g.entries.iter().find(|e| e.name == n)?;
        s.payload.get(e.value_offset..e.value_offset + e.value_len)
    };
    Some((
        i32le(get("OFX")?),
        i32le(get("OFY")?),
        i16le(get("H")?),
        i16le(get("DQA")?),
        i32le(get("DQB")?),
    ))
}

#[test]
fn the_screen_centre_and_depth_cue_slope_are_global_but_h_is_not() {
    let Some(files) = library() else {
        eprintln!("[skip] saves/library/mednafen missing");
        return;
    };
    let mut seen = 0usize;
    let mut hs = std::collections::BTreeSet::new();
    for (fp, phase) in STATES {
        let Some(p) = files.iter().find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(fp))
        }) else {
            eprintln!("[skip state] {fp} ({phase}) not in the library");
            continue;
        };
        let Some((ofx, ofy, h, dqa, dqb)) = gte(p) else {
            eprintln!("[skip state] {fp} has no GTE section");
            continue;
        };
        eprintln!("{phase:<34} OFX={ofx} OFY={ofy} H={h} DQA={dqa} DQB={dqb}");
        assert_eq!(ofx, EXPECT_OFX, "OFX drifted in {phase}");
        assert_eq!(ofy, EXPECT_OFY, "OFY drifted in {phase}");
        assert_eq!(dqa, EXPECT_DQA, "DQA drifted in {phase}");
        assert_eq!(dqb, EXPECT_DQB, "DQB drifted in {phase}");
        hs.insert(h);
        seen += 1;
    }
    assert!(
        seen >= 4,
        "need several states to make this non-vacuous, got {seen}"
    );
    // The counter-example: `H` is written per phase (256 in battle, 512 in
    // the field), so "constant across the corpus" is a real finding about
    // OFX/OFY rather than a property of the measurement.
    assert!(
        hs.len() > 1,
        "H should vary across phases; got {hs:?} - if it stopped varying, \
         check the state selection still spans field and battle"
    );
}

/// `OFY` is `240 / 2` minus six - but that is a fact about the *port's*
/// 320x240 logical screen, not about retail's frame.
///
/// Retail's drawing area is `320 x 224` (`ClipY0..ClipY1`) inside a `228`-line
/// display window (`DisplayVStart/End = (28, 256)`), on both halves of the
/// double buffer and in every state of this corpus - read with
/// `mednafen-state vram-dump --regs`. `114` is exactly `228 / 2`, i.e.
/// `SetGeomOffset(width / 2, height / 2)` for the screen retail actually
/// displays. A port that keeps a 240-row frame still has to write `114`,
/// because the 2D chrome it draws beside the 3D is authored in retail's
/// coordinates; what it must not do is *derive* the centre from its own frame
/// height. See `docs/subsystems/renderer.md` § "The screen the GTE projects
/// onto is 320x224, not 320x240".
#[test]
fn the_vertical_centre_is_the_centre_of_a_228_line_display_not_a_240_line_one() {
    assert_eq!(EXPECT_OFY >> 16, 114);
    assert_eq!((EXPECT_OFY >> 16) * 2, 228);
    assert_eq!((EXPECT_OFY >> 16) + 6, 240 / 2);
    assert_eq!(EXPECT_OFX >> 16, 320 / 2);
}
