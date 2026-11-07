//! Disc-gated: the Delilas siblings draw through their `stone` (Shadow Gate)
//! confrontation.
//!
//! `stone`'s partition-2 record 6 is the map-stone cutscene. The siblings
//! are partition-1 placements 12 / 13 / 14 (Gi, Lu, Che), parked in the
//! off-map hide box until the record seats them. Gi's entrance seats him
//! with `A3 13 39 46`, hides him with `CC 13 40 00 00 00 00` (`+0x72 = 0`,
//! the "do not draw" scale) and grows him back with `CC 13 40 00 10 08 00`,
//! op `4C` nibble-4 sub-0's ramp arm, which retail schedules on whichever
//! actor the op resolved (`0x801E118C..0x801E11A8` in PROT 0897, the
//! `FUN_8003C5F0` exit shared with sub-2). The engine ran that ramp on the
//! player only, so Gi stood at scale 0 for the whole scene: talking, with
//! nobody there.
//!
//! This plays the record from scene entry with a confirm edge every eighth
//! frame and pins, through `World::field_npc_draw_pose` (the pose both play
//! hosts draw NPCs from): all three siblings draw, Gi at full scale on the
//! tile his `MoveTo` names, and for most of the scene.
//!
//! Structural assertions only - no Sony bytes. Skip-passes without
//! `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;

/// The placement slots of Gi, Lu and Che (`model = 33 / 34 / 35`).
const SIBLINGS: [u8; 3] = [12, 13, 14];

/// Gi's seat: `A3 13 39 46`, tile `(57, 70)` at the tile centre.
const GI_SEAT: (i16, i16) = (57 * 128 + 64, 70 * 128 + 64);

#[test]
fn stone_confrontation_draws_every_delilas_sibling() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("stone", 0).expect("enter stone");
    let scene = host.scene.as_ref().expect("scene loaded");
    let man = scene
        .field_man_payload(&host.index)
        .expect("MAN read")
        .expect("stone carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("MAN parses");
    let placements = man_file.actor_placements(&man);
    for (slot, model) in SIBLINGS.into_iter().zip([33u8, 34, 35]) {
        let p = placements
            .iter()
            .find(|p| p.index == usize::from(slot))
            .expect("sibling placement");
        assert_eq!(p.model_index, model, "slot {slot} is a Delilas mesh");
    }
    assert!(
        host.world.install_gated_p2_record(&man_file, &man, 6),
        "P2[6] installs (its C1 latch 0x590 is clear on a fresh scene)"
    );

    // Off-map spawn anchor: only a live position can put a sibling on stage.
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    let mut drawn_frames = [0u32; 3];
    let mut gi_first_drawn: Option<(i16, i16, f32)> = None;
    let mut frames = 0u32;
    for t in 0..8000u32 {
        let pad = if t % 8 == 0 {
            legaia_engine_system::input::PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        let _ = host.world.tick();
        frames = t;
        for (i, slot) in SIBLINGS.into_iter().enumerate() {
            if let Some(pose) = host.world.field_npc_draw_pose(slot, (hide, hide)) {
                drawn_frames[i] += 1;
                if slot == 12 && gi_first_drawn.is_none() {
                    gi_first_drawn = Some((pose.pos[0] as i16, pose.pos[2] as i16, pose.scale));
                }
            }
        }
        if host.world.cutscene.timeline.is_none() {
            break;
        }
    }
    eprintln!("stone P2[6]: {frames} frames, drawn per sibling {drawn_frames:?}");
    assert!(
        host.world.cutscene.timeline.is_none(),
        "the confrontation runs to its end"
    );
    for (slot, n) in SIBLINGS.into_iter().zip(drawn_frames) {
        assert!(n > 0, "sibling slot {slot} draws during the cutscene");
    }
    let (x, z, _) = gi_first_drawn.expect("Gi draws");
    assert_eq!((x, z), GI_SEAT, "Gi first draws on his MoveTo tile");
    // Gi is on stage from his entrance to the closing `A3 13 7F 7F` park:
    // most of the record, not a stray frame.
    assert!(
        drawn_frames[0] > frames / 2,
        "Gi draws for most of the scene ({} of {frames} frames)",
        drawn_frames[0]
    );
}
