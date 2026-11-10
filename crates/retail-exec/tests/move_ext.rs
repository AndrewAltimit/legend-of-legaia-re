//! The move VM's overlay escape `FUN_801D362C` (PROT 0897), every sub-op
//! `0x00..=0x3C` and the out-of-range arm, called the way op `0x2F` calls it
//! (`a0` = the actor, `a1` = the instruction) on a record of random bytes,
//! against the port's dispatch through `MoveHost::ext_dispatch`.
//!
//! The port hands every global the arms touch to its host. The host here
//! answers each from a copy of the RAM the retail arm reads and writes - the
//! slot table `0x801F3498`, the predicate and counter `0x801F22F4` /
//! `0x801F22F6`, the object-effect rows `0x80083FF8`, the scratchpad slots
//! `0x1F80035C`, `0x8007B9D8`, the fade colour bytes, the player, the camera
//! focus, the move buffer itself - and the flag bank through the `World`'s
//! own flag calls, and the copies are compared with RAM after the arm. Calls
//! that draw are stubbed; the ramp installer is hooked and its arguments
//! compared; `rand()` is answered alike on both sides; the bearing
//! `FUN_80019B28`, the colour pair `FUN_8001A6C8` / `FUN_8001A78C`, the
//! Bezier `FUN_801E45BC` and the flag calls run for real.
//!
//! What the host does with each answer in the engine is the engine host's;
//! what this holds is the kernel: which global, which value, which width.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_vm::move_vm::{ActorState, MoveHost};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const EXT: u32 = 0x801D_362C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTOR: u32 = 0x8013_0000;
const RECORD: usize = 0x1AC;
const BUF: u32 = 0x8012_0000;
const BUF_WORDS: usize = 0x80;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8013_1000;
const PARTY: [u32; 3] = [0x8013_2000, 0x8013_3000, 0x8013_4000];
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN: u32 = 0x8014_0000;
const COS: u32 = 0x8014_2000;
const FLAG_BANK: u32 = 0x8008_5758;
const FLAG_BANK_BYTES: usize = 0x200;
const SLOTS: u32 = 0x801F_3498;
const PREDICATE: u32 = 0x801F_22F4;
const COUNTER: u32 = 0x801F_22F6;
const ROWS: u32 = 0x8008_3FF8;
const SCRATCH_SLOTS: u32 = 0x1F80_035C;
const FRAME_STEP: u32 = 0x1F80_0393;
const GLOBAL_B9D8: u32 = 0x8007_B9D8;
const FADE_A: u32 = 0x8007_BF5D;
const FADE_B: u32 = 0x8007_BFD1;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const POOL_TOP: u32 = 0x8007_C348;
const PARTY_LIST_PTR: u32 = 0x8007_B898;
const PARTY_BASE_ID: u32 = 0x40;

const DEBUG_PRINT: u32 = 0x8001_A068;
const RAND: u32 = 0x8005_6798;
const RAMP_INSTALL: u32 = 0x8003_C5F0;
const STRIP: u32 = 0x801D_31B0;
const OT_A: u32 = 0x8003_D2C4;
const OT_B: u32 = 0x8005_9010;
const PARTY_LIST: u32 = 0x8003_D064;
const RESOLVE: u32 = 0x8003_C83C;

/// A copy of RAM the host reads and writes.
struct Region {
    base: u32,
    bytes: Vec<u8>,
}

/// One ramp install: destination, kind, current value, end, ticks.
type RampCall = (u32, u32, i32, i32, i32);

struct Host {
    regions: Vec<Region>,
    world: World,
    sin: Vec<i16>,
    cos: Vec<i16>,
    rand: u16,
    ramps: Vec<RampCall>,
    party: [[i16; 3]; 3],
    buf: Vec<u16>,
}

