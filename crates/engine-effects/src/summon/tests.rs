use super::*;
use legaia_overlay_images::summon_overlay::{
    RENDER_NODE_MODE_A, RENDER_NODE_MODE_B, SummonOverlay, SummonPart,
};

/// The images that call `FUN_801F3D3C`: `0903..=0923` minus 0907
/// (Nighto) and 0916 (Aluru) - base and evolved blocks alike, Spoon
/// included; nothing from the flute block up.
#[test]
fn side_effect_stager_callers_are_the_base_and_evolved_blocks() {
    let stagers: Vec<u8> = (0..=u8::MAX)
        .filter(|&id| module_stages_side_effect(id))
        .collect();
    let want: Vec<u8> = (0x81..=0x95)
        .filter(|&id| id != 0x85 && id != 0x8E)
        .collect();
    assert_eq!(stagers, want);
    assert!(
        module_stages_side_effect(0x91),
        "Spoon stages its cure tier"
    );
}

/// A synthetic overlay: one transform node + one mesh part with a tiny
/// move-VM program (`0x00 ANIM_BANK_SET 1,2,3` then `0x08 HALT`) - the
/// anim banks are the per-part position the render-side latch reads.
fn synthetic() -> (Vec<u8>, SummonOverlay) {
    // Record layout: [i16 model_sel][u16 reserved][u16 move-VM bytecode...].
    // Build two records back-to-back in one byte buffer.
    let mut bytes = Vec::new();
    // record 0 @ 0x00: model_sel = -1 (transform node)
    let r0 = bytes.len();
    bytes.extend_from_slice(&(-1i16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
    bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT
    bytes.extend_from_slice(&0u16.to_le_bytes()); // pad to even record
    // record 1 @ next: model_sel = 0 (mesh), program:
    // ANIM_BANK_SET 1,2,3 ; WAIT_SET 1 ; HALT. Op 0x00 sets
    // anim_3c/3e/40 = v << 3 -> (8, 16, 24); the one-frame wait keeps the
    // part alive past its seat run (a part that halts there is killed
    // before its first tick), and the seat frame's tick then halts it.
    let r1 = bytes.len();
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0x00u16.to_le_bytes()); // ANIM_BANK_SET
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&0x09u16.to_le_bytes()); // WAIT_SET
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT

    let overlay = SummonOverlay {
        link_base: 0x801F_69D8,
        spawn_sites: 2,
        parts: vec![
            SummonPart {
                record_off: r0,
                model_sel: -1,
                reserved: 0,
                bytecode: (r0 + 4)..r1,
            },
            SummonPart {
                record_off: r1,
                model_sel: 0,
                reserved: 0,
                bytecode: (r1 + 4)..bytes.len(),
            },
        ],
    };
    (bytes, overlay)
}

struct H;
impl MoveHost for H {}

/// Every part is seated at the spawn call's render scale before its first
/// VM run: `FUN_80021B04` stores `$a3` at `+0x72` (`0x80021DAC`), and the
/// stagers pass `0x1000`. A zero scale draws a sprite-arm sheet as a point.
#[test]
fn parts_seat_at_the_spawn_render_scale() {
    let (bytes, overlay) = synthetic();
    let scene = SummonScene::spawn_parts(&overlay.parts, &bytes, 0, [0, 0, 0]);
    assert!(!scene.parts.is_empty());
    for p in &scene.parts {
        assert_eq!(p.state.field_72, SPAWN_RENDER_SCALE);
    }
}

