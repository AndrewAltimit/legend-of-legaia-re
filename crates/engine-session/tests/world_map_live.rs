//! Disc-gated: `NativeSession::enter_world_map_live` loads a real world-map
//! scene, routes its region-keyed encounter table onto the overworld, installs
//! the player, and reaches `SceneMode::WorldMap` ready to roll encounters.
//!
//! This is the reachability test for the playable overworld: it asserts the
//! window's `--world-map` path (which now calls `enter_world_map_live`) ends up
//! with a region tracker installed and a player actor, then walks the player
//! across tiles and confirms a region encounter eventually flips into a battle.
//!
//! Skip-passes without disc data / extracted assets (the `LEGAIA_DISC_BIN`
//! convention) so CI works without Sony bytes.

use legaia_engine_save_state::WorldSaveState;
use std::path::PathBuf;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

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
fn world_map_live_installs_regions_and_player() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // map03 is a kingdom-bundle world-map scene with a populated region table.
    let cfg = BootConfig {
        scene: "map03".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let opts = FieldLiveOpts::default();
    let mode = session
        .enter_world_map_live("map03", &opts)
        .expect("enter_world_map_live");

    assert_eq!(mode, SceneMode::WorldMap, "ends in world-map mode");
    let world = &session.host.world;
    assert!(
        world.world_map.region_tracker.is_some(),
        "the scene's region table is routed onto the overworld"
    );
    assert!(
        world.player_actor_slot.is_some(),
        "an overworld player actor is installed"
    );
    let regions = world
        .world_map
        .region_tracker
        .as_ref()
        .map(|t| t.table().regions.len())
        .unwrap_or(0);
    assert!(regions > 0, "map03 routes encounter regions ({regions})");
    // Report whether the overworld scene actually carries a walkability grid
    // (the same `_DAT_1f8003ec + 0x4000` source the field uses). A non-empty
    // grid means the overworld collision path is genuinely exercised; an empty
    // grid means the scene has no MAP block and the player roams unbounded.
    let wall_nibbles: usize = world
        .terrain
        .collision_grid
        .iter()
        .map(|b| (b >> 4).count_ones() as usize)
        .sum();
    eprintln!(
        "[map03] world-map live: {regions} regions, player installed, \
         collision grid {} bytes ({wall_nibbles} wall sub-cells)",
        world.terrain.collision_grid.len()
    );
}

/// The world-map load must source its geometry from the kingdom bundle's
/// slot-1 landmark TMD pack, not the generic raw/LZS TMD sweep (which can't
/// follow the 7-asset descriptor table and yields only a handful of stray
/// meshes - the historical "battle-map look"). Slot-1 pack counts are pinned
/// by the disc: Drake 40, Sebacus 36, Karisto 56.
#[test]
fn world_map_load_uses_kingdom_landmark_pack() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // (scene, slot-1 landmark TMD count) per crates/asset/src/kingdom_bundle.rs
    // and verified against the disc.
    for (scene, landmarks) in [("map01", 40usize), ("map02", 36), ("map03", 56)] {
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
        let opts = FieldLiveOpts::default();
        session
            .enter_world_map_live(scene, &opts)
            .expect("enter_world_map_live");

        let res = session
            .host
            .resources
            .as_ref()
            .expect("world-map scene resources built");
        let n = res.tmds.len();
        // The walk-view pool must be the kingdom landmark pack and nothing
        // else: at least the full pack, and not the sibling-range continent
        // overview pack (+43/+70 TMDs) or stray battle meshes. A small margin
        // tolerates future shared-block (player) contributions.
        assert!(
            (landmarks..landmarks + 12).contains(&n),
            "{scene}: expected ~{landmarks} landmark TMDs from the kingdom \
             slot-1 pack, got {n} (too few = battle-map fallback; too many = \
             the world-overview continent pack leaked in)"
        );
        // Every loaded TMD must come from the single primary kingdom entry.
        let entries: std::collections::BTreeSet<u32> =
            res.tmds.iter().map(|t| t.entry_idx).collect();
        assert_eq!(
            entries.len(),
            1,
            "{scene}: world-map geometry must come from one kingdom entry, \
             got {entries:?}"
        );
        // At least most of the pack must texture cleanly against the slot-0
        // atlas (the prim filter drops un-textured prims; a healthy world map
        // keeps the large majority of meshes). Guards against a regression
        // where the atlas stops covering the landmark texture pages.
        let nonempty = res
            .tmds
            .iter()
            .filter(|t| !t.build_filtered_vram_mesh(&res.vram).indices.is_empty())
            .count();
        assert!(
            nonempty * 2 >= n,
            "{scene}: only {nonempty}/{n} landmark meshes textured - the \
             slot-0 atlas isn't covering the pack"
        );
        eprintln!(
            "[{scene}] world-map mesh pool: {n} TMDs from entry {entries:?}, \
             {nonempty} textured"
        );
    }
}

