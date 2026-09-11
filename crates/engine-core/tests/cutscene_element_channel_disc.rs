//! Disc-gated: the **element channel**'s provenance, taken from the field
//! overlay's own bytes rather than from a dump filename.
//!
//! `crate::cutscene_script_elements` used to say there was "no element-actor
//! dispatch to hang these off". For the ambient emitter (`FUN_801D6058`) that
//! is falsified by the disc: its `+0x08` handler word sits in a `0x18`-byte
//! spawn descriptor at `0x801F271C`, inside the field overlay's own
//! plain-template table - the same table, and byte-for-byte the same shape, as
//! the three templates the engine already hosts:
//!
//! | template | handler | what it drives |
//! |---|---|---|
//! | `0x801F271C` | `FUN_801D6058` | the ambient particle emitter |
//! | `0x801F27EC` | `FUN_801DA930` | one rung of the floor-height ladder |
//! | `0x801F2840` | `FUN_801DD4C4` | an eased position move |
//! | `0x801F2858` | `FUN_801DD784` | the cinematic shutter bars |
//!
//! Asserting the shape from the real image is what keeps that reading honest:
//! the four rows are one family only if the bytes say so.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated
//! convention).

use std::path::PathBuf;

use legaia_engine_core::world::{
    AMBIENT_EMITTER_SCENE_ARM, AMBIENT_EMITTER_TEMPLATE_VA, ElementLink, SceneMode, World,
};

/// Load base of the field overlay (`crates/asset/data/static-overlays.toml`).
const FIELD_BASE: u32 = 0x801C_E818;

/// Descriptor stride of the plain-template family.
const TEMPLATE_STRIDE: usize = 0x18;

/// The three sibling templates the engine already hosts, plus the emitter's.
const TEMPLATES: [(u32, u32, &str); 4] = [
    (0x801F_271C, 0x801D_6058, "ambient particle emitter"),
    (0x801F_27EC, 0x801D_A930, "floor-height ladder rung"),
    (0x801F_2840, 0x801D_D4C4, "eased position move"),
    (0x801F_2858, 0x801D_D784, "cinematic shutter bars"),
];

fn field_overlay() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let p = PathBuf::from(c).join("overlays/overlay_field_0897.bin");
        if p.exists() {
            return std::fs::read(&p).ok();
        }
    }
    eprintln!("[skip] extracted/overlays/overlay_field_0897.bin missing");
    None
}

fn word(img: &[u8], va: u32) -> Option<u32> {
    let off = va.checked_sub(FIELD_BASE)? as usize;
    let b = img.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The four templates are one family, and the emitter's is one of them.
#[test]
fn the_ambient_emitter_is_a_field_overlay_plain_template() {
    let Some(img) = field_overlay() else { return };
    for (base, handler, what) in TEMPLATES {
        // `[u32 0][u16 0][u16 0xFFFF][u32 handler][u32 0][u32 0][u32 0]`.
        assert_eq!(word(&img, base), Some(0), "{what}: +0x00");
        assert_eq!(word(&img, base + 4), Some(0xFFFF_0000), "{what}: +0x04");
        assert_eq!(
            word(&img, base + 8),
            Some(handler),
            "{what}: +0x08 must be its handler"
        );
        for k in (12..TEMPLATE_STRIDE as u32).step_by(4) {
            assert_eq!(word(&img, base + k), Some(0), "{what}: +0x{k:02X}");
        }
        eprintln!("[ok] template 0x{base:08X} -> FUN_{handler:08X} ({what})");
    }
    assert_eq!(TEMPLATES[0].0, AMBIENT_EMITTER_TEMPLATE_VA);
}

/// The spawn site: `FUN_80024C88(&pos, 0x801F271C, pool)` at `0x801D6FD8`,
/// followed by `sh $s0, 0x1a($v0)` - the store that selects the emitter's
/// scene arm on the actor the spawn returned.
///
/// The instruction words are asserted, not the disassembler's rendering: the
/// `jal` target is a property of the bytes and survives any load base, and the
/// `sh` is what pins the arm.
#[test]
fn the_emitter_spawn_site_selects_the_scene_arm() {
    let Some(img) = field_overlay() else { return };
    // `lui $a1, 0x801f` / `addiu $a1, $a1, 0x271c` - the descriptor materialised
    // into the spawn's second argument.
    assert_eq!(word(&img, 0x801D_6FBC), Some(0x3C05_801F), "lui a1, 0x801f");
    assert_eq!(
        word(&img, 0x801D_6FC0),
        Some(0x24A5_271C),
        "addiu a1, a1, 0x271c"
    );
    // `jal 0x80024c88` - target = (word & 0x03FFFFFF) << 2 within the 256 MB
    // segment, which is base-independent.
    let jal = word(&img, 0x801D_6FD8).expect("jal word");
    assert_eq!(jal >> 26, 0b000011, "opcode must be jal");
    assert_eq!(
        0x8000_0000 | ((jal & 0x03FF_FFFF) << 2),
        0x8002_4C88,
        "the positioned spawn primitive"
    );
    // `sh $s0, 0x1a($v0)` on the returned actor: `+0x1A` is the emitter's arm
    // selector, and `$s0` is 1 at this point (`addiu $s0, $zero, 1` at
    // 0x801D6F98).
    assert_eq!(
        word(&img, 0x801D_6FE0),
        Some(0xA450_001A),
        "sh s0, 0x1a(v0)"
    );
    assert_eq!(
        word(&img, 0x801D_6F98),
        Some(0x2410_0001),
        "addiu s0, zero, 1"
    );
    assert_eq!(AMBIENT_EMITTER_SCENE_ARM, 1);
}

/// The channel itself, driven by the world's own frame tick rather than by a
/// direct call: a tween element installed on a real world advances and retires
/// through `World::tick`, which is the tick both hosts run.
#[test]
fn the_channel_advances_from_the_worlds_own_frame_tick() {
    // Disc-free on purpose - this asserts the wiring, not the data - but it
    // lives here because it is the other half of the claim above.
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.field_frame_step = 1;
    w.field_npc_positions.insert(2, (0, 0));
    w.spawn_element_position_tween(
        ElementLink::Placement(2),
        Default::default(),
        legaia_engine_core::cutscene_script_elements::ElementVec {
            x: 256,
            y: 0,
            z: 512,
            w: 0,
        },
        0x400,
    );
    for _ in 0..32 {
        let _ = w.tick();
        if w.cutscene_elements.is_empty() {
            break;
        }
    }
    assert!(
        w.cutscene_elements.is_empty(),
        "the world tick must run the channel, not just `tick_cutscene_elements`"
    );
    assert_eq!(
        w.field_npc_positions.get(&2),
        Some(&(256, 512)),
        "and the tween must have written through its link"
    );
}
