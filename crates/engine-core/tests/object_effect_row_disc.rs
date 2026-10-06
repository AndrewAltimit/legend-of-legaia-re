//! Disc-gated: a field beat that raises `actor[+0x42]` (field-VM `4C C2`)
//! also writes object-effect row 0 from the scene's prescript (move-VM ext
//! `0x1A`, the yaw seat), and the world hands the raised actors a clip.
//!
//! `rugi` P2[72] and `noaru` P2[34] are the two raising beats the engine
//! reaches from a bare scene entry (a sweep of every partition-2 record of
//! the nine raising scenes finds no other). The expected rows are what the
//! prescript's own `0x1A` operands give under the arm at `0x801D3CE0`:
//! `yaw = (op[3] + 0x400) & 0xFFF`, `+4 = 0x400`, and both clip words
//! offset by `(op[5] * sin[yaw] + op[4] * cos[yaw]) >> 12`. No Sony bytes
//! asserted beyond those derived halfwords.

use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

/// `(scene, partition-2 record, expected row 0)`.
const BEATS: [(&str, usize, [i16; 5]); 2] = [
    ("rugi", 72, [0, 3072, 1024, -14912, -10816]),
    ("noaru", 34, [0, 3072, 1024, -5696, -1600]),
];

#[test]
fn a_raising_beat_writes_row_zero_and_clips_its_actors() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    eprintln!("[ran] object-effect beats");
    for (name, record, want) in BEATS {
        host.enter_field_scene(name, 0).expect("enter");
        assert!(
            host.world.object_effect_clips().is_empty(),
            "{name}: nothing is raised on entry"
        );
        let man = host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
            .expect("MAN");
        let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
        let flat = mf.header.partition_counts[0].max(0) as usize
            + mf.header.partition_counts[1].max(0) as usize
            + record;
        host.world.field_vm.pending_record_spawns.push(flat as u8);
        let mut seen = None;
        for _ in 0..240 {
            let _ = host.tick();
            let clips = host.world.object_effect_clips();
            if !clips.is_empty() {
                seen = Some((host.world.object_effect.row(0), clips));
                break;
            }
        }
        let (row, clips) = seen.unwrap_or_else(|| panic!("{name}: P2[{record}] raised nothing"));
        eprintln!(
            "{name}: row0 {row:?}, clipped {:?}",
            clips.iter().map(|c| c.0).collect::<Vec<_>>()
        );
        // The beat's `CC F8 C2 01` (record `+0x64`) raises the player too.
        assert!(
            clips
                .iter()
                .any(|c| c.0 == legaia_engine_core::world::ActorTintKey::Player),
            "{name}: the player steps through the plane with the NPCs"
        );
        // The raised actors stand just short of the plane at the beat's
        // start (rugi z 10560 against 10816, noaru 1472 against 1600): they
        // step out through it, which is what the slab draws.
        for c in host
            .world
            .field_vm
            .channels
            .iter()
            .filter(|c| c.ctx.field_42 != 0)
        {
            let z = f32::from(c.ctx.world_z as i16);
            assert!(
                z < f32::from(want[4]).abs(),
                "{name}: actor at z {z} starts behind the plane"
            );
        }
        assert_eq!(row, Some(want), "{name}: the prescript's 0x1A row");
        for (_, clip, _) in &clips {
            // The yaw seat at 3072 turns the slab into a vertical plane on Z.
            assert!(
                clip.n[1].abs() < 1e-3 && (clip.n[2] + 1.0).abs() < 1e-3,
                "{clip:?}"
            );
            assert_eq!((clip.lo, clip.hi), (f32::from(want[3]), f32::from(want[4])));
        }
    }
}
