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
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

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

/// Census of `FUN_8004998C`'s Euler-flip retry over the monster archive
/// (PROT 867): every decodable action clip, every in-clip frame pair
/// `f -> f + 1`, every non-zero nibble `1..=15`, every part. Counts the part
/// samples whose summed angle step exceeds `0xC00` (the retry) and those whose
/// angles the retry actually changes, and proves the whole-frame samples
/// (nibble `0`, the blend arm skipped) are the plain decode - both through the
/// kernel and through [`MonsterAnimPlayer`] ticking a whole keyframe at a time.
#[test]
fn monster_archive_pose_blend_retry_census() {
    use legaia_engine_vm::battle_pose_blend::{EULER_FLIP_THRESHOLD, blend_part_pose};
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let archive = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .expect("read monster archive");
    let records = legaia_asset::monster_archive::records(&archive).expect("decode archive");

    let (mut clips, mut blended, mut retried, mut changed) = (0u64, 0u64, 0u64, 0u64);
    let (mut whole, mut clips_with_retry) = (0u64, 0u64);
    for r in &records {
        let Ok(Some(anims)) = legaia_asset::monster_archive::animations(&archive, r.id) else {
            continue;
        };
        for anim in &anims {
            clips += 1;
            let mut any = false;
            for f in 0..anim.frame_count {
                for p in 0..anim.part_count {
                    let cur = anim.frames[f][p];
                    // Whole-frame sample: the plain decode.
                    let w = blend_part_pose(cur, cur, 0, 0);
                    assert_eq!(w.translation, [cur.tx, cur.ty, cur.tz]);
                    assert_eq!(w.rotation, [cur.rx, cur.ry, cur.rz]);
                    whole += 1;
                    if f + 1 >= anim.frame_count {
                        continue;
                    }
                    let next = anim.frames[f + 1][p];
                    for frac in 1..=15u8 {
                        let b = blend_part_pose(cur, next, frac, 0);
                        blended += 1;
                        if b.retried {
                            retried += 1;
                            any = true;
                            // The no-retry angles, for the "changed" count.
                            let plain = |n: u16, c: u16| {
                                legaia_engine_vm::battle_pose_blend::lerp_battle_angle(
                                    i32::from(n),
                                    i32::from(c),
                                    i32::from(frac),
                                )
                            };
                            let (x, y, z) = (
                                plain(next.rx, cur.rx),
                                plain(next.ry, cur.ry),
                                plain(next.rz, cur.rz),
                            );
                            assert!(x.magnitude + y.magnitude + z.magnitude > EULER_FLIP_THRESHOLD);
                            if b.rotation != [x.value, y.value, z.value] {
                                changed += 1;
                            }
                        }
                    }
                }
            }
            if any {
                clips_with_retry += 1;
            }
            // Through the player: whole-keyframe ticks land on nibble 0 and
            // pose each frame exactly as decoded.
            if let Some(mut player) = MonsterAnimPlayer::new(anim) {
                player.step = 256;
                for _ in 0..anim.frame_count {
                    let pose = player.tick();
                    let f = player.current_frame() as usize;
                    for (p, (t, rot)) in pose.bone_outputs.iter().enumerate() {
                        let d = anim.frames[f][p];
                        assert_eq!(*t, [d.tx, d.ty, d.tz]);
                        assert_eq!(*rot, [d.rx as i16, d.ry as i16, d.rz as i16]);
                    }
                }
            }
        }
    }
    eprintln!(
        "[ok] battle pose blend census: {clips} clips; {whole} whole-frame part samples exact; \
         {retried} of {blended} blended part samples take the retry \
         ({changed} changed by it) in {clips_with_retry} clips"
    );
    assert!(clips > 0 && blended > 0);
}
