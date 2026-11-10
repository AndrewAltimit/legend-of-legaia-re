//! The SEQ resource-slot release `FUN_8001FF58` against
//! `SeqResourceTable::release`: the 12-byte slots at `0x80091508`, a loaded
//! slot (`+0xB` non-zero) cleared and its handle byte `+0x8` handed,
//! sign-extended, to the close `FUN_80068C80`; an unloaded one left alone.
//! The whole table is compared.

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::{SeqResourceSlot, SeqResourceTable};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const RELEASE: u32 = 0x8001_FF58;
const CLOSE: u32 = 0x8006_8C80;
const TABLE: u32 = 0x8009_1508;
const SLOTS: usize = 16;

#[test]
fn a_slot_releases_as_retail_releases_it() {
    differential!(scus(RELEASE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let closed: Rc<RefCell<Vec<i8>>> = Rc::new(RefCell::new(Vec::new()));
    let c = closed.clone();
    cpu.hook(CLOSE, move |r, _| c.borrow_mut().push(r.r[A0] as i8));
    let mut rng = Rng(0x534C_4F54);
    let (mut fired, mut idle) = (0, 0);
    for run in 0..20_000 {
        let mut raw: Vec<u8> = (0..SLOTS * 12).map(|_| rng.next() as u8).collect();
        let mut table = SeqResourceTable::new(SLOTS);
        for i in 0..SLOTS {
            // The loaded byte as the installers leave it, or clear.
            raw[i * 12 + 0xB] = [0, 1, 3, 0][rng.below(4) as usize];
            *table.slot_mut(i).unwrap() = SeqResourceSlot {
                handle: raw[i * 12 + 8] as i8,
                loaded: raw[i * 12 + 0xB] != 0,
            };
        }
        cpu.mem.load(TABLE, &raw);
        let index = rng.below(SLOTS as u32) as usize;
        closed.borrow_mut().clear();
        assert_eq!(cpu.call(RELEASE, &[index as u32]), Stop::Returned);
        let retail = cpu.mem.read(TABLE, SLOTS * 12);

        let mut port_closed = Vec::new();
        let did = table.release(index, |h| port_closed.push(h));
        let mut port = raw.clone();
        for i in 0..SLOTS {
            let s = table.slot(i).unwrap();
            port[i * 12 + 8] = s.handle as u8;
            if !s.loaded {
                port[i * 12 + 0xB] = 0;
            }
        }
        assert_eq!(retail, port, "run {run} slot {index}");
        assert_eq!(*closed.borrow(), port_closed, "run {run} slot {index}");
        fired += u32::from(did);
        idle += u32::from(!did);
    }
    assert!(fired > 5000 && idle > 5000, "{fired} {idle}");
}
