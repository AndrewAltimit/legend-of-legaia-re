//! The capture-class trampolines (the routine PROT 0898's `0x801CF56C` arm
//! calls in a module that holds several choreographies) against
//! `cast_module_ticks::capture_tick_body`: every trampoline of
//! `CAPTURE_TRAMPOLINES`, run off its own image at slot-B base `0x801F69D8`
//! for all 256 queued action ids `caster[+0x1DF]`, with the caster read
//! through `ctx[+0x13]` and the actor table. The body the trampoline calls
//! (its first `jal`; every call outside the trampoline returns at once) must
//! be the port's, and an id the port maps to nothing must call nothing.

use crate::common::{disc, scus_cpu};
use legaia_engine_cast::cast_module_ticks::{CAPTURE_TRAMPOLINES, capture_tick_body};
use psx_retail_exec::{Stop, V0};

const BASE: u32 = 0x801F_69D8;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
/// What the caller leaves in `a0`, which a fall-through hands back.
const A0_IN: u32 = 0x1234_5678;

const T938: u32 = 0x801F_7A40;
const T940: u32 = 0x801F_8228;
const T941: u32 = 0x801F_7D38;
const T942: u32 = 0x801F_80A0;
const T943: u32 = 0x801F_7624;
const T944: u32 = 0x801F_7EBC;
const T945: u32 = 0x801F_76F4;
const T950: u32 = 0x801F_8190;
const T951: u32 = 0x801F_816C;
const T952: u32 = 0x801F_7B28;
const T955: u32 = 0x801F_92A4;
const T956: u32 = 0x801F_7E4C;
const T957: u32 = 0x801F_9BA8;
const T958: u32 = 0x801F_8E60;
const T959: u32 = 0x801F_87F4;
const T960: u32 = 0x801F_8638;
const T961: u32 = 0x801F_7A54;
const T962: u32 = 0x801F_8080;
const T963: u32 = 0x801F_8438;
const T964: u32 = 0x801F_8E3C;
const T965: u32 = 0x801F_7B1C;

#[test]
fn every_trampoline_reaches_the_ports_body() {
    differential!(
        prot(938, T938),
        prot(940, T940),
        prot(941, T941),
        prot(942, T942),
        prot(943, T943),
        prot(944, T944),
        prot(945, T945),
        prot(950, T950),
        prot(951, T951),
        prot(952, T952),
        prot(955, T955),
        prot(956, T956),
        prot(957, T957),
        prot(958, T958),
        prot(959, T959),
        prot(960, T960),
        prot(961, T961),
        prot(962, T962),
        prot(963, T963),
        prot(964, T964),
        prot(965, T965)
    );
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut failures = Vec::new();
    let mut bodies = 0;
    for t in CAPTURE_TRAMPOLINES.iter() {
        let mut cpu = scus_cpu().expect("disc");
        let module = disc.read_entry(t.prot_entry as usize).expect("module");
        cpu.mem.load(BASE, &module);
        cpu.mem.wr32(CTX_PTR, CTX);
        // Everything the trampoline calls is a tick body: return at once.
        cpu.stub_outside = Some((t.trampoline, t.trampoline + 0x100));
        for seat in 0..8u32 {
            cpu.mem.wr32(ACTORS + seat * 4, ACTOR + seat * 0x400);
        }
        for id in 0..=255u8 {
            let seat = u32::from(id) % 8;
            cpu.mem.wr8(CTX + 0x13, seat as u8);
            for s in 0..8u32 {
                cpu.mem.wr8(ACTOR + s * 0x400 + 0x1DF, !id);
            }
            cpu.mem.wr8(ACTOR + seat * 0x400 + 0x1DF, id);
            let stop = cpu.call(t.trampoline, &[A0_IN]);
            assert_eq!(
                stop,
                Stop::Returned,
                "PROT {:04} id {id:#04x}",
                t.prot_entry
            );
            let retail = cpu.calls.first().copied();
            let port = capture_tick_body(t.prot_entry, id);
            if retail != port && failures.len() < 16 {
                failures.push(format!(
                    "PROT {:04} id {id:#04x}: retail {retail:x?} port {port:x?}",
                    t.prot_entry
                ));
            }
            match retail {
                Some(_) => {
                    bodies += 1;
                    assert_eq!(cpu.calls.len(), 1, "PROT {:04} id {id:#04x}", t.prot_entry);
                }
                // A fall-through hands the caller's `a0` back, or zero where
                // a single-arm `bne` clears it in its delay slot.
                None => assert!(
                    matches!(cpu.regs.r[V0], 0 | A0_IN),
                    "PROT {:04} id {id:#04x}: v0 {:#x}",
                    t.prot_entry,
                    cpu.regs.r[V0]
                ),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let arms: usize = CAPTURE_TRAMPOLINES.iter().map(|t| t.arms.len()).sum();
    assert_eq!(bodies, arms, "bodies reached");
}
