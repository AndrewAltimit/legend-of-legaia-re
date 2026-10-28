//! Disc-gated: the move-VM framebuffer grab (render mode `7`) through the
//! real chain - `kor5`'s Genesis Tree beat (`P2[5]`) spawns its grab parts,
//! the ambient tick turns each into a request, `World::queue_vram_grabs`
//! projects it through a field camera into the `FUN_800468A4` call, and
//! `World::step_field_vram_effects` lands the copy in the scene VRAM.
//!
//! `P2[5]` raises system flag `0x0B` and fires `34 30 01` / `34 30 03`; the
//! stager records those spawn (`2`, `4`) spawn the grab records `11` / `12`
//! through move-VM op `0x25`, and each grab loops on flag `0x0B`
//! (`2F 13 000B 0001`). The play hosts pass the camera of the frame they last
//! drew; this test stands in a camera aimed at the part, which pins the
//! chain, not the framing.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn kor5_genesis_tree_grab_lands_in_vram() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("kor5", 0).expect("enter kor5");
    let man = host
        .scene
        .as_ref()
        .expect("scene")
        .field_man_payload(&host.index)
        .expect("read MAN")
        .expect("kor5 carries a MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");
    assert!(
        host.world
            .install_cutscene_timeline_record(&mf, &man, 2, 5, false)
    );
    let cross = legaia_engine_system::input::PadButton::Cross.mask();
    let mut grabs = 0usize;
    let mut landed = false;
    for t in 0..3000u32 {
        host.world.input.set_pad(if t % 2 == 0 { cross } else { 0 });
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
        let Some(part) = host
            .world
            .ambient
            .fx
            .iter()
            .find(|p| !p.finished && p.state.move_submode == 7)
        else {
            continue;
        };
        let req = legaia_engine_vm::vram_grab::grab_request(&part.state).expect("mode 7");
        // Retail's operand block, read through op 0x45's offsets.
        assert_eq!(req.size, [64, 64], "the grab is 64 x 64");
        assert_eq!(req.dest, [832, 0], "into (832, 0)");
        assert_eq!(req.offset, [-32, -32], "centred on the part");
        grabs += 1;
        let view = FieldCameraView {
            focus: req.pos.map(f32::from),
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
            h: 256.0,
            tr_eye: [0.0, 0.0, 600.0],
        };
        let res = host.resources.as_mut().expect("resources");
        // A marker where the point projects (`OFX = 160`, `OFY = 114`).
        let probe = 0x1234u16 + grabs as u16;
        res.vram.write_block(160, 114, 1, 1, &probe.to_le_bytes());
        // The headless host runs the drain itself each tick; queue the grab
        // the way a play host does and drain once more.
        if host.world.queue_vram_grabs(Some(&view)) == 0 {
            // The part was seated this tick; its arm runs from the next one.
            continue;
        }
        assert!(host.world.step_field_vram_effects(&mut res.vram, false));
        assert_eq!(
            res.vram.pixel(832 + 32, 32),
            probe,
            "the pixel under the projected point lands at the rect's centre"
        );
        landed = true;
        if grabs > 8 {
            break;
        }
    }
    eprintln!("[grab] kor5 P2[5]: {grabs} grab ticks seen");
    assert!(landed, "no grab part reached its render-mode-7 arm");
}