impl Host {
    fn at(&self, a: u32) -> (usize, usize) {
        for (i, r) in self.regions.iter().enumerate() {
            if a >= r.base && ((a - r.base) as usize) < r.bytes.len() {
                return (i, (a - r.base) as usize);
            }
        }
        panic!("host read outside its regions: {a:#x}");
    }
    fn rd8(&self, a: u32) -> u8 {
        let (i, o) = self.at(a);
        self.regions[i].bytes[o]
    }
    fn wr8(&mut self, a: u32, v: u8) {
        let (i, o) = self.at(a);
        self.regions[i].bytes[o] = v;
    }
    fn rd16(&self, a: u32) -> u16 {
        u16::from_le_bytes([self.rd8(a), self.rd8(a + 1)])
    }
    fn wr16(&mut self, a: u32, v: u16) {
        let [x, y] = v.to_le_bytes();
        self.wr8(a, x);
        self.wr8(a + 1, y);
    }
    fn rd32(&self, a: u32) -> u32 {
        u32::from(self.rd16(a)) | u32::from(self.rd16(a + 2)) << 16
    }
    fn wr32(&mut self, a: u32, v: u32) {
        self.wr16(a, v as u16);
        self.wr16(a + 2, (v >> 16) as u16);
    }
    fn player(&self) -> [i16; 3] {
        [0x14, 0x16, 0x18].map(|o| self.rd16(PLAYER + o) as i16)
    }
    fn row(idx: i16) -> u32 {
        ROWS.wrapping_add((i32::from(idx) * 0x14) as u32)
    }
}

const ROW_OFFSETS: [u32; 5] = [0, 2, 4, 0x10, 0x12];

impl MoveHost for Host {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let i = usize::from(index & 0xFFF);
        (self.sin[i], self.cos[i])
    }
    fn ext_rand16(&mut self) -> u16 {
        self.rand
    }
    fn ext_query_flag_bank(&self, flag_index: i16) -> u32 {
        u32::from(self.world.system_flag_test(flag_index as u16))
    }
    fn ext_set_flag_bank(&mut self, flag_index: i16) {
        self.world.system_flag_set(flag_index as u16);
    }
    fn ext_clear_flag_bank(&mut self, flag_index: i16) {
        self.world.system_flag_clear(flag_index as u16);
    }
    fn ext_scratchpad_ramp(&mut self, slot: i16, target: i32, ticks: i16) {
        let dest = SCRATCH_SLOTS.wrapping_add((i32::from(slot) * 2) as u32);
        let current = i32::from(self.rd16(dest) as i16);
        self.ramps
            .push((dest, 2, current, target, i32::from(ticks)));
    }
    fn ext_scratchpad_write(&mut self, slot: i16, value: i16) {
        let dest = SCRATCH_SLOTS.wrapping_add((i32::from(slot) * 2) as u32);
        self.wr16(dest, value as u16);
    }
    fn ext_set_8007b9d8(&mut self, value: i32) {
        self.wr32(GLOBAL_B9D8, value as u32);
    }
    fn ext_compute_angle(&self, state: &ActorState) -> u16 {
        let p = self.player();
        legaia_engine_system::trig::bearing12(
            i32::from(p[2]) - i32::from(state.world_z),
            i32::from(p[0]) - i32::from(state.world_x),
        )
    }
    fn ext_party_member_lookup(&self, slot: i16) -> Option<[i16; 3]> {
        usize::try_from(slot)
            .ok()
            .and_then(|s| self.party.get(s))
            .copied()
    }
    fn ext_fade_color(&mut self, rgb: [u8; 3], ticks: u16) {
        for (k, c) in rgb.iter().enumerate() {
            for base in [FADE_A, FADE_B] {
                let dest = base + k as u32;
                if ticks == 0 {
                    self.wr8(dest, *c);
                }
            }
        }
        if ticks != 0 {
            for base in [FADE_A, FADE_B] {
                for (k, c) in rgb.iter().enumerate() {
                    let dest = base + k as u32;
                    let current = i32::from(self.rd8(dest));
                    self.ramps
                        .push((dest, 1, current, i32::from(*c), i32::from(ticks as i16)));
                }
            }
        }
    }
    fn ext_world_struct_write(&mut self, index: i16, values: [i16; 5]) {
        for (o, v) in ROW_OFFSETS.iter().zip(values) {
            self.wr16(Self::row(index) + o, v as u16);
        }
    }
    fn ext_world_struct_init(&mut self, index: i16, values: [i16; 5]) {
        self.ext_world_struct_write(index, values);
    }
    fn ext_world_struct_add(&mut self, index: i16, deltas: [i16; 5]) {
        for (o, d) in ROW_OFFSETS.iter().zip(deltas) {
            let a = Self::row(index) + o;
            let v = self.rd16(a).wrapping_add(d as u16);
            self.wr16(a, v);
        }
    }
    fn move_slot_load_u32(&self, slot: u16, dword_off: u8) -> u32 {
        self.rd32(SLOTS + u32::from(slot) * 8 + u32::from(dword_off))
    }
    fn move_slot_save_u32(&mut self, slot: u16, dword_off: u8, value: u32) {
        self.wr32(SLOTS + u32::from(slot) * 8 + u32::from(dword_off), value);
    }
    fn move_slot_load_u16(&self, slot: u16, byte_off: u8) -> u16 {
        self.rd16(SLOTS + u32::from(slot) * 8 + u32::from(byte_off))
    }
    fn move_slot_save_u16(&mut self, slot: u16, byte_off: u8, value: u16) {
        self.wr16(SLOTS + u32::from(slot) * 8 + u32::from(byte_off), value);
    }
    fn move_global_predicate_get(&self) -> u32 {
        u32::from(self.rd16(PREDICATE))
    }
    fn move_global_predicate_set(&mut self, value: u32) {
        self.wr16(PREDICATE, value as u16);
    }
    fn move_global_counter_get(&self) -> u16 {
        self.rd16(COUNTER)
    }
    fn move_global_counter_set(&mut self, value: u16) {
        self.wr16(COUNTER, value);
    }
    fn move_player_world_xyz(&self) -> [i16; 3] {
        self.player()
    }
    fn move_camera_focus_stored_xz(&self) -> (i32, i32) {
        (self.rd32(FOCUS_X) as i32, self.rd32(FOCUS_Z) as i32)
    }
    fn move_dat_1f800393(&self) -> u8 {
        self.rd8(FRAME_STEP)
    }
    fn move_axis_threshold(&self) -> i16 {
        self.rd16(POOL_TOP) as i16
    }
    fn move_bytecode_read_u16(&self, word_off: usize) -> u16 {
        self.buf[word_off]
    }
    fn move_bytecode_write_u16(&mut self, word_off: usize, value: u16) {
        self.buf[word_off] = value;
    }
}