#[test]
fn world_map_live_walk_reaches_battle() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "map03".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let opts = FieldLiveOpts::default();
    session
        .enter_world_map_live("map03", &opts)
        .expect("enter_world_map_live");

    let world = &mut session.host.world;
    // A held screen-Right is remapped through the overworld camera azimuth
    // before it becomes a world direction (`world_map_camera_relative_bits`;
    // azimuth 0 maps screen-right to world Z-), so resolve the actual world
    // axis the walk will travel before picking a start tile.
    let azimuth = world
        .world_map
        .ctrl
        .as_ref()
        .map(|c| c.azimuth)
        .unwrap_or(0);
    let dir_bits = legaia_engine_core::world::world_map_camera_relative_bits(azimuth, 1, 0);
    let dxs = (dir_bits & 0x2000 != 0) as i32 - (dir_bits & 0x8000 != 0) as i32;
    let dzs = (dir_bits & 0x1000 != 0) as i32 - (dir_bits & 0x4000 != 0) as i32;
    assert!(
        dxs != 0 || dzs != 0,
        "camera remap must yield a world direction for a held Right"
    );

    // Find a start tile inside a rollable region with a clear run along the
    // walk direction. The walk must CROSS tiles to roll, and the real
    // walkability grid has walls (a region's geometric centre can sit on or
    // against one), so scan each rollable region's tile rect for consecutive
    // walkable tiles rather than assuming the centre is clear. Walls live in
    // 64-unit SUB-cells (4 per 128-unit tile): probe at 32-unit stride - a
    // tile-stride scan skips alternate sub-cells and picks runs the 2-unit
    // stepper blocks on immediately.
    let rects: Vec<(u8, u8, u8, u8)> = world
        .world_map
        .region_tracker
        .as_ref()
        .map(|t| {
            t.table()
                .regions
                .iter()
                .filter(|r| r.formation_count > 0 && r.rate_increment > 0)
                .map(|r| (r.tile_x_min, r.tile_x_max, r.tile_z_min, r.tile_z_max))
                .collect()
        })
        .unwrap_or_default();
    const RUN_TILES: i32 = 6;
    let mut pos = None;
    'scan: for (x_min, x_max, z_min, z_max) in rects {
        for zt in z_min as i32..=z_max as i32 {
            for xt in x_min as i32..=x_max as i32 {
                let x0 = xt * 128 + 64;
                let z0 = zt * 128 + 64;
                let end_x = x0 + dxs * RUN_TILES * 128;
                let end_z = z0 + dzs * RUN_TILES * 128;
                let in_rect = (x_min as i32 * 128..=(x_max as i32 + 1) * 128).contains(&end_x)
                    && (z_min as i32 * 128..=(z_max as i32 + 1) * 128).contains(&end_z);
                if !in_rect {
                    continue;
                }
                if (0..=RUN_TILES * 4).all(|k| {
                    !world
                        .field_tile_is_wall((x0 + dxs * k * 32) as i16, (z0 + dzs * k * 32) as i16)
                }) {
                    pos = Some((x0 as i16, z0 as i16));
                    break 'scan;
                }
            }
        }
    }
    let Some((px, pz)) = pos else {
        eprintln!("[skip] map03 has no rollable region with a walkable run");
        return;
    };
    if let Some(slot) = world.player_actor_slot {
        let a = &mut world.actors[slot as usize];
        a.move_state.world_x = px;
        a.move_state.world_z = pz;
    }
    // Re-seed the step latch so the move above isn't counted as a crossing.
    world.world_map.last_tile = None;

    // Hold Right; each 128-unit tile crossing rolls the region. A real
    // region's rate is modest, so allow a generous budget.
    world.set_pad(PadButton::Right.mask());
    let mut reached = false;
    let mut last_pos = (px, pz);
    for _ in 0..20_000 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            reached = true;
            break;
        }
        let slot = world.player_actor_slot.unwrap() as usize;
        // Restart the run when the player leaves every region OR hits a wall
        // (position unchanged = blocked). Each pass over the run re-crosses
        // its tiles, accumulating rolls.
        let cur = {
            let ms = &world.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        let out_of_region = world
            .world_map
            .region_tracker
            .as_ref()
            .and_then(|t| t.table().region_at_world(cur.0, cur.1))
            .is_none();
        if out_of_region || cur == last_pos {
            world.actors[slot].move_state.world_x = px;
            world.actors[slot].move_state.world_z = pz;
            world.world_map.last_tile = None;
            last_pos = (px, pz);
        } else {
            last_pos = cur;
        }
    }
    assert!(
        reached,
        "walking the overworld in-region reaches a battle within the budget"
    );
    assert_eq!(world.battle.return_mode, SceneMode::WorldMap);
}