/// A ribbon carrier (a `model_sel -1` node that runs move-VM op `0x42`)
/// keeps its emitter arguments across ticks and reaches the ribbon draw
/// list. The translation glide used to treat `+0x9C` / `+0x9E` as its
/// clock on every node, advancing the packed count by `frame_delta` each
/// tick (`12` read back as `0x040C` after one) and clearing the `0x2000`
/// ribbon flag at its latch.
#[test]
fn a_ribbon_node_keeps_its_emitter_words_and_draws() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(-1i16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let program: [u16; 17] = [
        0x42, 1, 12, 0, 28, 96, 0x3039, 0, 512, 0x80, 0x80, 0x80, 0x40, 0x40, 0x40, 0x09, 0x0FFF,
    ];
    for w in program {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    let parts = vec![SummonPart {
        record_off: 0,
        model_sel: -1,
        reserved: 0,
        bytecode: 4..bytes.len(),
    }];
    let mut scene = SummonScene::spawn_parts(&parts, &bytes, 0, [0, 0, 0]);
    for _ in 0..5 {
        scene.tick(&mut H, 0x0400);
    }
    let st = &scene.parts[0].state;
    assert_eq!(st.actor_u16(0x9C), 12, "the packed count is not a clock");
    assert_eq!(st.field_9e, 0x2001, "the ribbon flag survives");
    assert!(
        scene.part_draws().is_empty(),
        "a transform node has no mesh part"
    );
    let ribbons = scene.ribbon_draws();
    assert_eq!(ribbons.len(), 1);
    assert_eq!(
        ribbons[0].mesh.indices.len(),
        12 * 6 * 6,
        "six GT4 packets a step"
    );
}

#[test]
fn spawns_one_state_per_part() {
    let (bytes, ov) = synthetic();
    let scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
    assert_eq!(scene.parts.len(), 2);
    assert_eq!(scene.mesh_part_count(), 1, "one mesh part (model_sel >= 0)");
    // Each part seeded at the origin with PC = 2.
    for part in &scene.parts {
        assert_eq!(part.state.pc, 2);
        assert_eq!(part.state.world_x, 100);
    }
}

#[test]
fn tick_latches_anim_banks_into_world_pos_plus_origin() {
    let (bytes, ov) = synthetic();
    let mut scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
    let mut host = H;
    scene.tick(&mut host, 0x1000);
    // The mesh part ran ANIM_BANK_SET 1,2,3 (-> anim = 8,16,24). With the
    // keyframe gate held (field_9c == field_9e == 0), the render-side latch
    // overwrites world pos with origin + anim bank.
    let mesh = scene.parts.iter().find(|p| p.model_sel == 0).unwrap();
    assert_eq!(
        (mesh.state.anim_3c, mesh.state.anim_3e, mesh.state.anim_40),
        (8, 16, 24),
        "op 0x00 set the anim banks to v << 3"
    );
    assert_eq!(
        (mesh.state.world_x, mesh.state.world_y, mesh.state.world_z),
        (108, 216, 324),
        "latch: world = origin + anim bank"
    );
    // Both tiny programs HALT on the first frame.
    scene.tick(&mut host, 0x1000);
    assert!(scene.finished(), "both parts halted");
}

#[test]
fn lerp_axis_matches_fun_801de4c8_mode1() {
    // Port of `FUN_801DE4C8(a, b, t, D, 1)`: returns `b + (a-b)*t/D` with
    // integer truncating div, and `a` exactly when `t >= D` or `a == b`.
    assert_eq!(lerp_axis(100, 0, 0, 10), 0, "t=0 → start");
    assert_eq!(lerp_axis(100, 0, 5, 10), 50, "midpoint");
    assert_eq!(lerp_axis(100, 0, 10, 10), 100, "t=D → exact target");
    assert_eq!(lerp_axis(100, 0, 11, 10), 100, "t>D → clamped to target");
    assert_eq!(lerp_axis(50, 50, 3, 10), 50, "a==b → target");
    // Truncation toward zero (retail `div`): (10-0)*3/10 = 3.
    assert_eq!(lerp_axis(10, 0, 3, 10), 3);
    // Negative direction truncates toward zero too: (0-10)*3/10 = -3.
    assert_eq!(lerp_axis(0, 10, 3, 10), 7);
}

#[test]
fn tick_drives_the_lerp_update_for_a_live_part() {
    // The integration path: one `tick` runs the move VM (which sets the
    // anim banks to 8/16/24) and then `apply_translation_update`. With a
    // fresh part (`+0x9E == 0`), that is the snap-to-target arm.
    let (bytes, ov) = synthetic();
    let mut scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
    let mut host = H;
    scene.tick(&mut host, 0x1000);
    let mesh = scene.parts.iter().find(|p| p.model_sel == 0).unwrap();
    assert_eq!(
        (mesh.state.world_x, mesh.state.world_y, mesh.state.world_z),
        (108, 216, 324),
        "tick wires apply_translation_update (snap arm: origin + anim bank)"
    );
}

#[test]
fn lerp_update_glides_toward_target_and_lands_exactly_on_completion() {
    // Drive `apply_translation_update` (the FUN_801F811C-shaped glide) across
    // several frames with an active interpolation window (+0x9E = duration)
    // and assert the world position moves monotonically toward the
    // (origin + anim-bank) target and reaches it exactly on completion.
    let origin = [100_i16, 200, 300];
    let mut state = ActorState::new();
    // Anim banks (summon-local target offsets); the move VM op 0x00 would
    // have set these to v << 3 = (8, 16, 24).
    state.anim_3c = 8;
    state.anim_3e = 16;
    state.anim_40 = 24;
    // Start at the bare origin, with a 40-tick interpolation window.
    state.world_x = origin[0];
    state.world_y = origin[1];
    state.world_z = origin[2];
    state.world_y_mirror = origin[1];
    state.field_9c = 0;
    state.field_9e = 40;

    let target = [108_i16, 216, 324]; // origin + (8, 16, 24)

    // One step (t advances 0 → 10, D = 40): x = 100 + (108-100)*10/40 = 102.
    apply_translation_update(&mut state, origin, 10);
    assert_eq!(
        (state.world_x, state.world_y, state.world_z),
        (102, 204, 306),
        "first lerp step is partial, not a snap"
    );
    assert_ne!(state.field_9e, 0, "still mid-tween");
    assert_eq!(state.world_y_mirror, state.world_y, "y mirror tracks y");

    // Continue ticking; assert monotonic approach and exact landing.
    let mut prev = [state.world_x, state.world_y, state.world_z];
    let mut reached = false;
    for step in 0..6 {
        apply_translation_update(&mut state, origin, 10);
        let pos = [state.world_x, state.world_y, state.world_z];
        for axis in 0..3 {
            assert!(
                pos[axis] >= prev[axis] && pos[axis] <= target[axis],
                "axis {axis} step {step}: {} not in [{}, {}]",
                pos[axis],
                prev[axis],
                target[axis]
            );
        }
        if pos == target {
            reached = true;
            assert_eq!(state.field_9e, 0, "duration cleared on completion");
            break;
        }
        prev = pos;
    }
    assert!(reached, "part reached the target exactly on completion");

    // After completion, +0x9E == 0, so the next update is the snap arm and
    // the part stays pinned exactly on the target.
    apply_translation_update(&mut state, origin, 10);
    assert_eq!(
        (state.world_x, state.world_y, state.world_z),
        (target[0], target[1], target[2]),
        "post-completion snap holds on target"
    );
}

#[test]
fn latch_holds_part_at_origin_when_anim_banks_are_zero() {
    // The transform-node part (no anim ops) stays at the origin after the
    // latch (origin + 0).
    let (bytes, ov) = synthetic();
    let mut scene = SummonScene::spawn(&ov, &bytes, 26, [40, 50, 60]);
    let mut host = H;
    scene.tick(&mut host, 0x1000);
    let node = &scene.parts[0];
    assert_eq!(
        (node.state.world_x, node.state.world_y, node.state.world_z),
        (40, 50, 60)
    );
}

#[test]
fn summon_prot_entry_maps_the_seru_block() {
    assert_eq!(summon_stager_prot_entry(0x81), Some(903)); // Gimard Burning Attack
    assert_eq!(summon_stager_prot_entry(0x8B), Some(913)); // last base-block summon
    assert_eq!(summon_stager_prot_entry(0x80), None); // below the block
    assert_eq!(summon_stager_prot_entry(0x27), None); // a monster attack id
}

#[test]
fn summon_prot_entry_maps_the_evolved_block() {
    // The evolved-Seru block bridges the base and high blocks on one
    // continuous arithmetic run; stager-shaped, three legs capture-pinned.
    assert_eq!(summon_stager_prot_entry(0x8C), Some(914)); // Gola Gola (capture-pinned)
    assert_eq!(summon_stager_prot_entry(0x8D), Some(915)); // Mushura (capture-pinned)
    assert_eq!(summon_stager_prot_entry(0x8E), Some(916)); // carries 0x4000 render-mode nodes
    assert_eq!(summon_stager_prot_entry(0x8F), Some(917)); // Barra (capture-pinned)
    assert_eq!(summon_stager_prot_entry(0x93), Some(921)); // carries 0x4000 render-mode nodes
    assert_eq!(summon_stager_prot_entry(0x95), Some(923)); // last evolved-Seru leg
}

#[test]
fn summon_prot_entry_maps_the_flute_block() {
    // 0x96..=0x98 used to be read as a hole ("the enemy 895+id block, not
    // a player summon"). That is falsified: both live legs are
    // capture-pinned mid-cast on the *player* path (loader-B 0x1D/0x1E,
    // slot-B head byte-identical to the disc entry), and SummonFlute items
    // (item-effect classes 126/127) enqueue the spell id directly.
    assert_eq!(summon_stager_prot_entry(0x96), Some(924)); // Lippian
    assert_eq!(summon_stager_prot_entry(0x97), Some(925)); // Spikefish
    assert_eq!(summon_stager_prot_entry(0x98), Some(926)); // unused in retail
}

#[test]
fn the_player_summon_run_is_one_unbroken_line() {
    // Every id from the base block through the high block resolves under
    // the single loader arithmetic, with no special-cased gap.
    for (i, id) in PLAYER_SUMMON_IDS.clone().enumerate() {
        assert_eq!(
            summon_stager_prot_entry(id),
            Some(903 + i as u32),
            "spell id {id:#04x}"
        );
    }
    assert_eq!(summon_stager_prot_entry(0x80), None);
    assert_eq!(summon_stager_prot_entry(0xA1), None);
}

#[test]
fn summon_prot_entry_maps_the_high_block() {
    assert_eq!(summon_stager_prot_entry(0x99), Some(927)); // Evil Seru Magic (Juggernaut cast)
    assert_eq!(summon_stager_prot_entry(0x9A), Some(928)); // Palma
    assert_eq!(summon_stager_prot_entry(0x9D), Some(931)); // Jedo
    assert_eq!(summon_stager_prot_entry(0x9E), Some(932)); // Meta
    assert_eq!(summon_stager_prot_entry(0xA0), Some(934)); // Ozma
    assert_eq!(summon_stager_prot_entry(0xA1), None); // above the high block
}

#[test]
fn big_summons_cover_the_seven_named_summons_exactly() {
    // The seven names players know them by, each on exactly one id, in one
    // contiguous run that is the whole four-slot band.
    let ids: Vec<u8> = BIG_SUMMONS.iter().map(|s| s.spell_id).collect();
    assert_eq!(ids, (0x9A..=0xA0).collect::<Vec<u8>>());
    for want in ["Meta", "Ozma", "Terra", "Horn", "Jedo", "Palma", "Mule"] {
        let s = big_summon_by_name(want).unwrap_or_else(|| panic!("{want} resolves"));
        assert_eq!(s.name, want);
        assert!(!s.attack.is_empty(), "{want} names its cast");
    }
    // Every element id is distinct and inside the 0..=7 space - a
    // duplicated element would mean a row was copied, not read.
    let mut elems: Vec<u8> = BIG_SUMMONS.iter().map(|s| s.element).collect();
    elems.sort_unstable();
    assert_eq!(elems, vec![0, 1, 2, 3, 4, 5, 6]);
    // 0x99 (Evil Seru Magic) shares the high block but not the band.
    assert!(big_summon(0x99).is_none());
    assert!(big_summon(0xA1).is_none());
    assert!(big_summon_by_name("Juggernaut").is_none());
}

#[test]
fn display_names_span_the_whole_player_summon_run() {
    // Base / evolved: the namesake battle_data creature.
    assert_eq!(summon_display_name(0x81), Some("Gimard"));
    assert_eq!(summon_display_name(0x95), Some("Gilium"));
    // Big-summon band: the summon's own name, not a creature id.
    assert_eq!(summon_display_name(0x9E), Some("Meta"));
    assert_eq!(summon_display_name(0xA0), Some("Ozma"));
    assert_eq!(summon_display_name(0x9A), Some("Palma"));
    // The flute pair, which neither of the other two maps covers.
    assert_eq!(summon_display_name(0x96), Some("Lippian"));
    assert_eq!(summon_display_name(0x97), Some("Spikefish"));
    // 0x98 is retail's unused slot and 0x99's creature resolves per cast,
    // so neither gets a name here - the host falls back to the disc's own
    // attack string rather than guessing.
    assert_eq!(summon_display_name(0x98), None);
    assert_eq!(summon_display_name(0x99), None);
    // Outside the run.
    assert_eq!(summon_display_name(0x80), None);
    assert_eq!(summon_display_name(0xA1), None);
}

#[test]
fn part_draws_map_model_sel_against_the_base() {
    let (bytes, ov) = synthetic();
    let scene = SummonScene::spawn(&ov, &bytes, 26, [0, 0, 0]);
    let draws = scene.part_draws();
    assert_eq!(draws.len(), 1, "only the mesh part draws");
    assert_eq!(draws[0].model_index, 26, "model_sel 0 + base 26");
    assert_eq!(draws[0].flags_52, 0, "no op 0x15: the full-camera default");
}

/// Move-VM op `0x15` writes the part's `+0x52` word, and the draw record
/// carries it to the host, whose part pass resolves the `0x780` bits
/// through `FUN_8001CF50`'s placement. `0x380` is the screen-aligned
/// billboard the battle-effect parts stage.
#[test]
fn op_0x15_reaches_the_part_draw_record() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for w in [0x15u16, 0x0380, 0x09, 0x0FFF] {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    let parts = vec![SummonPart {
        record_off: 0,
        model_sel: 0,
        reserved: 0,
        bytecode: 4..bytes.len(),
    }];
    let mut scene = SummonScene::spawn_parts(&parts, &bytes, 0, [0, 0, 0]);
    scene.tick(&mut H, 0x0400);
    assert_eq!(scene.parts[0].state.field_52, 0x0380);
    let draws = scene.part_draws();
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].flags_52, 0x0380);
}

