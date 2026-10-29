//! Disc-gated: `rayman`'s bridge collapse (Octam's underground city), through
//! the real `SceneHost`.
//!
//! The plank bridge is fourteen placements (`P1[70..=83]`, contexts
//! `0x53..=0x60`). Each prologue raises the actor tick's height-law bit
//! (`31 1D`, `+0x10 |= 0x20000000`) and parks `+0x8E = 320`
//! (`4C 42 40 01 00 00`), so the plank stands at `Y = -320`, level with the
//! ledges. The collapse beat `P2[7]` (global `13 + 92 + 7`) raises `0x1EB`
//! and drops the planks: `CC 53 42 00 FD 00 00` snaps context `0x53` to
//! `+0x8E = -768` at once, and each of `0x54..=0x57` takes a look turn
//! (`CC xx 45 00 B0 04 00 00 B0 04 ..`) and a `+0x8E` tween to `-768` over
//! 240 / 260 frames (`CC xx 42 00 FD <ticks>`).
//!
//! The tween is the field VM's nibble-4 sub-2 ramp arm (`0x801E1214..
//! 0x801E1230`, PROT 0897): `FUN_8003C5F0` schedules a kind-2 slot on the
//! resolved actor's `&+0x8E`, from its live value. The pool tick
//! (`FUN_80036D80`) writes `end + (start - end) * remaining / total` and the
//! height arm (`FUN_8003BC08`, `0x8003BC50..0x8003BC64`) mirrors
//! `+0x16 = -(+0x8E)` - so each plank falls from `-320` to `+768` on a straight
//! line while it turns. A port that drops the placement ramp leaves the planks
//! turning in place at the ledge.
//!
//! The planks are also what walls the row off once the bridge has gone.
//! Each plank's spawn section runs `31 00` (`+0x10 |= 1`), which keeps it
//! out of the collision candidate table `FUN_801CF754` builds (it skips any
//! actor with `+0x10 & 3` set), and the ledge row stays walkable in the base
//! grid either way. With the collapse flag up the same section runs `32 00`,
//! and the fallen plank - a moving-class actor, `0x20000` from
//! `FUN_8003A1E4` - blocks the row on its own tile.
//!
//! The aftermath (`rayman2`, the frozen revisit, and `rayman` re-entered)
//! re-runs those sections and the sinking set pieces' (`P0[5]`, `P0[6]`,
//! `P0[8]`, `P0[9]`): each raises its height law, parks `+0x8E = -768` and
//! blacks itself out with `CC <own id> 81 00 00 00 FF 0F 00 00`.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// The collapse beat, as an op-`0x44` global record index.
const COLLAPSE_BEAT: u8 = 13 + 92 + 7;

/// The plank the beat snaps (`0x53`) and the four it tweens (`0x54..=0x57`),
/// as placement slots (`context - N0`).
const SNAPPED: u8 = 70;
const FALLING: [u8; 4] = [71, 72, 73, 74];

/// The bridge row (`Z = 10*128 + 64`) and the plank columns of each bridge:
/// `P1[70..=76]` east (`0x1EB`), `P1[77..=83]` west (`0x1FC`).
const ROW_Z: i16 = 10 * 128 + 64;
const EAST_BRIDGE: std::ops::RangeInclusive<i16> = 37..=43;
const WEST_BRIDGE: std::ops::RangeInclusive<i16> = 19..=25;

/// How many of `cols`' tiles refuse an X+ step taken from one tile west of
/// the tile centre on the bridge row.
fn row_blocked(host: &SceneHost, cols: std::ops::RangeInclusive<i16>) -> usize {
    cols.filter(|&c| host.world.field_actor_dir_blocked(c * 128, ROW_Z, 3))
        .count()
}

/// The plank's drawn Y this frame (retail Y-down), where it stands.
fn plank_y(host: &SceneHost, slot: u8) -> i32 {
    let c = host
        .world
        .field_vm
        .channels
        .iter()
        .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
        .unwrap_or_else(|| panic!("plank P1[{slot}] has a channel"));
    host.world
        .field_npc_render_y(slot, c.ctx.world_x as i16, c.ctx.world_z as i16)
}

