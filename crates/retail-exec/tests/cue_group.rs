//! The applier's cue-group expander `FUN_801E22C8` against
//! `expand_cue_group` over the tables `EffectAuxTables` parses out of PROT
//! 0898: every group id, tints neutral and not, the revive's skip state and
//! others, every actor seat and heading. The three calls it makes - the 2D
//! cue `FUN_801DFDF0`, `MoveImage` `FUN_80058490` and the prototype spawn
//! `FUN_80050ED4` - are hooked and logged in order, and compared with the
//! port's spawns: the cue id and heading, the CLUT row the copy names, the
//! prototype word, and the tint the spawned node takes. The actor's `+0x04`
//! and `+0x0C` writes are compared too.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::move_power::MovePowerCatalog;
use legaia_engine_battle_vm::battle_cue_group::{
    CUE_TINT_BLEND, CUE_TINT_NEUTRAL, CueSpawn, CueTables, expand_cue_group,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const EXPAND: u32 = 0x801E_22C8;
const ACTOR_CUE: u32 = 0x801D_FDF0;
const MOVE_IMAGE: u32 = 0x8005_8490;
const SPAWN: u32 = 0x8005_0ED4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const PRIM_CURSOR: u32 = 0x1F80_0314 + 0x8C;
const PRIMS: u32 = 0x8011_0000;
const NODES: u32 = 0x8016_0000;

#[derive(Debug, PartialEq)]
enum Call {
    Actor {
        id: u32,
        yaw: i16,
    },
    Clut {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        dx: u32,
        dy: u32,
    },
    Spawn {
        proto: u32,
        scale: u32,
        node: u32,
    },
}

#[test]
fn a_cue_group_expands_as_retail_expands_it() {
    differential!(prot(898, EXPAND));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = disc.read_entry(898).expect("PROT 0898");
    // The holder the battle host reads both tables through.
    let catalog = MovePowerCatalog::from_overlay_0898(&image).expect("move power");
    let aux = catalog.aux_tables().expect("aux tables");
    cpu.mem.load(OVERLAY_BASE, &image);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let log = log.clone();
        cpu.hook(ACTOR_CUE, move |r, _| {
            log.borrow_mut().push(Call::Actor {
                id: r.r[A0],
                yaw: r.r[A2] as i16,
            })
        });
    }
    {
        let log = log.clone();
        cpu.hook(MOVE_IMAGE, move |r, m| {
            let rect = r.r[A0];
            log.borrow_mut().push(Call::Clut {
                x: m.rd16(rect),
                y: m.rd16(rect + 2),
                w: m.rd16(rect + 4),
                h: m.rd16(rect + 6),
                dx: r.r[A1],
                dy: r.r[A2],
            })
        });
    }
    {
        let log = log.clone();
        let next = Rc::new(RefCell::new(0u32));
        cpu.hook(SPAWN, move |r, m| {
            let mut k = next.borrow_mut();
            let node = NODES + *k * 0x100;
            *k = (*k + 1) % 16;
            m.wr32(node + 0x74, 0xEEEE_EEEE);
            m.wr16(node + 0x78, 0xEEEE);
            log.borrow_mut().push(Call::Spawn {
                proto: r.r[A2],
                scale: r.r[A3],
                node,
            });
            r.r[V0] = node;
        });
    }
    let groups = aux.cue_group_bytes();
    let n_groups = groups.len() / 5;
    let tables = CueTables {
        groups,
        clut_map: aux.clut_map(),
    };
    let mut rng = Rng(0x4355_4547);
    let (mut actor_cues, mut effects, mut copies, mut tinted) = (0, 0, 0, 0);
    for n in 0..20_000 {
        let group = rng.below(n_groups as u32) as u8;
        let seat = rng.below(8);
        let tint = match rng.below(3) {
            0 => CUE_TINT_NEUTRAL,
            _ => rng.next() & 0x00FF_FFFF,
        };
        let state = match rng.below(3) {
            0 => 0x2008_0200,
            _ => rng.next(),
        };
        let yaw = rng.next() as i16;
        let a = ACTOR + seat * 0x400;
        cpu.mem.wr16(a + 0x46, yaw as u16);
        cpu.mem.wr32(a + 0x0C, 0x1234_5678);
        cpu.mem.wr32(PRIM_CURSOR, PRIMS);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(EXPAND, &[tint, state, seat, u32::from(group)]),
            Stop::Returned,
            "n {n}"
        );
        let port = expand_cue_group(tint, state, yaw, group, &tables);
        let calls = std::mem::take(&mut *log.borrow_mut());
        let mut it = calls.iter();
        for spawn in &port.spawns {
            match *spawn {
                CueSpawn::Actor { id, yaw } => {
                    assert_eq!(
                        it.next(),
                        Some(&Call::Actor {
                            id: u32::from(id),
                            yaw
                        }),
                        "n {n} group {group}"
                    );
                    actor_cues += 1;
                }
                CueSpawn::Effect {
                    clut_x,
                    effect_index,
                    tint,
                    ..
                } => {
                    if let Some(x) = clut_x {
                        assert_eq!(
                            it.next(),
                            Some(&Call::Clut {
                                x: u16::from(x),
                                y: 0x1DC,
                                w: 0x10,
                                h: 1,
                                dx: 0xE0,
                                dy: 0x1DC
                            }),
                            "n {n} group {group}"
                        );
                        copies += 1;
                    }
                    let Some(Call::Spawn { proto, scale, node }) = it.next() else {
                        panic!("n {n} group {group}: no spawn");
                    };
                    assert_eq!(
                        (*proto, *scale),
                        (aux.proto()[usize::from(effect_index)], 0x1000),
                        "n {n} group {group}"
                    );
                    let (colour, blend) = match tint {
                        Some(t) => {
                            tinted += 1;
                            (t, CUE_TINT_BLEND)
                        }
                        None => (0xEEEE_EEEE, 0xEEEE),
                    };
                    assert_eq!(
                        (cpu.mem.rd32(node + 0x74), cpu.mem.rd16(node + 0x78)),
                        (colour, blend),
                        "n {n}"
                    );
                    effects += 1;
                }
            }
        }
        assert_eq!(it.next(), None, "n {n} group {group}: retail spawned more");
        assert_eq!(cpu.mem.rd32(a + 0x04), port.actor_state, "n {n}");
        assert_eq!(
            cpu.mem.rd32(a + 0x0C),
            port.actor_blend.unwrap_or(0x1234_5678),
            "n {n}"
        );
    }
    assert!(
        actor_cues > 1000 && effects > 3000 && copies > 500 && tinted > 1000,
        "{actor_cues} {effects} {copies} {tinted}"
    );
}
