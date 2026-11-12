//! The touch post `FUN_801D5B5C` (PROT 0897) over generated actors, players
//! and scene words, with the motion-pause kick `FUN_8003C9AC` it ends on
//! hooked, against the port's two posts: a prop's
//! `World::start_prop_interaction` and an NPC's `World::trigger_field_interact`.
//! Both must leave what the post leaves for the frames after it: the
//! player's engaged bit and the eight-tick roll hold `_DAT_8007B604` the
//! system entity spends before it samples another step.
//!
//! The post's actor-side stores (`+0x10 |= 0x100`, the `+0x2A` and scene
//! `+0x0A` counts, the `+0x26 -> +0x5A` facing save) are checked against the
//! bytes; the port carries the facing save as `FieldNpcState::facing_save`,
//! which the talk's face-the-player arm fills.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_scene::field_env::{PropAnim, PropAnimState, PropProgram};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const POST: u32 = 0x801D_5B5C;
const KICK: u32 = 0x8003_C9AC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const PLAYER_PTR: u32 = 0x8007_C364;
const OVERWORLD: u32 = 0x8007_B6A8;
const CLIP_BASE: u32 = 0x8007_BDD8;
const ROLL_HOLD: u32 = 0x8007_B604;
const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8015_0000;
const PLAYER: u32 = 0x8013_0000;
const ACTOR: u32 = 0x8014_0000;

#[test]
fn the_touch_post_arms_what_retail_arms() {
    differential!(prot(897, POST));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let kicked = Rc::new(RefCell::new(Vec::<u32>::new()));
    let k = kicked.clone();
    cpu.hook(KICK, move |r, _| k.borrow_mut().push(r.r[A0]));
    let mut rng = Rng(0x544F_5543);
    for round in 0..2000 {
        let label = format!("round {round}");
        let actor: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        let player: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        let scene: Vec<u8> = (0..0x40).map(|_| rng.next() as u8).collect();
        cpu.mem.load(ACTOR, &actor);
        cpu.mem.load(PLAYER, &player);
        cpu.mem.load(SCENE, &scene);
        cpu.mem.wr32(PLAYER_PTR, PLAYER);
        cpu.mem.wr32(SCENE_PTR, SCENE);
        let overworld = rng.below(2) == 0;
        cpu.mem
            .wr8(OVERWORLD, u8::from(overworld) * (1 + rng.below(0xFF) as u8));
        let clip_base = rng.next();
        cpu.mem.wr32(CLIP_BASE, clip_base);
        cpu.mem.wr8(ROLL_HOLD, rng.next() as u8);
        kicked.borrow_mut().clear();
        assert_eq!(cpu.call(POST, &[ACTOR]), Stop::Returned, "{label}");

        let w32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let w16 = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let engaged = 0x0008_0000 | if overworld { 0x0100_0000 } else { 0 };
        assert_eq!(
            cpu.mem.rd32(PLAYER + 0x10),
            w32(&player, 0x10) | engaged,
            "{label}"
        );
        assert_eq!(
            cpu.mem.rd32(CLIP_BASE),
            if overworld { 2 } else { clip_base },
            "{label}"
        );
        assert_eq!(cpu.mem.rd8(ROLL_HOLD), 8, "{label}");
        assert_eq!(
            cpu.mem.rd32(ACTOR + 0x10),
            w32(&actor, 0x10) | 0x100,
            "{label}"
        );
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x2A),
            w16(&actor, 0x2A).wrapping_add(1)
        );
        assert_eq!(
            cpu.mem.rd16(SCENE + 0x0A),
            w16(&scene, 0x0A).wrapping_add(1)
        );
        assert_eq!(cpu.mem.rd16(ACTOR + 0x5A), w16(&actor, 0x26), "{label}");
        assert_eq!(*kicked.borrow(), vec![u32::from(w16(&actor, 0x26))]);

        // The port's prop post.
        let mut w = World::new();
        w.install_field_player(0);
        w.encounters.roll_hold = cpu.mem.rd8(ROLL_HOLD).wrapping_add(round as u8 | 1);
        let anchor = (16, 19);
        w.props.bank.props.insert(
            anchor,
            PropAnimState {
                anim: PropAnim::spawned(1, 4, false, 0),
                program: PropProgram::default(),
                world: (2240, 2496),
                collider: (2240, 2496),
                record: 1,
                record_body: std::sync::Arc::new(vec![0, 0, 0x21]),
                parked_pc: 2,
                cflags: 0x4000_0000,
            },
        );
        assert!(w.start_prop_interaction(anchor), "{label}");
        assert_ne!(w.actors[0].move_state.flags & 0x0008_0000, 0, "{label}");
        assert_eq!(w.encounters.roll_hold, 8, "{label}: the prop's post");

        // The port's NPC post.
        let mut w = World::new();
        w.install_field_player(0);
        w.trigger_field_interact(0, 3);
        assert_eq!(w.encounters.roll_hold, 8, "{label}: the NPC's post");
    }
}
