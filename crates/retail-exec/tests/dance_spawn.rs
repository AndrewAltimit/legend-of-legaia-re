//! The dance floor's spawner `FUN_801D0190` in PROT 0980, whole, mode by mode
//! against `DanceGame::from_overlay_for_mode`: how many actors it asks the
//! shared spawner for, the mesh each is asked on (with or without the scene's
//! pack bias), and every store it makes into the actor it is handed -
//! position, floor slot, kind descriptor, idle clip and rate, the human's
//! scale and party-bank flag, and the how-to mode's Disco King.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::dance::{DanceBodyModel, DanceGame, DanceMode, UNIT_RENDER_SCALE};
use legaia_engine_minigames::minigame_actor::FLAG_PARTY_CLIP_BANK;
use legaia_minigame_formats::dance_cast as dc;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SPAWN: u32 = 0x801D_0190;
const ACTOR_SPAWN: u32 = 0x8002_0DE0;
const PRINT: u32 = 0x8005_67A8;
const MODE: u32 = 0x801D_514C;
const KIND: u32 = 0x801D_540C;
const PACK_BIAS: u32 = 0x8007_B6F8;
const SPAWN_ARG: u32 = 0x8007_C34C;
const ACTORS: u32 = 0x8014_0000;
const ACTOR_LEN: u32 = 0x100;

#[test]
fn every_mode_spawns_the_floor_retail_spawns() {
    differential!(prot(980, SPAWN));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(980).expect("PROT 0980");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    cpu.stub(PRINT, 0);
    // Each call of the shared spawner: the mesh its record names and its
    // second argument. It hands back the next zeroed actor.
    let asked = Rc::new(RefCell::new(Vec::<(u16, u32)>::new()));
    let a = asked.clone();
    cpu.hook(ACTOR_SPAWN, move |r, m| {
        let mut a = a.borrow_mut();
        r.r[V0] = ACTORS + a.len() as u32 * ACTOR_LEN;
        a.push((m.rd16(r.r[A0] + 4), r.r[A1]));
    });
    let mut rng = Rng(0x5350_574E);
    for round in 0..40 {
        let mode = [
            DanceMode::Qualifier,
            DanceMode::Finals,
            DanceMode::HowTo,
            DanceMode::FreePlay,
        ][round % 4];
        let bias = rng.below(0x400) as u16;
        let arg = rng.next();
        let flags = rng.next() & !FLAG_PARTY_CLIP_BANK;
        cpu.mem.wr32(MODE, mode as u32);
        cpu.mem.wr16(PACK_BIAS, bias);
        cpu.mem.wr32(SPAWN_ARG, arg);
        cpu.mem.load(ACTORS, &vec![0; 8 * ACTOR_LEN as usize]);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + i * ACTOR_LEN + 0x10, flags);
            cpu.mem.wr32(KIND + i * 4, 0xDEAD);
        }
        asked.borrow_mut().clear();
        assert_eq!(cpu.call(SPAWN, &[]), Stop::Returned);

        let g = DanceGame::from_overlay_for_mode(&overlay, mode).expect("run");
        let bodies = g.body_frames();
        let asked = asked.borrow();
        assert_eq!(asked.len(), bodies.len(), "{mode:?}: the floor's size");
        assert_eq!(
            bodies.len(),
            mode.cast_size() + usize::from(mode == DanceMode::HowTo)
        );
        for (i, b) in bodies.iter().enumerate() {
            let at = ACTORS + i as u32 * ACTOR_LEN;
            let ctx = format!("{mode:?} body {i}");
            let (mesh, human) = match b.model {
                DanceBodyModel::Resident(m) => (m, true),
                DanceBodyModel::Scene(m) => (bias.wrapping_add(m), false),
            };
            assert_eq!(asked[i], (mesh, arg), "{ctx}: the spawner's call");
            let pos = [0x14, 0x16, 0x18].map(|o| cpu.mem.rd16(at + o) as i16);
            assert_eq!(pos, b.pos, "{ctx}: the position");
            assert_eq!(
                (cpu.mem.rd16(at + 0x5C), cpu.mem.rd16(at + 0x6A)),
                (b.loop_clip.id, b.loop_clip.rate),
                "{ctx}: the clip"
            );
            // The human's scale and bank flag; every other body keeps what
            // the shared spawner gave it.
            let scale = cpu.mem.rd16(at + 0x72);
            assert_eq!(
                if scale == 0 { UNIT_RENDER_SCALE } else { scale },
                b.scale,
                "{ctx}: the scale"
            );
            assert_eq!(
                cpu.mem.rd32(at + 0x10),
                flags | if human { FLAG_PARTY_CLIP_BANK } else { 0 },
                "{ctx}: the flags"
            );
            let Some(kind) = b.kind else {
                // The Disco King: nothing else is stored.
                assert_eq!(cpu.mem.rd32(at + 0x48), 0, "{ctx}");
                continue;
            };
            let actor = &g.dancer_actors()[i];
            assert_eq!(
                (
                    cpu.mem.rd16(at + 0x5A),
                    cpu.mem.rd16(at + 0x5C) as i16,
                    cpu.mem.rd16(at + 0x56),
                    cpu.mem.rd32(at + 0x48),
                    cpu.mem.rd32(KIND + i as u32 * 4),
                ),
                (
                    actor.live_mask,
                    actor.field_5c,
                    1,
                    dc::KIND_TABLE_VA + (kind * dc::KIND_STRIDE) as u32,
                    kind as u32,
                ),
                "{ctx}: the slot, clip, descriptor and kind"
            );
            assert_eq!(g.dancer_kind(i), kind);
        }
        // Nothing past the floor is spawned or named.
        let past = bodies.len() as u32;
        assert_eq!(cpu.mem.rd32(ACTORS + past * ACTOR_LEN + 0x10), flags);
        if mode.cast_size() < 8 {
            assert_eq!(cpu.mem.rd32(KIND + mode.cast_size() as u32 * 4), 0xDEAD);
        }
    }
}
