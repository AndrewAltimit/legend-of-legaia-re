//! The battle XA seek-ahead pass `FUN_8004DA00` against
//! `legaia_engine_audio::battle_voice::battle_voice_step`: random gates,
//! seats, action classes and spell ids over the shipped spell table, the
//! drive seek `FUN_8003EAE4` hooked to record the clip and the passive sweep
//! the pass ends on stubbed. Compared: the clip sought (or none), the latch
//! `_DAT_8007BDB0` and the drive-state word the seek clears.

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::battle_voice::{
    BattleVoiceAction, BattleVoiceCtx, BattleVoiceStep, BattleVoiceTables, battle_voice_step,
};
use psx_retail_exec::{A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const PASS: u32 = 0x8004_DA00;
const SEEK: u32 = 0x8003_EAE4;
const SWEEP: u32 = 0x8004_CE2C;
const CD_BUSY: u32 = 0x8007_BC20;
const STREAM_GATE: u32 = 0x8007_BD71;
const LATCH: u32 = 0x8007_BDB0;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8010_1000;

#[test]
fn the_battle_voice_seek_matches_retail() {
    differential!(scus(PASS));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let sought = Rc::new(RefCell::new(None));
    let s = sought.clone();
    cpu.hook(SEEK, move |r, _| *s.borrow_mut() = Some(r.r[A1]));
    cpu.hook(SWEEP, |_, _| {});
    cpu.mem.wr32(CTX_PTR, CTX);
    let spell_class: Vec<u8> = (0..256u32)
        .map(|id| cpu.mem.rd8(0x8007_54C8 + id * 12))
        .collect();
    let mut rng = Rng(0x564F_4943);
    let mut seen = [0usize; 3];
    for case in 0..20_000 {
        let pick = |rng: &mut Rng, rare: u32| rng.below(rare) == 0;
        let cd_busy = pick(&mut rng, 10);
        let gate = if pick(&mut rng, 10) {
            rng.next() as u8
        } else {
            0xFF
        };
        let latch = if pick(&mut rng, 4) {
            rng.below(0x30) as i32
        } else {
            -1
        };
        let ctx = BattleVoiceCtx {
            suppress: if pick(&mut rng, 10) {
                1 + rng.below(255) as u8
            } else {
                0
            },
            side_band_stage: if pick(&mut rng, 10) {
                1 + rng.below(255) as u8
            } else {
                0
            },
            phase: if pick(&mut rng, 10) {
                0x5A
            } else {
                rng.next() as u8
            },
            seat: rng.below(8) as u8,
        };
        let action = BattleVoiceAction {
            class: rng.below(7) as u8,
            id: rng.next() as u8,
        };
        let party_order: Vec<u8> = (0..3).map(|_| 1 + rng.below(4) as u8).collect();
        let monster_index: Vec<u8> = (0..8).map(|_| rng.next() as u8).collect();
        let monster_clips: Vec<u8> = (0..256).map(|_| rng.next() as u8).collect();

        cpu.mem.wr32(CD_BUSY, u32::from(cd_busy));
        cpu.mem.wr8(STREAM_GATE, gate);
        cpu.mem.wr32(LATCH, latch as u32);
        cpu.mem.wr8(CTX + 0x26B, ctx.suppress);
        cpu.mem.wr8(CTX + 0x276, ctx.side_band_stage);
        cpu.mem.wr8(CTX + 7, ctx.phase);
        cpu.mem.wr8(CTX + 0x274, ctx.seat);
        for i in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * i, ACTOR);
        }
        cpu.mem.wr8(ACTOR + 0x1DE, action.class);
        cpu.mem.wr8(ACTOR + 0x1DF, action.id);
        cpu.mem.load(0x8007_BD09, &monster_index);
        cpu.mem.load(0x8007_BD10, &party_order);
        cpu.mem.load(0x8007_87AF, &monster_clips);
        // The party order sits right past the monster indices: seats 7.. read it.
        let monster_index: Vec<u8> = cpu.mem.read(0x8007_BD09, 8);
        *sought.borrow_mut() = None;

        assert_eq!(cpu.call(PASS, &[]), Stop::Returned);
        let port = battle_voice_step(
            cd_busy,
            gate,
            latch,
            ctx,
            action,
            BattleVoiceTables {
                party_order: &party_order,
                monster_index: &monster_index,
                monster_clips: &monster_clips,
                spell_class: &spell_class,
            },
        );
        let at =
            format!("case {case}: {ctx:?} {action:?} busy {cd_busy} gate {gate:#x} latch {latch}");
        let got_latch = cpu.mem.rd32(LATCH) as i32;
        match port {
            BattleVoiceStep::Arm { clip } => {
                assert_eq!(*sought.borrow(), Some(u32::from(clip)), "{at}");
                assert_eq!(got_latch, i32::from(clip), "{at}");
                assert_eq!(
                    cpu.mem.rd32(CD_BUSY),
                    0,
                    "{at}: the seek clears the drive word"
                );
                seen[0] += 1;
            }
            BattleVoiceStep::ClearLatch => {
                assert_eq!(*sought.borrow(), None, "{at}");
                assert_eq!(got_latch, -1, "{at}");
                seen[1] += 1;
            }
            BattleVoiceStep::Hold => {
                assert_eq!(*sought.borrow(), None, "{at}");
                assert_eq!(got_latch, latch, "{at}");
                seen[2] += 1;
            }
        }
    }
    assert!(seen.iter().all(|&n| n > 1000), "{seen:?}");
}