/// Every kingdom overworld scene loads a **non-empty** walkability grid into
/// the live world after `enter_world_map_live`.
///
/// The overworld walk overlay's locomotion is the same `FUN_801d01b0` +
/// `FUN_801cfe4c` as the field, colliding against the same
/// `_DAT_1f8003ec + 0x4000` grid - and the three kingdom maps carry thousands
/// of wall sub-cells in that grid. This guards the regression where a redundant
/// `install_field_player` after scene load reset the grid to zeros and left the
/// overworld player roaming unbounded.
#[test]
fn overworld_scenes_load_nonempty_walkability_grid() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    for scene in ["map01", "map02", "map03"] {
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
        let opts = FieldLiveOpts::default();
        session
            .enter_world_map_live(scene, &opts)
            .expect("enter_world_map_live");
        let walls: usize = session
            .host
            .world
            .terrain
            .collision_grid
            .iter()
            .map(|b| (b >> 4).count_ones() as usize)
            .sum();
        eprintln!("[{scene}] live walkability grid: {walls} wall sub-cells");
        assert!(
            walls > 0,
            "{scene}: overworld must keep its walkability grid (got {walls} walls - \
             a re-install likely zeroed it)"
        );
    }
}

/// A field-VM `scene_transition(map_id)` that resolves to an overworld scene is
/// auto-routed into world-map mode with its region table seeded - the
/// boot/transition path, not the explicit `--world-map` entry. This is the
/// regression guard for "the overworld seeds itself when the game walks onto
/// it", driving the real `SceneHost::tick` transition handler.
#[test]
fn world_map_scene_transition_auto_enters_world_map() {
    use legaia_engine_mode::mode::SceneMode;
    use legaia_engine_scene::scene::VecMapIdResolver;
    use legaia_engine_scene_host::scene_host::SceneTickEvent;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // Boot on a town, then transition to the overworld the way the field VM
    // would: map_id 0 -> "map03" via the resolver, latched as a pending
    // scene transition that `SceneHost::tick` processes.
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    session
        .host
        .set_map_resolver(Box::new(VecMapIdResolver::new(vec!["map03".into()])));
    session.host.world.pending_scene_transition = Some(0);

    let event = session.host.tick().expect("tick processes the transition");
    assert!(
        matches!(&event, SceneTickEvent::SceneEntered { name } if name == "map03"),
        "transition entered map03, got {event:?}"
    );

    let world = &session.host.world;
    assert_eq!(
        world.mode,
        SceneMode::WorldMap,
        "an overworld transition lands in world-map mode, not plain Field"
    );
    assert!(
        world.world_map.region_tracker.is_some(),
        "the overworld region table is seeded on the transition path"
    );
    assert!(
        world.world_map.ctrl.is_some(),
        "the world-map camera controller is installed"
    );
    let walls: usize = world
        .terrain
        .collision_grid
        .iter()
        .map(|b| (b >> 4).count_ones() as usize)
        .sum();
    assert!(
        walls > 0,
        "the overworld walkability grid is loaded ({walls} walls)"
    );
}

