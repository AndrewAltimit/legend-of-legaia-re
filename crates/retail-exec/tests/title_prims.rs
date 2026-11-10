//! Two primitive helpers the title and save screens share: the sprite
//! descriptor dispatcher `FUN_800198E0` (`SCUS_942.54`) against
//! `exec_sprite_descriptor` over generated descriptors of both variants -
//! the emits, the STP-OR pass over the pixel bytes and the texpage ->
//! CLUT-row table `0x8007BEC0` - and the centred bar `FUN_801E36C4` (menu
//! overlay, PROT 0899) against `exec_centered_bar`. The emit, packet and bar
//! routines are hooked and logged; the descriptor's main record is resolved
//! from the bytes the way retail resolves it (`desc + 8 + (len & !3)` under
//! `flags & 8`), which is the caller's job in the port.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_title::title_prim::{
    PrimHost, Rect12, SpriteDescriptor, exec_centered_bar, exec_sprite_descriptor,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const DISPATCH: u32 = 0x8001_98E0;
const BAR: u32 = 0x801E_36C4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const EMIT: u32 = 0x8005_83C8;
const PACKET: u32 = 0x8003_4B6C;
const HBAR: u32 = 0x8002_C69C;
const GATE: u32 = 0x8007_B998;
const TABLE: u32 = 0x8007_BEC0;
const DESC: u32 = 0x8010_0000;
const SPAN: usize = 0x6_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ev {
    Emit(Rect12, u32),
    Packet(u32),
    Bar(i16, i16, i16, u32),
}

struct Host {
    events: Vec<Ev>,
    gate: bool,
    /// The bytes from `DESC` on, which the STP-OR pass edits in place.
    ram: Vec<u8>,
    table: [u16; 32],
}

impl PrimHost for Host {
    fn queue_clear_rect(&mut self, _: Rect12, _: u8, _: u8, _: u8) {
        unreachable!()
    }
    fn queue_move_image(&mut self, _: Rect12, _: u16, _: i16) {
        unreachable!()
    }
    fn emit_sprite(&mut self, rect: Rect12, ptr: u32, _strip: bool) {
        self.events.push(Ev::Emit(rect, ptr));
    }
    fn stp_or_pixels(&mut self, ptr: u32, count: i32) {
        let base = (ptr - DESC) as usize;
        for i in 0..count as usize {
            let o = base + 2 * i;
            let v = u16::from_le_bytes([self.ram[o], self.ram[o + 1]]);
            if v != 0 {
                self.ram[o..o + 2].copy_from_slice(&(v | 0x8000).to_le_bytes());
            }
        }
    }
    fn stp_or_gate_set(&self) -> bool {
        self.gate
    }
    fn record_texpage_clut_row(&mut self, texpage: u8, row: u16) {
        self.table[usize::from(texpage)] = row;
    }
    fn prim_packet_alloc(&mut self, t: u8) {
        self.events.push(Ev::Packet(u32::from(t)));
    }
    fn queue_horizontal_bar(&mut self, x: i16, y: i16, w: i16, color: u32) {
        self.events.push(Ev::Bar(x, y, w, color));
    }
}

fn log_emits(cpu: &mut Cpu) -> Rc<RefCell<Vec<Ev>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(EMIT, move |r, m| {
        let mut h = |a: u32| m.rd16(a) as i16;
        let a = r.r[A0];
        let rect = Rect12::new(h(a), h(a + 2), h(a + 4), h(a + 6));
        l.borrow_mut().push(Ev::Emit(rect, r.r[A1]));
    });
    let l = log.clone();
    cpu.hook(PACKET, move |r, _| l.borrow_mut().push(Ev::Packet(r.r[A0])));
    let l = log.clone();
    cpu.hook(HBAR, move |r, _| {
        l.borrow_mut().push(Ev::Bar(
            r.r[A0] as i16,
            r.r[A1] as i16,
            r.r[A2] as i16,
            r.r[A3],
        ))
    });
    log
}

fn rect_at(cpu: &mut Cpu, a: u32) -> Rect12 {
    let mut h = |o: u32| cpu.mem.rd16(a + o) as i16;
    Rect12::new(h(0), h(2), h(4), h(6))
}

