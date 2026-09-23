//! Disc-gated: the field VM's op `0x43` sub-0/1/A/B **scripted arc**, run
//! from a real cutscene record.
//!
//! `town0b`'s partition-2 record 26 pokes a string of NPC channels with
//! `C3 <id> 00 <tx> <tz> <apex> <frames>` - each NPC hops from where it
//! stands to a landing tile along an arc that peaks `apex` units above the
//! higher endpoint (`FUN_801D25EC` -> `FUN_801D5C08`), and its script stays
//! halted until the arc's release watcher (`FUN_801D5D60`) lands it.
//!
//! The engine used to read the operand's halfwords as resume PCs and bytes
//! `1..2` as one merged coordinate, and ran no arc at all. This pins, from
//! the disc bytes: every arc site in the scene decodes; the record reaches
//! one; the NPC leaves the ground by the apex, lands exactly on the decoded
//! tile at the floor height, and its channel's halt bit is clear after.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::ScriptActorRef;
use legaia_engine_vm::field_ledge_hop_arc::ScriptArcRequest;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn town0b_cutscene_arcs_an_npc_to_its_landing_tile_and_releases_it() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town0b", 0).expect("enter town0b");
    let man = host
        .scene
        .as_ref()
        .expect("scene loaded")
        .field_man_payload(&host.index)
        .expect("MAN read")
        .expect("town0b carries a MAN");
    let man_file = legaia_asset::man_section::parse(&man).expect("MAN parses");

    // Every arc the record carries decodes from the operand alone.
    let (start, pc0, len) =
        legaia_asset::field_disasm::partition_record_span(&man_file, &man, 2, 26)
            .expect("p2[26] span");
    let body = &man[start..start + len];
    let mut sites = Vec::new();
    for sub in [0u8, 1, 0xA, 0xB] {
        let key = legaia_asset::field_disasm::OpKey {
            opcode: 0x43,
            sub: Some(sub),
        };
        for pc in legaia_asset::field_disasm::clean_hit_offsets(body, pc0, key) {
            let hdr = if body[pc] & 0x80 != 0 { 2 } else { 1 };
            let req = ScriptArcRequest::decode(&body[pc + hdr..])
                .unwrap_or_else(|| panic!("arc at 0x{pc:04X} decodes"));
            assert!(req.frames > 0, "0x{pc:04X}: a positive clip length");
            assert!(
                req.landing_xz().is_some(),
                "0x{pc:04X}: this record's arcs all name a landing tile"
            );
            sites.push((pc, req));
        }
    }
    assert!(!sites.is_empty(), "p2[26] carries arc sites");
    eprintln!("[ok] town0b p2[26]: {} arc sites decode", sites.len());

    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, &man, 2, 26, false)
    );

    // Tick until an NPC arc starts, then follow it to its landing.
    let mut followed: Option<(u8, (i16, i16, i16))> = None;
    let mut peak_rise = 0i32;
    let mut landed = false;
    for t in 0..6000 {
        // Page through the record's dialog boxes: a confirm edge every
        // eighth frame, released in between.
        let pad = if t % 8 == 0 {
            legaia_engine_core::input::PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        let _ = host.world.tick();
        if std::env::var_os("ARC_DIAG").is_some() && t % 50 == 0 {
            let tl = host.world.cutscene.timeline.as_ref();
            eprintln!(
                "[diag] t={t} pc={:?} done={:?} bytes={:02X?} dialog={}",
                tl.map(|t| t.pc),
                tl.map(|t| t.done),
                tl.and_then(|t| t.bytecode.get(t.pc..t.pc + 12)),
                tl.is_some_and(|t| t.dialog.is_some())
            );
        }
        if followed.is_none()
            && let Some(a) = host
                .world
                .script_actors
                .arcs
                .iter()
                .find(|a| matches!(a.actor, ScriptActorRef::Npc(_)))
        {
            let ScriptActorRef::Npc(slot) = a.actor else {
                unreachable!()
            };
            followed = Some((slot, a.arc.end));
            eprintln!(
                "[arc] NPC slot {slot}: {:?} -> {:?}",
                a.arc.start, a.arc.end
            );
        }
        if let Some((slot, end)) = followed {
            let actor = ScriptActorRef::Npc(slot);
            if host.world.script_arc_live(actor) {
                let (x, y, z) = host.world.script_actor_position(actor).unwrap();
                let floor = host
                    .world
                    .sample_field_floor_height(i32::from(x), i32::from(z));
                // Y-down: a raised actor sits at a smaller Y than the floor.
                peak_rise = peak_rise.max(floor - i32::from(y));
            } else {
                let (x, y, z) = host.world.script_actor_position(actor).unwrap();
                assert_eq!((x, z), (end.0, end.2), "lands on the decoded tile");
                assert_eq!(y, end.1, "lands at the landing Y");
                let halted = host
                    .world
                    .field_vm
                    .channels
                    .iter()
                    .filter(|c| !c.object_bind && c.placement_index == usize::from(slot))
                    .any(|c| c.ctx.flags & 0x400 != 0);
                assert!(!halted, "the watcher released the NPC's script");
                landed = true;
                break;
            }
        }
    }
    let (slot, end) = followed.expect("the record reached an NPC arc");
    assert!(landed, "NPC slot {slot} landed at {end:?}");
    assert!(
        peak_rise > 16,
        "the NPC left the ground mid-arc (peak rise {peak_rise})"
    );
    eprintln!("[ok] NPC slot {slot} peaked {peak_rise} units up and landed at {end:?}");
}

/// `cave01`'s scene record carries `B4 F8 11 80 80 20 00 0C 00 0C 60 00 00 00`
/// (op `0x34` sub-1) against the player anchor with op0 bit 0 set: a
/// subtractive darkness mask of colour `(0x80, 0x80, 0x20)` with a lit hole
/// `0xC00` view units across, lifted `0x60` above the player's feet. Entering
/// the scene seats it on the player.
#[test]
fn cave01_seats_a_darkness_mask_on_the_player() {
    use legaia_engine_vm::field_actor_billboard::light_abr;
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("cave01", 0).expect("enter cave01");
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    let lights = &host.world.script_actors.lights;
    let l = lights
        .iter()
        .find(|l| l.parent == ScriptActorRef::Player)
        .unwrap_or_else(|| panic!("a light on the player; lights = {lights:?}"));
    assert_eq!(l.sprite.abr, light_abr::SUBTRACTIVE);
    assert_eq!((l.sprite.color_a, l.sprite.color_b), (0, 0x80_8020));
    assert_eq!(l.sprite.half_extent, (0xC00, 0xC00));
    assert_eq!(l.sprite.offset, (0, -0x60, 0));
    eprintln!(
        "[ok] cave01: darkness mask on the player ({} light(s))",
        lights.len()
    );
}