#[test]
fn render_mode_classifies_only_the_sentinel_nodes() {
    // FUN_80021B04: 0x4000 -> +0x5A=3 (Particle), 0x4001 -> +0x5A=5 (Sound).
    assert_eq!(
        RenderMode::from_model_sel(0x4000),
        Some(RenderMode::Particle)
    );
    assert_eq!(
        RenderMode::from_model_sel(0x4001),
        Some(RenderMode::SoundEmitter)
    );
    assert_eq!(RenderMode::Particle.raw(), 3);
    assert_eq!(RenderMode::SoundEmitter.raw(), 5);
    // Mesh parts and transform nodes are not statically classified.
    assert_eq!(RenderMode::from_model_sel(0), None);
    assert_eq!(RenderMode::from_model_sel(-1), None);
    assert_eq!(RenderMode::from_model_sel(5), None);
}

#[test]
fn special_render_nodes_are_split_from_the_mesh_draw_list() {
    // One mesh part (sel 0) + a 0x4000 particle node + a 0x4001 sound node.
    let mut bytes = Vec::new();
    let mut part = |sel: i16| {
        let off = bytes.len();
        bytes.extend_from_slice(&sel.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // reserved
        bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT
        bytes.extend_from_slice(&0u16.to_le_bytes()); // pad
        SummonPart {
            record_off: off,
            model_sel: sel,
            reserved: 0,
            bytecode: (off + 4)..(off + 6),
        }
    };
    let parts = vec![part(0), part(RENDER_NODE_MODE_A), part(RENDER_NODE_MODE_B)];
    let scene = SummonScene::spawn_parts(&parts, &bytes, 26, [10, 20, 30]);

    // The mesh draw list carries only the real mesh part - the two sentinel
    // render nodes (incl. the audio-only 0x4001) never mesh-draw.
    let draws = scene.part_draws();
    assert_eq!(draws.len(), 1, "sentinel render nodes never mesh-draw");
    assert_eq!(draws[0].model_index, 26);

    // The render-mode nodes surface through the dedicated channel instead.
    let nodes = scene.special_render_nodes();
    assert_eq!(nodes.len(), 2, "the 0x4000 + 0x4001 nodes");
    assert_eq!(nodes[0].mode, RenderMode::Particle);
    assert_eq!(nodes[1].mode, RenderMode::SoundEmitter);
    assert_eq!(nodes[1].world_pos, [10.0, 20.0, 30.0]);
}

/// A captured cast's parts seat over the replay's parts of the same record
/// in order: a record with fewer seeds than live parts retires the rest, one
/// with more clones its first live part, and a record no seed names, or a
/// seed whose record has no live part, is left alone.
#[test]
fn install_part_snapshot_reseats_parts_by_record() {
    let (bytes, overlay) = synthetic();
    let mut scene = SummonScene::spawn_parts(&overlay.parts, &bytes, 0, [0, 0, 0]);
    let template = scene.parts[1].clone();
    scene.parts.clear();
    for (tag, n) in [(0x801F_876C_u32, 3), (0x801F_86B8, 1)] {
        for _ in 0..n {
            let mut p = template.clone();
            p.tag = Some(tag);
            scene.parts.push(p);
        }
    }
    let ray = |roll: u16, age: u16| CastPartSeed {
        tag: 0x801F_876C,
        pokes: vec![(0x28, roll), (0xB6, age), (0x16, 0x10)],
    };
    scene.install_part_snapshot(&[
        ray(0x2CC, 0x80),
        ray(0x723, 0x100),
        CastPartSeed {
            tag: 0x801F_9999,
            pokes: vec![(0x28, 1)],
        },
    ]);
    let live_rays: Vec<_> = scene
        .parts
        .iter()
        .filter(|p| !p.finished && p.tag == Some(0x801F_876C))
        .collect();
    assert_eq!(live_rays.len(), 2, "the third ray is retired");
    assert_eq!(live_rays[0].state.render_28, 0x2CC);
    assert_eq!(live_rays[0].state.actor_u16(0xB6), 0x80);
    assert_eq!(live_rays[1].state.render_28, 0x723);
    assert_eq!(live_rays[1].state.world_y_mirror, 0x10);
    assert!(
        scene
            .parts
            .iter()
            .any(|p| !p.finished && p.tag == Some(0x801F_86B8)),
        "a record no seed names is kept"
    );
    assert_eq!(
        scene.parts.len(),
        4,
        "a seed with no live record spawns nothing"
    );

    scene.install_part_snapshot(&[ray(1, 0), ray(2, 0), ray(3, 0)]);
    let live = scene
        .parts
        .iter()
        .filter(|p| !p.finished && p.tag == Some(0x801F_876C))
        .count();
    assert_eq!(live, 3, "a seed past the live count clones a live part");
}