fn h(r: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([r[o], r[o + 1]])
}
fn w(r: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]])
}

/// The record fields the ext arms touch, by offset (`ActorState`'s names).
fn from_record(r: &[u8]) -> ActorState {
    let mut s = ActorState::new();
    s.flags = w(r, 0x10);
    s.world_x = h(r, 0x14) as i16;
    s.world_y = h(r, 0x16) as i16;
    s.world_z = h(r, 0x18) as i16;
    s.world_pad = h(r, 0x1A) as i16;
    s.render_24 = h(r, 0x24) as i16;
    s.render_26 = h(r, 0x26) as i16;
    s.render_28 = h(r, 0x28) as i16;
    s.world_y_mirror = h(r, 0x2A) as i16;
    s.anim_3c = h(r, 0x3C) as i16;
    s.anim_3e = h(r, 0x3E) as i16;
    s.anim_40 = h(r, 0x40) as i16;
    s.field_50 = h(r, 0x50);
    s.face_rotation = r[0x6D];
    s.pc = h(r, 0x70) as i16;
    s.field_72 = h(r, 0x72);
    s.field_86 = h(r, 0x86);
    s.tween_src_x = h(r, 0x90) as i16;
    s.tween_src_y = h(r, 0x92) as i16;
    s.tween_src_z = h(r, 0x94) as i16;
    for o in (0x9C..RECORD).step_by(2) {
        s.set_actor_u16(o, h(r, o));
    }
    s
}

fn to_record(s: &ActorState, r: &mut [u8]) {
    let mut p16 = |o: usize, v: u16| r[o..o + 2].copy_from_slice(&v.to_le_bytes());
    p16(0x14, s.world_x as u16);
    p16(0x16, s.world_y as u16);
    p16(0x18, s.world_z as u16);
    p16(0x1A, s.world_pad as u16);
    p16(0x24, s.render_24 as u16);
    p16(0x26, s.render_26 as u16);
    p16(0x28, s.render_28 as u16);
    p16(0x2A, s.world_y_mirror as u16);
    p16(0x3C, s.anim_3c as u16);
    p16(0x3E, s.anim_3e as u16);
    p16(0x40, s.anim_40 as u16);
    p16(0x50, s.field_50);
    p16(0x70, s.pc as u16);
    p16(0x72, s.field_72);
    p16(0x86, s.field_86);
    p16(0x90, s.tween_src_x as u16);
    p16(0x92, s.tween_src_y as u16);
    p16(0x94, s.tween_src_z as u16);
    for o in (0x9C..RECORD).step_by(2) {
        p16(o, s.actor_u16(o));
    }
    r[0x10..0x14].copy_from_slice(&s.flags.to_le_bytes());
    r[0x6D] = s.face_rotation;
}