#[test]
fn the_bridge_planks_fall_while_they_turn_or_skip() {
    let Some(root) = extracted_root() else { return };
    // The beat is a walk-on: a gate-1 tile trigger on the `.MAP` names it.
    let index = legaia_engine_scene::scene::ProtIndex::open_extracted(&root).expect("index");
    let scene = legaia_engine_scene::scene::Scene::load(&index, "rayman").expect("rayman");
    let (primary, fallback) = scene.field_tile_triggers(&index).expect("triggers");
    let tiles: Vec<(u8, u8)> = primary
        .iter()
        .chain(&fallback)
        .filter(|t| t.gate == 1 && t.record == COLLAPSE_BEAT - 13 - 92)
        .map(|t| (t.tile_x, t.tile_z))
        .collect();
    eprintln!("[ran] P2[7] walk-on tiles: {tiles:?}");
    assert!(!tiles.is_empty(), "a walk-on tile spawns the collapse beat");
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.world.npcs.solid = true;
    host.enter_field_scene("rayman", 0).expect("enter rayman");
    for _ in 0..4 {
        host.tick().expect("tick");
    }
    assert_eq!(
        row_blocked(&host, EAST_BRIDGE),
        0,
        "the standing bridge is walkable"
    );
    for slot in std::iter::once(SNAPPED).chain(FALLING) {
        assert_eq!(
            plank_y(&host, slot),
            -320,
            "plank P1[{slot}] stands level with the ledges before the collapse"
        );
    }
    host.world
        .field_vm
        .pending_record_spawns
        .push(COLLAPSE_BEAT);
    let mut tracks: Vec<Vec<i32>> = vec![Vec::new(); FALLING.len()];
    let mut snapped_seen = false;
    for i in 0..3000 {
        // A dialogue beat waits for Cross: press 2, release 14.
        let pad = if i % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.input.set_pad(pad);
        host.tick().expect("tick");
        snapped_seen |= plank_y(&host, SNAPPED) == 768;
        for (t, &slot) in tracks.iter_mut().zip(&FALLING) {
            let y = plank_y(&host, slot);
            if t.last() != Some(&y) {
                t.push(y);
            }
        }
    }
    assert!(
        host.world.system_flag_test(0x1EB),
        "the collapse beat ran (it raises 0x1EB first)"
    );
    assert!(
        snapped_seen,
        "plank P1[{SNAPPED}] snapped to the chasm floor"
    );
    let blocked = row_blocked(&host, EAST_BRIDGE);
    eprintln!("[ran] east bridge row: {blocked} of 7 plank tiles block an X+ step");
    assert_eq!(blocked, 7, "the fallen planks wall the bridge row off");
    for (t, &slot) in tracks.iter().zip(&FALLING) {
        eprintln!(
            "[ran] plank P1[{slot}]: {} distinct Ys, {:?}",
            t.len(),
            &t[t.len().saturating_sub(4)..]
        );
        assert_eq!(t.first(), Some(&-320), "P1[{slot}] starts at the ledge");
        let landed = t
            .iter()
            .position(|&y| y == 768)
            .unwrap_or_else(|| panic!("P1[{slot}] lands at -(-768): {t:?}"));
        let fall = &t[..=landed];
        assert!(
            fall.windows(2).all(|w| w[1] > w[0]),
            "P1[{slot}] only ever descends on the way down: {fall:?}"
        );
        // A straight-line tween over 240 / 260 frames, not a snap: it
        // passes through many intermediate heights, none far apart.
        assert!(
            fall.len() > 30,
            "P1[{slot}] falls over the beat, not at once: {fall:?}"
        );
        assert!(
            fall.windows(2).all(|w| w[1] - w[0] <= 16),
            "P1[{slot}] falls in even steps: {fall:?}"
        );
        // After the beat the village re-seat (`P2[17]`) re-runs the plank's
        // spawn section, which with `0x1EB` up parks it at `+0x8E = -512`.
        assert!(
            t[landed + 1..].iter().all(|&y| y == 512),
            "P1[{slot}] settles only at the collapsed seat: {t:?}"
        );
    }
}

/// The aftermath: both collapse flags up, as on every later visit. Each
/// sinking set piece stands at `+768` and draws black, and both bridge rows
/// are walled off; the frozen revisit `rayman2` carries the same sections.
#[test]
fn the_collapsed_set_pieces_stay_sunk_dark_and_walled_or_skip() {
    let Some(root) = extracted_root() else { return };
    for scene in ["rayman", "rayman2"] {
        let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
        for flag in [0x1EB, 0x1FC, 0x200] {
            host.world.system_flag_set(flag);
        }
        host.world.npcs.solid = true;
        host.world.stage_picker_entry(scene, false);
        host.enter_field_scene(scene, 0).expect("enter scene");
        for _ in 0..10 {
            host.tick().expect("tick");
        }
        let moves = host.world.object_draw_displacements();
        let tints = host.world.object_draw_tints();
        for record in [5usize, 6, 8, 9] {
            assert_eq!(
                moves.get(&record),
                Some(&[0, 768, 0]),
                "{scene} P0[{record}] sinks to +0x8E = -768"
            );
            assert_eq!(
                tints.get(&record),
                Some(&(0, 0x0FFF)),
                "{scene} P0[{record}] blacks itself out"
            );
        }
        let (east, west) = (
            row_blocked(&host, EAST_BRIDGE),
            row_blocked(&host, WEST_BRIDGE),
        );
        eprintln!("[ran] {scene}: sunk + black, bridge rows block {east} / {west} of 7");
        assert_eq!((east, west), (7, 7), "{scene}: both bridge rows walled off");
    }
}