/// The fishing signboard's record, decoded off the disc: its placement slot,
/// world anchor, and the record-relative PCs the tests key on.
struct Signboard {
    /// Index into `world_map.entity_configs`.
    entity: usize,
    /// Partition-1 placement index (the interaction record's key).
    slot: u8,
    sub_id: u8,
    pos: (i16, i16),
    /// The `0x2A` picker.
    picker_pc: usize,
    /// The first `4C DC` holdings test (the rod check).
    first_holding_pc: usize,
    /// The `SysFlag.Test` ahead of the holdings tests, if the record has one
    /// (`map03`'s frozen-lake gate).
    gate_flag: Option<u16>,
}

/// Open `scene` straight off the disc image (no `extracted/` needed) and
/// enter it as the overworld, armed the way both play hosts arm it.
fn open_world_map_disc(scene: &str) -> Option<NativeSession> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open_disc(&disc, &cfg).expect("open boot session");
    session
        .enter_world_map_live(scene, &FieldLiveOpts::default())
        .expect("enter_world_map_live");
    session.host.world.toggles.use_vm_dialogue = true;
    // No region roll may interrupt the talk.
    session.host.world.world_map.region_tracker = None;
    // Let the entry frames settle (the arrival seat and the entry fade
    // reposition and turn the player) before a test seats it.
    session.host.world.set_pad(0);
    for _ in 0..8 {
        session.host.tick().expect("settle tick");
    }
    Some(session)
}

fn signboard(session: &NativeSession) -> Signboard {
    use legaia_engine_field::world_map_entity_config::WorldMapEntityConfig;
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker};

    let world = &session.host.world;
    let (entity, sub_id, slot) = world
        .world_map
        .entity_configs
        .iter()
        .enumerate()
        .find_map(|(i, c)| match c {
            WorldMapEntityConfig::MinigameDoor { sub_id, slot } => Some((i, *sub_id, *slot)),
            _ => None,
        })
        .expect("the overworld installs its fishing signboard");
    let record = world
        .npcs
        .dialog_prologue
        .get(&slot)
        .expect("the signboard installs its interaction record");
    let mut picker_pc = None;
    let mut first_holding_pc = None;
    let mut gate_flag = None;
    for insn in LinearWalker::new(&record.body, record.entry_pc).map_while(Result::ok) {
        let b = &record.body[insn.pc..];
        if b.first() == Some(&0x2A) && picker_pc.is_none() {
            picker_pc = Some(insn.pc);
        }
        if b.starts_with(&[0x4C, 0xDC]) && first_holding_pc.is_none() {
            first_holding_pc = Some(insn.pc);
        }
        if first_holding_pc.is_none()
            && gate_flag.is_none()
            && let InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                ..
            } = insn.info
        {
            gate_flag = Some(idx);
        }
    }
    Signboard {
        entity,
        slot,
        sub_id,
        pos: world.world_map.entity_positions[entity],
        picker_pc: picker_pc.expect("the signboard record carries a picker"),
        first_holding_pc: first_holding_pc.expect("the signboard record tests holdings"),
        gate_flag,
    }
}

/// Seat the player beside the signboard, facing it, so the retail action
/// probe (64 units ahead, a ±72 box) finds it. `false` when none of the four
/// cardinal approaches puts the board in the probe.
fn seat_facing(session: &mut NativeSession, board: &Signboard) -> bool {
    seat_facing_slot(session, board.slot, board.pos)
}

/// [`seat_facing`] for any placement `slot` standing at `pos`.
fn seat_facing_slot(session: &mut NativeSession, slot: u8, pos: (i16, i16)) -> bool {
    let world = &mut session.host.world;
    let Some(p) = world.player_actor_slot else {
        return false;
    };
    for (dx, dz) in [(0i16, -96i16), (0, 96), (-96, 0), (96, 0)] {
        for heading in [0i16, 0x400, 0x800, 0xC00] {
            let a = &mut world.actors[p as usize];
            a.move_state.world_x = pos.0 + dx;
            a.move_state.world_z = pos.1 + dz;
            a.move_state.render_26 = heading;
            if world.field_interact_probe_slot() == Some(slot) {
                world.world_map.last_tile = None;
                return true;
            }
        }
    }
    false
}

