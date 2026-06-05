//! Disc-gated: a real monster's idle animation drives a changing posed mesh.
//!
//! Exercises the battle-actor animation pipeline end to end on real disc data
//! without the windowed shell: decode a monster's idle clip
//! (`monster_archive::idle_animation`, the `+0x8c` 9-byte TRS stream), build a
//! [`MonsterAnimPlayer`] and tick it, then deform the monster's TMD with the
//! rigid `R·v + T` builder ([`legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`]) -
//! asserting the posed mesh actually moves frame-to-frame and stays bounded.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (disc-gated convention).
use legaia_engine_core::battle_anim::MonsterAnimPlayer;
use legaia_rando::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(path).ok()
}

#[test]
fn real_monster_idle_animation_drives_a_moving_posed_mesh() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let archive = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .expect("read monster archive");

    let records = legaia_asset::monster_archive::records(&archive).expect("decode archive");

    // Find the first monster that (a) decodes a multi-frame, multi-part idle
    // clip, and (b) whose embedded TMD parses with one object per animated part
    // (the pose addresses every object). Not every slot animates, so scan.
    let mut chosen: Option<(
        u16,
        legaia_asset::monster_archive::MonsterAnimation,
        legaia_tmd::Tmd,
        Vec<u8>,
    )> = None;
    for r in &records {
        let Ok(Some(idle)) = legaia_asset::monster_archive::idle_animation(&archive, r.id) else {
            continue;
        };
        if idle.frame_count < 2 || idle.part_count < 2 {
            continue;
        }
        let Ok(Some(mesh)) = legaia_asset::monster_archive::mesh(&archive, r.id) else {
            continue;
        };
        let raw = mesh.tmd_bytes().to_vec();
        let Ok(tmd) = legaia_tmd::parse(&raw) else {
            continue;
        };
        // The pose has one transform per TMD object.
        if tmd.objects.len() != idle.part_count {
            continue;
        }
        chosen = Some((r.id, idle, tmd, raw));
        break;
    }

    let Some((id, idle, tmd, raw)) = chosen else {
        panic!("no monster with a usable multi-frame idle animation + matching TMD found");
    };
    eprintln!(
        "[battle-anim] monster {id}: idle {} frames x {} parts, TMD {} objects",
        idle.frame_count,
        idle.part_count,
        tmd.objects.len()
    );

    // Rest-pose (unposed) AABB for a bounds sanity reference.
    let rest = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &raw);
    assert!(!rest.positions.is_empty(), "rest mesh must have geometry");
    let (rlo, rhi) = rest.aabb();
    let span = (rhi[0] - rlo[0]).max(rhi[1] - rlo[1]).max(rhi[2] - rlo[2]);
    assert!(span.is_finite() && span > 0.0, "rest AABB span sane");

    let mut player = MonsterAnimPlayer::new(&idle).expect("build idle player");
    assert_eq!(player.part_count(), tmd.objects.len());
    // Step a quarter keyframe per tick so a handful of ticks visits distinct
    // sub-frames within the clip.
    player.step = 64;

    let mut frames: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut vert_count = None;
    for _ in 0..24 {
        let pose = player.tick();
        assert_eq!(
            pose.bone_outputs.len(),
            idle.part_count,
            "pose addresses every part"
        );
        let posed = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&tmd, &raw, &pose.bone_outputs);
        // Topology is stable across frames (same vertex count every frame).
        match vert_count {
            None => vert_count = Some(posed.positions.len()),
            Some(n) => assert_eq!(
                n,
                posed.positions.len(),
                "vertex count stable across frames"
            ),
        }
        // No NaN / explosion: every posed vertex stays within a generous
        // multiple of the rest-pose span around the origin.
        let bound = span * 8.0 + 4096.0;
        for p in &posed.positions {
            for k in 0..3 {
                assert!(
                    p[k].is_finite() && p[k].abs() <= bound,
                    "posed vertex out of bounds: {p:?} (bound {bound})"
                );
            }
        }
        frames.push(posed.positions);
    }

    // The animation is non-static: at least one pair of captured frames differs
    // by a visible amount on some vertex (the idle clip actually moves the
    // mesh, proving rotation/translation reached the geometry).
    let mut max_delta = 0.0f32;
    for w in frames.windows(2) {
        for (a, b) in w[0].iter().zip(w[1].iter()) {
            for k in 0..3 {
                max_delta = max_delta.max((a[k] - b[k]).abs());
            }
        }
    }
    assert!(
        max_delta > 0.5,
        "idle animation should move the mesh across frames (max delta {max_delta})"
    );
    eprintln!("[battle-anim] max per-vertex frame-to-frame delta = {max_delta:.2}");
}
