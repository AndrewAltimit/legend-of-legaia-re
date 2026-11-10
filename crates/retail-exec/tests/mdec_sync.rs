//! The STR overlay's MDEC DMA sync pair off the disc (PROT 0970, base
//! `0x801CE818`): the mode wrappers `FUN_801CFE20` / `FUN_801CFE5C` and the
//! blocking waits beneath them `FUN_801D0100` / `FUN_801D0198`, against
//! `legaia_engine_system::movie::mdec_dma_sync`. The status words are RAM
//! the register pointers are aimed at, so a wait sees one value throughout:
//! idle returns at once, busy runs the whole countdown out and reports
//! through the trace printer, which is recorded.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_system::movie::mdec_dma_sync::{
    IN_TIMEOUT_MESSAGE, MdecSync, OUT_TIMEOUT_MESSAGE, SYNC_TIMEOUT, mdec_in_sync, mdec_out_sync,
    wait_mdec_in_idle, wait_mdec_out_idle,
};
use psx_retail_exec::{A0, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801C_E818;
const IN_SYNC: u32 = 0x801C_FE20;
const OUT_SYNC: u32 = 0x801C_FE5C;
const IN_WAIT: u32 = 0x801D_0100;
const OUT_WAIT: u32 = 0x801D_0198;
/// The timeout's trace printer, and the two names it is handed.
const TRACE: u32 = 0x801D_0248;
const IN_NAME: u32 = 0x801C_E9AC;
const OUT_NAME: u32 = 0x801C_E9BC;
/// The pointers to the MDEC status words (in, out).
const IN_REG_PTR: u32 = 0x801D_0E94;
const OUT_REG_PTR: u32 = 0x801D_0E74;
const IN_REG: u32 = 0x801F_A000;
const OUT_REG: u32 = 0x801F_A010;

fn sync_cpu() -> Option<(Cpu, Rc<RefCell<Vec<u32>>>)> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(BASE, &disc()?.read_entry(970).ok()?);
    cpu.mem.wr32(IN_REG_PTR, IN_REG);
    cpu.mem.wr32(OUT_REG_PTR, OUT_REG);
    cpu.step_budget = 40_000_000;
    let traced: Rc<RefCell<Vec<u32>>> = Rc::default();
    let t = traced.clone();
    cpu.hook(TRACE, move |r, _| t.borrow_mut().push(r.r[A0]));
    Some((cpu, traced))
}

#[test]
fn the_mdec_sync_pair_matches_the_overlay() {
    differential!(
        prot(970, IN_SYNC),
        prot(970, OUT_SYNC),
        prot(970, IN_WAIT),
        prot(970, OUT_WAIT)
    );
    let Some((mut cpu, traced)) = sync_cpu() else {
        return;
    };
    let mut rng = Rng(0x4D44_5359);
    let (mut timeouts, mut polls) = (0usize, 0usize);
    for case in 0..48 {
        // Busy words now and then: each one runs the whole countdown.
        let busy = case % 6 == 0;
        let mut in_word = rng.next();
        let mut out_word = rng.next();
        if !busy {
            in_word &= !0x2000_0000;
            out_word &= !0x0100_0000;
        } else {
            in_word |= 0x2000_0000;
            out_word |= 0x0100_0000;
        }
        cpu.mem.wr32(IN_REG, in_word);
        cpu.mem.wr32(OUT_REG, out_word);
        let mode = if case % 2 == 0 { 0 } else { rng.next() | 1 };
        for (entry, wait, name) in [(IN_SYNC, IN_WAIT, IN_NAME), (OUT_SYNC, OUT_WAIT, OUT_NAME)] {
            for direct in [false, true] {
                if direct && mode != 0 {
                    continue;
                }
                traced.borrow_mut().clear();
                let (call, args) = if direct {
                    (wait, vec![])
                } else {
                    (entry, vec![mode])
                };
                assert_eq!(cpu.call(call, &args), Stop::Returned, "case {case}");
                let got = cpu.regs.r[V0] as i32;
                let port = if direct {
                    let r = if entry == IN_SYNC {
                        wait_mdec_in_idle(|| in_word)
                    } else {
                        wait_mdec_out_idle(|| out_word)
                    };
                    MdecSync::Waited(r)
                } else if entry == IN_SYNC {
                    mdec_in_sync(mode as i32, || in_word)
                } else {
                    mdec_out_sync(mode as i32, || out_word, || in_word)
                };
                assert_eq!(
                    got,
                    port.value(),
                    "case {case} entry {entry:08X} direct {direct}"
                );
                match port {
                    MdecSync::Waited(r) => {
                        // A timeout traces its channel's name; a clear one nothing.
                        let want: Vec<u32> = r.timeout_message.map(|_| name).into_iter().collect();
                        assert_eq!(*traced.borrow(), want, "case {case}");
                        if let Some(m) = r.timeout_message {
                            assert_eq!(r.code, SYNC_TIMEOUT);
                            let named = if name == IN_NAME {
                                IN_TIMEOUT_MESSAGE
                            } else {
                                OUT_TIMEOUT_MESSAGE
                            };
                            assert_eq!(m, named);
                            timeouts += 1;
                        }
                    }
                    MdecSync::Polled(_) => polls += 1,
                }
            }
        }
    }
    assert!(timeouts >= 12 && polls >= 40, "{timeouts} {polls}");
}