/// Run the talk a confirm press opens: Cross on a 6-frame cadence (one edge
/// per press, so "Yes" - the picker's first row - is the answer) through
/// `SceneHost::tick`, whose minigame drain is the one both play hosts run.
/// Returns the furthest record PC the runner reached, whether a talk ever
/// ran, and the mode the run ended in.
fn run_talk(session: &mut NativeSession, frames: usize) -> (usize, bool, SceneMode) {
    let mut max_pc = 0usize;
    let mut ran = false;
    for f in 0..frames {
        let pad = if f % 6 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        session.host.world.set_pad(pad);
        session.host.tick().expect("scene host tick");
        if let Some(r) = session.host.world.dialog.inline.as_ref() {
            ran = true;
            max_pc = max_pc.max(r.pc);
        }
        if session.host.world.mode != SceneMode::WorldMap {
            break;
        }
        // The talk ended: stop before the next press opens it again.
        if ran && !session.host.world.dialogue_owns_input() && f % 6 == 5 {
            break;
        }
    }
    (max_pc, ran, session.host.world.mode)
}

/// The overworld **fishing signboard** is talk-to, not walk-onto. Standing on
/// its tile does nothing; the confirm press runs the placement's own
/// interaction record through the field's talk runner - holdings checks, the
/// "fish?" picker, the white fade and the wait - and that record's `0x3E` puts
/// the party in the pond.
///
/// This used to assert the opposite, that walking onto the board armed the
/// warp. That was the port's shortcut: the `MinigameDoor` entity warped on
/// the walk-over tile and the record never ran.
#[test]
fn world_map_fishing_signboard_runs_its_record_into_fishing() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    for scene in ["map02", "map03"] {
        let mut session = open_world_map_disc(scene).expect("disc session");
        let board = signboard(&session);
        assert_eq!(
            board.sub_id, 0,
            "[{scene}] the signboard is the fishing door"
        );

        // A party that can fish: a rod, a lure and (map03) a thawed lake.
        let world = &mut session.host.world;
        world.party.inventory.add(0xA0, 1);
        world.party.inventory.add(0x9D, 1);
        if let Some(flag) = board.gate_flag {
            world.system_flag_set(flag);
        }

        // Standing on the board's tile is not an interaction.
        if let Some(p) = world.player_actor_slot {
            let a = &mut world.actors[p as usize];
            a.move_state.world_x = board.pos.0;
            a.move_state.world_z = board.pos.1;
        }
        world.world_map.last_tile = None;
        world.set_pad(0);
        for _ in 0..8 {
            session.host.tick().expect("tick");
        }
        assert_eq!(
            session.host.world.minigames.pending_warp, None,
            "[{scene}] walking onto the signboard must not warp"
        );
        assert_eq!(session.host.world.mode, SceneMode::WorldMap);
        assert!(session.host.world.dialog.inline.is_none());

        assert!(
            seat_facing(&mut session, &board),
            "[{scene}] no approach puts the signboard in the action probe"
        );
        session.host.world.set_pad(0);
        session.host.tick().expect("tick");
        let (max_pc, ran, mode) = run_talk(&mut session, 900);
        eprintln!(
            "[{scene}] signboard P1[{}] entity #{}: ran={ran} max_pc=0x{max_pc:X} \
             picker=0x{:X} mode={mode:?}",
            board.slot, board.entity, board.picker_pc
        );
        assert!(
            ran,
            "[{scene}] the confirm press runs the signboard's record"
        );
        assert!(
            max_pc >= board.picker_pc,
            "[{scene}] the talk reaches the picker (max pc 0x{max_pc:X})"
        );
        assert_eq!(
            mode,
            SceneMode::Fishing,
            "[{scene}] answering yes lands in the fishing minigame"
        );
    }
}

