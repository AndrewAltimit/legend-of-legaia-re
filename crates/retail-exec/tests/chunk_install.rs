//! The streaming-chunk installer `FUN_8001E54C` off the disc, list arm
//! (`a0 >> 16 == 0`), against `install_chunks`: every leaf call it makes in
//! order - the SEQ release, the raw copies and where they land, the VAB
//! uploads with their budget word, the SEQ opens and the record they open -
//! the slot's loaded byte, the cross-call stream flag and the return word,
//! over generated chunk lists of every type byte, all three transfer modes
//! and budgets on both sides of the type-3 chunk.

use crate::common::{Rng, scus_cpu};
use legaia_engine_system::chunk_install::{
    CopyDest, InstallHost, SeqRecord, SeqSlot, install_chunks,
};
use psx_retail_exec::{A0, A1, A2, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const INSTALL: u32 = 0x8001_E54C;
const RELEASE: u32 = 0x8001_FF58;
const RAW_COPY: u32 = 0x8001_A8B0;
const VAB_UPLOAD: u32 = 0x8002_630C;
const SEQ_DETACH: u32 = 0x8002_66E0;
const SEQ_CLOSE: u32 = 0x8002_6520;
const SEQ_OPEN: u32 = 0x8002_6410;
/// The 12-byte-stride resource table and the SEQ records.
const SLOTS: u32 = 0x8009_1508;
const SEQ_RECORDS: u32 = 0x8007_051C;
const FIXED_RECORD: u32 = SEQ_RECORDS + 0x90;
const MODE: u32 = 0x8007_B8B8;
const BUSY: u32 = 0x8007_B868;
/// `gp + 0x700`.
const STREAM_FLAG: u32 = 0x8007_BA18;
const BUF: u32 = 0x8010_0000;
const DEST: u32 = 0x8014_0000;
const STAGE: u32 = 0x8015_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retail {
    Release(i8),
    Copy(u32, u32, u32),
    Vab(u32, u32, i8, u32),
    Detach(u32),
    Close(u32),
    Open(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Release(i8),
    Copy(CopyDest, usize, usize),
    Vab(i8, usize, u32),
    Seq(SeqRecord, i8, Option<(usize, usize)>),
}

struct Spy {
    base: usize,
    mode: u32,
    events: Vec<Event>,
    flagged: bool,
}

impl InstallHost for Spy {
    fn seq_release(&mut self, id: i8) {
        self.events.push(Event::Release(id));
    }
    fn raw_copy(&mut self, dest: CopyDest, data: &[u8]) {
        self.events.push(Event::Copy(
            dest,
            data.as_ptr() as usize - self.base,
            data.len(),
        ));
    }
    fn vab_upload(&mut self, id: i8, data: &[u8], budget: Option<usize>) {
        self.events.push(Event::Vab(
            id,
            data.as_ptr() as usize - self.base,
            budget.unwrap_or(0) as u32,
        ));
    }
    fn seq_install(&mut self, record: SeqRecord, id: i8, data: &[u8]) {
        // The fixed record's copy takes none of the chunk: its three
        // arguments are resource record 9's words.
        let payload = (record == SeqRecord::Slot)
            .then(|| (data.as_ptr() as usize - self.base, data.len()));
        self.events.push(Event::Seq(record, id, payload));
    }
    fn set_stream_flag(&mut self) {
        self.flagged = true;
    }
    fn transfer_mode(&self) -> u32 {
        self.mode
    }
}

#[test]
fn the_chunk_installer_routes_what_retail_routes() {
    differential!(scus(INSTALL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<Retail>>> = Rc::default();
    macro_rules! hook {
        ($va:expr, $f:expr) => {{
            let log = log.clone();
            cpu.hook($va, move |r, m| {
                let f: fn(&psx_retail_exec::Regs, &mut psx_retail_exec::Memory) -> Retail = $f;
                log.borrow_mut().push(f(r, m));
                r.r[V0] = 0;
            });
        }};
    }
    hook!(RELEASE, |r, _| Retail::Release(r.r[A0] as i8));
    hook!(RAW_COPY, |r, _| Retail::Copy(r.r[A0], r.r[A1], r.r[A2]));
    hook!(VAB_UPLOAD, |r, m| Retail::Vab(
        r.r[A0],
        r.r[A1],
        r.r[A2] as i8,
        m.rd32(r.r[SP] + 0x10)
    ));
    hook!(SEQ_DETACH, |r, _| Retail::Detach(r.r[A0]));
    hook!(SEQ_CLOSE, |r, _| Retail::Close(r.r[A0]));
    hook!(SEQ_OPEN, |r, _| Retail::Open(r.r[A0]));

    let mut rng = Rng(0x4348_554E);
    let mut seen = [0u32; 16];
    let (mut budget_stops, mut short_budget, mut sub_word) = (0, 0, 0);
    for round in 0..6_000 {
        // The chunk list.
        let mut stream = Vec::new();
        let mut type3_at = None;
        for _ in 0..rng.below(7) {
            let ty = match rng.below(12) {
                0..=4 => rng.below(5) as u8,
                5 => 0xC,
                6 | 7 => 2,
                8 => 3,
                _ => rng.below(0x12) as u8,
            };
            let size = match rng.below(12) {
                0 => {
                    sub_word += 1;
                    1 + rng.below(3)
                }
                1 => 4 * (1 + rng.below(12)) + 1 + rng.below(3),
                _ => 4 * (1 + rng.below(12)),
            };
            stream.extend_from_slice(&((u32::from(ty) << 24) | size).to_le_bytes());
            if ty == 3 && size >= 4 && type3_at.is_none() {
                type3_at = Some(stream.len());
            }
            // The walk steps `size >> 2` words, so the tail of a payload
            // whose size is no multiple of four lies over the next header.
            let body = (size as usize / 4) * 4;
            stream.extend((0..body).map(|_| rng.next() as u8));
            seen[usize::from(ty.min(15))] += 1;
        }
        stream.extend_from_slice(&((rng.below(256) << 24) & 0xFF00_0000).to_le_bytes());
        stream.extend((0..16).map(|_| rng.next() as u8));

        let slot = rng.below(9);
        let id = rng.next() as i8;
        let mode = rng.below(3);
        // A budget at or past the type-3 payload, or (rarely) short of it.
        let budget = match (type3_at, rng.below(8)) {
            (Some(at), 0) => {
                short_budget += 1;
                rng.below(at as u32 + 1)
            }
            (Some(at), _) => at as u32 + rng.below(0x80),
            (None, _) => rng.below(0x400),
        };

        let rec = SLOTS + 12 * slot;
        let seq = SEQ_RECORDS + 0x10 * slot;
        for a in (SLOTS..SLOTS + 12 * 10).step_by(4) {
            cpu.mem.wr32(a, 0);
        }
        for a in (SEQ_RECORDS..SEQ_RECORDS + 0x10 * 10).step_by(4) {
            cpu.mem.wr32(a, 0x5A5A_5A5A);
        }
        cpu.mem.wr32(rec, DEST);
        cpu.mem.wr32(rec + 4, STAGE);
        cpu.mem.wr8(rec + 8, id as u8);
        cpu.mem.wr32(MODE, mode);
        cpu.mem.wr32(BUSY, 0);
        cpu.mem.wr32(STREAM_FLAG, 0);
        cpu.mem.load(BUF, &stream);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(INSTALL, &[slot, BUF, budget]),
            Stop::Returned,
            "round {round}"
        );
        let retail_ret = cpu.regs.r[V0];
        let retail_loaded = cpu.mem.rd8(rec + 0xB) != 0;
        let retail_flag = cpu.mem.rd32(STREAM_FLAG) != 0;

        // Fold the retail calls into the port's events.
        let raw = log.borrow().clone();
        let mut retail = Vec::new();
        let mut i = 0;
        let mut detached = false;
        while i < raw.len() {
            match raw[i] {
                Retail::Release(id) => retail.push(Event::Release(id)),
                Retail::Vab(dst, src, id, stack) => {
                    assert_eq!(dst, DEST, "round {round}: VAB destination");
                    retail.push(Event::Vab(id, (src - BUF) as usize, stack));
                }
                Retail::Detach(a) => {
                    assert_eq!(a, seq, "round {round}: detach record");
                    assert_eq!(raw.get(i + 1), Some(&Retail::Close(seq)), "round {round}");
                    assert_ne!(mode, 2, "round {round}: detach under mode 2");
                    detached = true;
                    i += 1;
                }
                Retail::Close(_) => panic!("round {round}: close with no detach"),
                Retail::Copy(dst, src, len) => {
                    if raw.get(i + 1) == Some(&Retail::Open(seq)) && dst == STAGE {
                        assert!(detached || mode == 2, "round {round}: open with no detach");
                        if mode != 2 {
                            assert_eq!(cpu.mem.rd32(seq), STAGE, "round {round}: score pointer");
                        }
                        let stamped = cpu.mem.rd16(seq + 0xC) as i16;
                        assert_eq!(stamped, i16::from(id), "round {round}: record id");
                        retail.push(Event::Seq(
                            SeqRecord::Slot,
                            id,
                            Some(((src - BUF) as usize, len as usize)),
                        ));
                        detached = false;
                        i += 1;
                    } else if raw.get(i + 1) == Some(&Retail::Open(FIXED_RECORD)) {
                        // Record 9's three words, whatever the chunk holds.
                        let nine = SLOTS + 12 * 9;
                        assert_eq!(
                            [dst, src, len],
                            [
                                cpu.mem.rd32(nine),
                                cpu.mem.rd32(nine + 4),
                                cpu.mem.rd32(nine + 8)
                            ],
                            "round {round}: fixed-record copy"
                        );
                        let stamped = cpu.mem.rd16(FIXED_RECORD + 0xC) as i16;
                        assert_eq!(stamped, i16::from(id), "round {round}: fixed id");
                        retail.push(Event::Seq(SeqRecord::Fixed, id, None));
                        detached = false;
                        i += 1;
                    } else {
                        let dest = match dst {
                            DEST => CopyDest::SlotDest,
                            STAGE => CopyDest::SlotStage,
                            _ => panic!("round {round}: copy to {dst:#x}"),
                        };
                        retail.push(Event::Copy(dest, (src - BUF) as usize, len as usize));
                    }
                }
                Retail::Open(a) => panic!("round {round}: bare open of {a:#x}"),
            }
            i += 1;
        }

        let mut spy = Spy {
            base: stream.as_ptr() as usize,
            mode,
            events: Vec::new(),
            flagged: false,
        };
        let mut port_slot = SeqSlot { id, loaded: false };
        let port_ret = install_chunks(&mut spy, &mut port_slot, &stream, budget as usize);
        assert_eq!(retail, spy.events, "round {round}: leaf calls");
        assert_eq!(retail_loaded, port_slot.loaded, "round {round}: loaded");
        assert_eq!(retail_flag, spy.flagged, "round {round}: stream flag");
        assert_eq!(retail_ret, port_ret as u32, "round {round}: return");
        if retail_ret != 0 {
            budget_stops += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
    assert!(
        budget_stops > 300 && short_budget > 50 && sub_word > 500,
        "{budget_stops} {short_budget} {sub_word}"
    );
}