fn retail_cpu(ramps: Rc<RefCell<Vec<RampCall>>>, rand: Rc<RefCell<u16>>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    for f in [DEBUG_PRINT, STRIP, OT_A, OT_B] {
        cpu.stub(f, 0);
    }
    cpu.hook(RAND, move |r, _| r.r[V0] = u32::from(*rand.borrow()));
    cpu.hook(RAMP_INSTALL, move |r, m| {
        let sp = r.r[SP];
        ramps.borrow_mut().push((
            r.r[A1],
            r.r[A2],
            r.r[A3] as i32,
            m.rd32(sp + 0x10) as i32,
            m.rd32(sp + 0x14) as i32,
        ));
        r.r[V0] = 1;
    });
    // The party list's base id, and the resolver over it: ids past the base
    // name the three seated members.
    cpu.hook(PARTY_LIST, |r, m| {
        m.wr32(r.r[A1], PARTY_BASE_ID);
    });
    cpu.hook(RESOLVE, |r, _| {
        let k = r.r[A0].wrapping_sub(PARTY_BASE_ID + 1) as usize;
        r.r[V0] = PARTY.get(k).copied().unwrap_or(0);
    });
    Some(cpu)
}

#[test]
fn the_move_vm_overlay_escape_matches_retail() {
    differential!(prot(897, EXT));
    let ramps_retail = Rc::new(RefCell::new(Vec::new()));
    let rand = Rc::new(RefCell::new(0u16));
    let Some(mut cpu) = retail_cpu(ramps_retail.clone(), rand.clone()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4558_5432);
    let sin: Vec<i16> = (0..0x1000).map(|_| rng.next() as i16).collect();
    let cos: Vec<i16> = (0..0x1000).map(|_| rng.next() as i16).collect();
    for i in 0..0x1000u32 {
        cpu.mem.wr16(SIN + i * 2, sin[i as usize] as u16);
        cpu.mem.wr16(COS + i * 2, cos[i as usize] as u16);
    }
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    cpu.mem.wr32(PARTY_LIST_PTR, 0x8015_0000);
    let mut world = World::new();
    // (base, length) of every region a host answer comes from.
    let spans: [(u32, usize); 10] = [
        (SLOTS, 0x100 * 8),
        (PREDICATE, 4),
        (ROWS, 0x20 * 0x14),
        (SCRATCH_SLOTS, 0x20),
        (FRAME_STEP, 1),
        (GLOBAL_B9D8, 4),
        (FADE_A, 3),
        (FADE_B, 3),
        (PLAYER + 0x14, 6),
        (FOCUS_X, 0xC),
    ];
    let mut seen = [0u32; 0x3E];
    let mut taken = 0;
    for run in 0..80_000 {
        let sub = if rng.below(40) == 0 {
            0x3D + rng.below(0x100) as u16
        } else {
            rng.below(0x3D) as u16
        };
        // The instruction sits at the buffer's head, the record's PC on it.
        let mut buf: Vec<u16> = (0..BUF_WORDS).map(|_| rng.next() as u16).collect();
        buf[0] = 0x2F;
        buf[1] = sub;
        let small = |rng: &mut Rng, n: u32| rng.below(n) as u16;
        match sub {
            // Offsets into the buffer stay inside it.
            0x04 | 0x3A => buf[2] = small(&mut rng, 0x40),
            0x05 | 0x30 => buf[4] = small(&mut rng, 0x40),
            0x3B => {
                buf[2] = small(&mut rng, 5).wrapping_sub(1);
                buf[3] = small(&mut rng, 0x40);
            }
            0x1B => {
                buf[2] = small(&mut rng, 0x30);
                buf[3] = small(&mut rng, 0x30);
                buf[4] = small(&mut rng, 0x20).wrapping_sub(4);
            }
            0x1E => buf[2] = small(&mut rng, 0x40),
            // Flag indices inside the bank.
            0x13 | 0x14 | 0x1C | 0x1D => buf[2] = small(&mut rng, 0x1000),
            // Object-effect rows, slots and scratchpad slots inside their
            // tables.
            0x17..=0x1A => buf[2] = small(&mut rng, 0x20),
            0x25..=0x28 | 0x31 | 0x32 | 0x34 | 0x35 => buf[2] = small(&mut rng, 0x100),
            0x29 => {
                buf[2] = small(&mut rng, 0x10);
                if rng.below(8) == 0 {
                    buf[3] = 0x8000;
                }
                if rng.below(2) == 0 {
                    buf[4] = 0;
                }
            }
            0x3C => {
                for v in &mut buf[2..5] {
                    *v &= 0xFF;
                }
                if rng.below(2) == 0 {
                    buf[5] = 0;
                }
            }
            0x0A | 0x0B => buf[2] = small(&mut rng, 8),
            _ => {}
        }
        let mut rec: Vec<u8> = (0..RECORD).map(|_| rng.next() as u8).collect();
        rec[0x70..0x72].copy_from_slice(&0u16.to_le_bytes());
        rec[0x48..0x4C].copy_from_slice(&BUF.to_le_bytes());
        cpu.mem.load(ACTOR, &rec);
        for (i, v) in buf.iter().enumerate() {
            cpu.mem.wr16(BUF + i as u32 * 2, *v);
        }
        let mut regions = Vec::new();
        for (base, len) in spans {
            let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            cpu.mem.load(base, &bytes);
            regions.push(Region { base, bytes });
        }
        // The pool top near its empty `0x8E`, so both pool tests take sides.
        let top = 0x80 + rng.below(0x10) as u16;
        cpu.mem.wr16(POOL_TOP, top);
        regions.push(Region {
            base: POOL_TOP,
            bytes: top.to_le_bytes().to_vec(),
        });
        let mut party = [[0i16; 3]; 3];
        for (k, a) in PARTY.iter().enumerate() {
            for (j, o) in [0x14u32, 0x16, 0x18].iter().enumerate() {
                party[k][j] = rng.next() as i16;
                cpu.mem.wr16(a + o, party[k][j] as u16);
            }
        }
        let bank: Vec<u8> = (0..FLAG_BANK_BYTES).map(|_| rng.next() as u8).collect();
        cpu.mem.load(FLAG_BANK, &bank);
        world.flags.system_flags = bank;
        *rand.borrow_mut() = rng.below(0x8000) as u16;
        ramps_retail.borrow_mut().clear();

        cpu.regs.r[V0] = 0xDEAD_BEEF;
        let stop = cpu.call(EXT, &[ACTOR, BUF]);
        let label = format!("run {run}: sub {sub:#04x} {:04X?}", &buf[..12]);
        assert_eq!(stop, Stop::Returned, "{label}");
        let retail_size = cpu.regs.r[V0] as u16 as i16;

        let mut host = Host {
            regions,
            world: std::mem::replace(&mut world, World::new()),
            sin: sin.clone(),
            cos: cos.clone(),
            rand: *rand.borrow(),
            ramps: Vec::new(),
            party,
            buf: buf.clone(),
        };
        let mut state = from_record(&rec);
        let port_size = host.ext_dispatch(&mut state, sub, &buf).size_u16;
        assert_eq!(retail_size, port_size, "{label}: size");
        let mut port = rec.clone();
        to_record(&state, &mut port);
        let mut retail = cpu.mem.read(ACTOR, RECORD);
        if matches!(sub, 0x0E | 0x12) {
            // The curve's point is copied out as a whole SVECTOR
            // (`0x801D39C4..0x801D39D0`, `0x801D3B44..0x801D3B50`), and its
            // pad halfword is stack the arm never wrote: `+0x1A` takes
            // whatever was there.
            retail[0x1A..0x1C].copy_from_slice(&port[0x1A..0x1C]);
        }
        assert!(
            retail == port,
            "{label}: {:?}",
            diff_offsets(&retail, &port)
        );
        let retail_buf: Vec<u16> = (0..BUF_WORDS as u32)
            .map(|i| cpu.mem.rd16(BUF + i * 2))
            .collect();
        assert_eq!(retail_buf, host.buf, "{label}: move buffer");
        for r in &host.regions {
            let got = cpu.mem.read(r.base, r.bytes.len());
            assert!(
                got == r.bytes,
                "{label}: {:#x}: {:?}",
                r.base,
                diff_offsets(&got, &r.bytes)
            );
        }
        assert!(
            cpu.mem.read(FLAG_BANK, FLAG_BANK_BYTES) == host.world.flags.system_flags,
            "{label}: flag bank"
        );
        assert_eq!(*ramps_retail.borrow(), host.ramps, "{label}: ramps");
        world = host.world;
        if usize::from(sub) < seen.len() {
            seen[usize::from(sub)] += 1;
        } else {
            seen[0x3D] += 1;
        }
        taken += u32::from(retail_size != 0 && !(2..=0x10).contains(&retail_size));
    }
    let thin: Vec<usize> = (0..seen.len()).filter(|&s| seen[s] < 500).collect();
    assert!(thin.is_empty() && taken > 1000, "{thin:?} {taken}");
}