/// `map03`'s board refuses with the lake frozen: flag clear, the record takes
/// its first text branch and the talk ends before it ever tests the party's
/// tackle. With the lake thawed but no rod it reaches the holdings tests and
/// still refuses ahead of the picker. Neither opens the minigame.
#[test]
fn world_map_fishing_signboard_refuses_frozen_lake_and_no_rod() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    // Frozen lake, full tackle.
    let mut session = open_world_map_disc("map03").expect("disc session");
    let board = signboard(&session);
    let flag = board
        .gate_flag
        .expect("map03's signboard gates on the frozen-lake flag");
    session.host.world.party.inventory.add(0xA0, 1);
    session.host.world.party.inventory.add(0x9D, 1);
    session.host.world.system_flag_clear(flag);
    assert!(seat_facing(&mut session, &board), "seat at the signboard");
    session.host.world.set_pad(0);
    session.host.tick().expect("tick");
    let (max_pc, ran, mode) = run_talk(&mut session, 600);
    eprintln!("[frozen] ran={ran} max_pc=0x{max_pc:X} mode={mode:?}");
    assert!(ran, "the confirm press runs the record");
    assert!(
        max_pc < board.first_holding_pc,
        "a frozen lake refuses before the tackle tests (max pc 0x{max_pc:X})"
    );
    assert_eq!(mode, SceneMode::WorldMap, "a frozen lake opens no minigame");
    assert_eq!(session.host.world.minigames.pending_warp, None);

    // Thawed lake, no rod.
    let mut session = open_world_map_disc("map03").expect("disc session");
    let board = signboard(&session);
    session.host.world.system_flag_set(flag);
    assert!(seat_facing(&mut session, &board), "seat at the signboard");
    session.host.world.set_pad(0);
    session.host.tick().expect("tick");
    let (max_pc, ran, mode) = run_talk(&mut session, 600);
    eprintln!("[no rod] ran={ran} max_pc=0x{max_pc:X} mode={mode:?}");
    assert!(ran, "the confirm press runs the record");
    assert!(
        max_pc >= board.first_holding_pc && max_pc < board.picker_pc,
        "no rod refuses after the holdings tests, ahead of the picker (max pc 0x{max_pc:X})"
    );
    assert_eq!(mode, SceneMode::WorldMap, "no rod opens no minigame");
}

/// A real overworld NPC carrying inline dialog text opens AND renders its
/// message when talked to: teleport the player onto a classified `Npc` entity,
/// press confirm, and assert (a) the world's `current_dialog` opens carrying
/// the inline text and (b) `SceneHost::open_pending_dialog` builds a panel that
/// types real glyphs out of it. The inline path is what makes placement-NPC
/// dialogue render at all - its `text_id` is a box-config id that never
/// resolves through the scene MES. Portals are walk-onto; NPCs are talk-to.
#[test]
fn world_map_talking_to_real_npc_renders_inline_dialogue() {
    use legaia_engine_field::world_map_entity_config::WorldMapEntityConfig;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // map01 / map02 each classify at least one NPC that carries inline dialog
    // text in its placement script.
    let mut tested_any = false;
    for scene in ["map01", "map02"] {
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
        let opts = FieldLiveOpts::default();
        session
            .enter_world_map_live(scene, &opts)
            .expect("enter_world_map_live");

        let npc = session
            .host
            .world
            .world_map
            .entity_configs
            .iter()
            .enumerate()
            .find_map(|(i, c)| match c {
                WorldMapEntityConfig::Npc { inline, .. } if !inline.is_empty() => Some(i),
                _ => None,
            });
        let Some(idx) = npc else {
            eprintln!("[{scene}] no talkable NPC entity with inline text installed");
            continue;
        };
        let (nx, nz) = session.host.world.world_map.entity_positions[idx];
        if let Some(slot) = session.host.world.player_actor_slot {
            let a = &mut session.host.world.actors[slot as usize];
            a.move_state.world_x = nx;
            a.move_state.world_z = nz;
        }
        // Don't count the teleport as a tile crossing (avoids a stray region
        // roll flipping us into Battle before the confirm).
        session.host.world.world_map.last_tile = None;

        // Settle a no-input frame so the confirm press is a clean edge, then talk.
        session.host.world.set_pad(0);
        let _ = session.host.world.tick();
        session.host.world.set_pad(PadButton::Cross.mask());
        let _ = session.host.world.tick();

        assert!(
            session
                .host
                .world
                .dialog
                .current
                .as_ref()
                .is_some_and(|d| !d.inline.is_empty()),
            "[{scene}] talking to the NPC opens a dialog carrying inline text"
        );

        // The host resolves it into a renderable panel; type ~64 glyphs and
        // confirm real text comes out (placement-NPC text only renders via the
        // inline path - the MES `text_id` lookup never resolves it).
        let mut panel = session
            .host
            .open_pending_dialog()
            .expect("[scene] inline dialog resolves to a panel");
        panel.set_glyphs_per_frame(1);
        for _ in 0..64 {
            panel.tick();
        }
        let glyphs = panel.page_bytes();
        let printable = glyphs
            .iter()
            .filter(|&&b| (0x20..=0x7E).contains(&b))
            .count();
        eprintln!(
            "[{scene}] NPC #{idx}: panel typed {} glyphs, {printable} printable",
            glyphs.len()
        );
        assert!(
            printable >= 3,
            "[{scene}] the inline dialog renders real text (got {printable} printable glyphs)"
        );
        tested_any = true;
    }
    assert!(
        tested_any,
        "at least one overworld NPC with inline dialog text must be exercised"
    );
}

