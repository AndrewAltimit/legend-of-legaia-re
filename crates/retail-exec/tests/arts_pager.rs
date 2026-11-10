//! The Triangle arts list's pager `FUN_801D3748` against `arts_list_pager`:
//! every learned count, every open page and the closed list, a press of the
//! configured button and of others. The open bit `0x801F4E09 & 1` and the
//! row start `_DAT_8007B458` are read back as the port's page; the list's
//! registered bit `0x80` rides through untouched, and an opening press
//! rewinds the row start and the slide byte `0x801F4E08` and plays the
//! cue `FUN_800319A8(1)`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::arts_command_input::arts_list_pager;
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const PAGER: u32 = 0x801D_3748;
const CUE: u32 = 0x8003_19A8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const BUTTON: u32 = 0x8008_4140 + 0x598;
const CHAR: u32 = 0x8007_BB8C;
const RECORDS: u32 = 0x8008_4140;
const LIST_BITS: u32 = 0x801F_4E09;
const LIST_SLIDE: u32 = 0x801F_4E08;
const ROW_START: u32 = 0x8007_B458;

#[test]
fn the_arts_pager_pages_as_retail_pages() {
    differential!(prot(898, PAGER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let cues = Rc::new(RefCell::new(Vec::new()));
    {
        let cues = cues.clone();
        cpu.hook(CUE, move |r, _| cues.borrow_mut().push(r.r[A0]));
    }
    let mut rng = Rng(0x5041_4745);
    let (mut opened, mut paged, mut closed, mut ignored) = (0, 0, 0, 0);
    for n in 0..40_000 {
        let ch = rng.below(3);
        cpu.mem.wr32(CHAR, ch);
        let learned = match rng.below(4) {
            0 => 0,
            _ => rng.below(17) as u8,
        };
        cpu.mem.wr8(RECORDS + ch * 0x414 + 0x74D, learned);
        let button = 1u32 << rng.below(16);
        cpu.mem.wr32(BUTTON, button);
        let pad = match rng.below(4) {
            0 => rng.next() & !button,
            _ => button | (rng.next() & 0xFFFF),
        };
        let open = match rng.below(4) {
            0 => None,
            k => Some(k as u8 - 1),
        };
        let registered = (rng.below(2) as u8) << 7;
        cpu.mem.wr8(
            LIST_BITS,
            registered | u8::from(open.is_some()) | (rng.below(2) as u8) << 1,
        );
        cpu.mem.wr32(
            ROW_START,
            open.map_or(rng.below(3) * 5, |p| u32::from(p) * 5),
        );
        cpu.mem.wr8(LIST_SLIDE, 7);
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(PAGER, &[pad]), Stop::Returned, "n {n}");

        let bits = cpu.mem.rd8(LIST_BITS);
        let retail = (bits & 1 != 0).then(|| (cpu.mem.rd32(ROW_START) / 5) as u8);
        let port = if pad & button == 0 {
            ignored += 1;
            open
        } else {
            arts_list_pager(open, learned)
        };
        assert_eq!(
            retail, port,
            "n {n} open {open:?} learned {learned} pad {pad:#x}"
        );
        assert_eq!(bits & 0x80, registered, "n {n}");
        let opening = open.is_none() && port.is_some();
        assert_eq!(
            cues.borrow().as_slice(),
            if opening { &[1u32][..] } else { &[][..] },
            "n {n}"
        );
        assert_eq!(
            cpu.mem.rd8(LIST_SLIDE),
            if opening { 0 } else { 7 },
            "n {n}"
        );
        match (open, port) {
            (None, Some(_)) => opened += 1,
            (Some(a), Some(b)) if a != b => paged += 1,
            (Some(_), None) => closed += 1,
            _ => {}
        }
    }
    assert!(
        opened > 3000 && paged > 1000 && closed > 3000 && ignored > 5000,
        "{opened} {paged} {closed} {ignored}"
    );
}