#[test]
fn the_sprite_dispatcher_emits_and_records_what_retail_does() {
    differential!(scus(DISPATCH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = log_emits(&mut cpu);
    let mut rng = Rng(0x5350_5249);
    let (mut simple, mut clut, mut ored, mut wrapped) = (0, 0, 0, 0);
    for case in 0..3000 {
        let tag = [0x11u32, 0x10, 0x10, 0x12][rng.below(4) as usize];
        let flags = rng.below(16) | ((rng.next() & 0xFFF0) * u32::from(rng.below(4) == 0));
        // The first record: a CLUT strip, now and then one whose `w * h`
        // overflows the halfword it is stored in.
        let (w, h) = match rng.below(6) {
            0 => (0x100 + rng.below(0x100), 0x100 + rng.below(0x40)),
            1 => (0, rng.below(4)),
            _ => ([16u32, 256][rng.below(2) as usize], 1 + rng.below(4)),
        };
        // The block holds every pixel its header names, as a TIM's does.
        let pixels = (w * h) as usize;
        let len = 0xC + 2 * pixels as u32 + [0, 1, 2, 3][rng.below(4) as usize];
        let mut bytes = vec![0u8; SPAN];
        bytes[0..4].copy_from_slice(&tag.to_le_bytes());
        bytes[4..8].copy_from_slice(&flags.to_le_bytes());
        bytes[8..12].copy_from_slice(&len.to_le_bytes());
        for (i, v) in [rng.below(1024), rng.below(512), w, h]
            .into_iter()
            .enumerate()
        {
            bytes[12 + 2 * i..14 + 2 * i].copy_from_slice(&(v as u16).to_le_bytes());
        }
        for i in 0..pixels {
            let v: u16 = if rng.below(3) == 0 {
                0
            } else {
                rng.next() as u16
            };
            let o = 0x14 + 2 * i;
            bytes[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
        let main = 8 + (len & !3);
        for (i, v) in [
            rng.next() as u16,
            rng.next() as u16,
            rng.below(512) as u16,
            rng.below(512) as u16,
        ]
        .into_iter()
        .enumerate()
        {
            let o = (main + 4) as usize + 2 * i;
            bytes[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
        let gate = rng.below(2) == 0;
        cpu.mem.load(DESC, &bytes);
        cpu.mem.wr32(GATE, u32::from(gate));
        let mut table = [0u16; 32];
        for (i, t) in table.iter_mut().enumerate() {
            *t = rng.next() as u16;
            cpu.mem.wr16(TABLE + 2 * i as u32, *t);
        }
        log.borrow_mut().clear();
        assert_eq!(cpu.call(DISPATCH, &[DESC]), Stop::Returned, "case {case}");

        let complex = flags & 8 != 0;
        let rect = rect_at(&mut cpu, DESC + 0xC);
        // The bytes as they were: the run has OR'd the pixels in place.
        let mut port = Host {
            events: Vec::new(),
            gate,
            ram: bytes,
            table,
        };
        let main_rect = Rect12::new(
            i16::from_le_bytes([port.ram[main as usize + 4], port.ram[main as usize + 5]]),
            i16::from_le_bytes([port.ram[main as usize + 6], port.ram[main as usize + 7]]),
            i16::from_le_bytes([port.ram[main as usize + 8], port.ram[main as usize + 9]]),
            i16::from_le_bytes([port.ram[main as usize + 10], port.ram[main as usize + 11]]),
        );
        let desc = SpriteDescriptor {
            tag,
            flags,
            rect,
            pixel_data_ptr: DESC + 0x14,
            main_rect: if complex { main_rect } else { rect },
            main_pixel_data_ptr: if complex {
                DESC + main + 0xC
            } else {
                DESC + 0x14
            },
        };
        exec_sprite_descriptor(&mut port, &desc);
        let what = format!("case {case} tag {tag:#x} flags {flags:#x} {rect:?}");
        assert_eq!(*log.borrow(), port.events, "{what}");
        let after = cpu.mem.read(DESC, SPAN);
        assert!(
            after == port.ram,
            "{what}: {:?}",
            crate::common::diff_offsets(&after, &port.ram)
        );
        for (i, t) in port.table.iter().enumerate() {
            assert_eq!(cpu.mem.rd16(TABLE + 2 * i as u32), *t, "{what}: table {i}");
        }
        simple += u32::from(tag == 0x11);
        clut += u32::from(tag != 0x11 && complex);
        ored += u32::from(tag != 0x11 && complex && gate && rect.w != 0 && rect.h != 0);
        wrapped += u32::from(tag != 0x11 && complex && (w * h) > 0xFFFF);
    }
    assert!(
        simple > 300 && clut > 300 && ored > 100 && wrapped > 20,
        "{simple} {clut} {ored} {wrapped}"
    );
}

#[test]
fn the_centred_bar_emits_where_retail_emits_it() {
    differential!(prot(899, BAR));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let log = log_emits(&mut cpu);
    let mut rng = Rng(0x4241_5242);
    let mut drawn = 0;
    for case in 0..20000 {
        let pick = |rng: &mut Rng| match rng.below(3) {
            0 => rng.next() as i16,
            1 => rng.below(0x200) as i16 - 0x40,
            _ => 0xF0 + rng.below(3) as i16,
        };
        let (x, y, w) = (pick(&mut rng), pick(&mut rng), pick(&mut rng));
        let color = rng.next();
        log.borrow_mut().clear();
        let args = [x as i32 as u32, y as i32 as u32, w as i32 as u32, color];
        assert_eq!(cpu.call(BAR, &args), Stop::Returned);
        let mut port = Host {
            events: Vec::new(),
            gate: false,
            ram: Vec::new(),
            table: [0; 32],
        };
        exec_centered_bar(&mut port, x, y, w, color);
        assert_eq!(*log.borrow(), port.events, "case {case} ({x}, {y}, {w})");
        drawn += u32::from(!port.events.is_empty());
    }
    assert!(drawn > 5000, "{drawn}");
}