/// Disc-gated: each kingdom overworld decodes its continent terrain as
/// world-frame static-object placements (the DATA_FIELD `.MAP` object grid,
/// `Scene::field_object_placements`). This is the data feeding the world-map
/// render's per-tile terrain draw (`resolve_field_placement_draws`): every
/// placement must sit on the 128-unit world tile grid (not pack-local), resolve
/// to a slot-1 pack mesh index, and land inside the world bounds - the same
/// frame the player marker uses, which is what aligns the continent under the
/// player instead of piling it at the pack origin.
#[test]
fn world_maps_decode_world_frame_terrain_placements() {
    use std::sync::Arc;

    use legaia_engine_scene::scene::{ProtIndex, Scene};

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));

    for label in ["map01", "map02", "map03"] {
        let scene = Scene::load(&index, label).expect("load world-map scene");
        let placements = scene
            .field_object_placements(&index)
            .expect("read field map")
            .expect("world-map scene has a DATA_FIELD object grid");
        assert!(
            !placements.is_empty(),
            "[{label}] continent terrain must decode at least one placement"
        );
        let mut resolved = 0usize;
        for p in &placements {
            // World-frame coordinates: the 128x128 tile grid spans 0..0x4000
            // world units per axis (col/row 0..127, tile size 0x80).
            assert!(
                (0..0x4000).contains(&p.world_x) && (0..0x4000).contains(&p.world_z),
                "[{label}] placement obj {} is off the world tile grid: ({}, {})",
                p.obj_idx,
                p.world_x,
                p.world_z
            );
            assert!(
                p.col < 0x80 && p.row < 0x80,
                "[{label}] tile index out of grid"
            );
            if p.pack_index.is_some() {
                resolved += 1;
            }
        }
        // The bulk of placements bind a slot-1 kingdom-pack mesh (a few are
        // script-only / global-pool refs that don't carry a pack index).
        assert!(
            resolved * 2 >= placements.len(),
            "[{label}] most placements ({resolved}/{}) must resolve to a kingdom-pack mesh",
            placements.len()
        );
        eprintln!(
            "[{label}] {} terrain placements, {resolved} resolve to a slot-1 pack mesh",
            placements.len()
        );

        // The DENSE continent layer: the visible-tile set (CELL_VISIBLE) is
        // the ground / trees / mountains the overhead sweep draws, far more
        // numerous than the placed-flag interactive objects above. This is the
        // set the world-map render now draws so the overworld isn't sparse.
        let tiles = scene
            .field_terrain_tiles(&index)
            .expect("read field map")
            .expect("world-map scene has a terrain tile grid");
        assert!(
            tiles.len() > placements.len() * 2,
            "[{label}] the visible terrain layer ({}) must dwarf the placed objects ({})",
            tiles.len(),
            placements.len()
        );
        let tile_in_pack = tiles
            .iter()
            .filter(|p| p.pack_index.is_some())
            .filter(|p| (0..0x4000).contains(&p.world_x) && (0..0x4000).contains(&p.world_z))
            .count();
        // Most visible tiles draw from the loaded slot-1 pack (a minority index
        // the wider global TMD pool, which the world-map load doesn't pull in).
        assert!(
            tile_in_pack * 3 >= tiles.len(),
            "[{label}] most terrain tiles ({tile_in_pack}/{}) resolve in-pack + on-grid",
            tiles.len()
        );
        eprintln!(
            "[{label}] {} dense terrain tiles ({tile_in_pack} drawable)",
            tiles.len()
        );
    }
}

/// A save written on a kingdom overworld resumes on the overworld. The native
/// Load path (`enter_field_live_from_save`) used to enter every label through
/// the field entry, so an `mapNN` save came back as a plain field scene with
/// no region table and no overworld controller - while the browser page's
/// card Load routed the same label through the world-map entry.
#[test]
fn save_load_on_an_overworld_label_resumes_in_world_map_mode() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = session.host.world.save_full();
    let opts = FieldLiveOpts::default();
    let mode = session
        .enter_field_live_from_save("map03", &opts, save)
        .expect("enter_field_live_from_save");
    assert_eq!(
        mode,
        SceneMode::WorldMap,
        "an overworld save resumes on the overworld"
    );
    assert!(
        session.host.world.world_map.region_tracker.is_some(),
        "the overworld's region table is routed"
    );
    // A town label still takes the field entry.
    let save = session.host.world.save_full();
    let mode = session
        .enter_field_live_from_save("town01", &opts, save)
        .expect("enter_field_live_from_save town01");
    assert_ne!(mode, SceneMode::WorldMap);
    eprintln!("[ok] map03 save resumes in WorldMap; town01 in {mode:?}");
}

/// Every overworld talk placement standing on the map at entry talks through
/// its own record on the action button, at the seat the record put it on.
/// `map02` `P1[6]` (a direction sign) is the case: its spawn section seats it
/// with `23 xb zb`, while its static spawn is the off-map hide box - which is
/// where the simplified overworld talk looked for it, so the sign could not
/// be read at all.
#[test]
fn world_map_seated_talk_placements_run_their_records() {
    /// The off-map hide box a parked placement spawns in (tile `0x7F`).
    const PARKED: (i16, i16) = (0x7F * 128 + 0x40, 0x7F * 128 + 0x40);
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let mut talked = 0usize;
    for scene in ["map01", "map02", "map03"] {
        let probe = open_world_map_disc(scene).expect("disc session");
        let npcs = &probe.host.world.npcs;
        let mut seated: Vec<(u8, (i16, i16))> = npcs
            .dialog
            .keys()
            .filter(|s| npcs.dialog_prologue.contains_key(s))
            .filter_map(|s| npcs.positions.get(s).map(|p| (*s, *p)))
            .filter(|(_, p)| *p != PARKED)
            .collect();
        seated.sort_unstable();
        drop(probe);
        for (slot, pos) in seated {
            let mut session = open_world_map_disc(scene).expect("disc session");
            assert!(
                seat_facing_slot(&mut session, slot, pos),
                "[{scene}] P1[{slot}] at {pos:?}: no approach puts it in the action probe"
            );
            session.host.world.set_pad(0);
            session.host.tick().expect("tick");
            let mut boxed = false;
            let mut ended = false;
            for f in 0..900 {
                let pad = if f % 6 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                };
                session.host.world.set_pad(pad);
                session.host.tick().expect("tick");
                let w = &session.host.world;
                if w.dialog.inline.as_ref().is_some_and(|r| r.panel.is_some()) {
                    boxed = true;
                }
                if boxed && !w.dialogue_owns_input() {
                    ended = true;
                    break;
                }
            }
            eprintln!("[{scene}] P1[{slot}] at {pos:?}: boxed={boxed} ended={ended}");
            assert!(boxed, "[{scene}] P1[{slot}]: the record opens its box");
            assert!(ended, "[{scene}] P1[{slot}]: the conversation ends");
            assert_eq!(session.host.world.mode, SceneMode::WorldMap);
            talked += 1;
        }
    }
    assert!(
        talked > 0,
        "no seated overworld talk placement was exercised"
    );
}
