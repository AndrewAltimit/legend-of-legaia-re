//! The move VM `FUN_80023070`, run on an actor record of random bytes over short
//! random programs, against `legaia_engine_vm::move_vm::actor_tick` on the same
//! record read into an `ActorState`. Every call the VM makes is stubbed on the
//! retail side and a no-op on the port's (the host arms that write memory -
//! op `0x1D`'s global, op `0x21`'s deformation row, op `0x03`'s sine tables -
//! are answered from the same RAM), so what is compared is the whole record
//! the VM owns, `+0x00..+0x1AC`, byte for byte: a write the port's state does
//! not carry shows as a byte the port left at its seed.
//!
//! Each program ends on a HALT or a WAIT, the only ops that end the frame.
//! Between its ops sit op `0x0B` and words past `0x46`, which retail's loop
//! skips by the previous op's width. Op `0x2F` (the overlay escape) is left
//! out.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_vm::move_vm::{ActorState, ActorTickOutcome, MoveHost, actor_tick};
use psx_retail_exec::Stop;

const VM: u32 = 0x8002_3070;
const VM_END: u32 = 0x8002_4190;
const ACTOR: u32 = 0x8013_0000;
const RECORD: usize = 0x1AC;
const BUF: u32 = 0x8012_0000;
const BUF_WORDS: usize = 0x400;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN: u32 = 0x8014_0000;
const COS: u32 = 0x8014_2000;
const GLOBAL_1D: u32 = 0x8007_B6DE;
const FACE_ROWS: u32 = 0x8007_BE60;
const CURVE_MUL: u32 = 0x1F80_037D;
const MODEL: u32 = 0x8015_0000;
const HEAP: u32 = 0x8015_1000;
/// The move table op `0x25` reads its child's record offset from.
const MOVE_TABLE_PTR: u32 = 0x8007_B8D0;
const MOVE_TABLE: u32 = 0x8016_0000;

/// The keyframe-pose block behind `+0x4C`: an `8 + parts * 8` head, then one
/// `0x18`-byte entry per part, current keyframe then target.
fn pose_at(parts: usize) -> u32 {
    HEAP + 8 + parts as u32 * 8
}

struct Host {
    sin: Vec<i16>,
    cos: Vec<i16>,
    curve: u8,
    global_1d: Option<u16>,
    faces: Vec<(u8, [u16; 4], i32)>,
}

impl MoveHost for Host {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let i = usize::from(index & 0xFFF);
        (self.sin[i], self.cos[i])
    }
    fn global_write_1d(&mut self, value: u16) {
        self.global_1d = Some(value);
    }
    fn face_rotation_setup(&mut self, face_id: u8, params: [u16; 4], target: i32) {
        self.faces.push((face_id, params, target));
    }
    fn keyframe_curve_multiplier(&self) -> u8 {
        self.curve
    }
}

fn h(r: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([r[o], r[o + 1]])
}
fn w(r: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]])
}
fn put16(r: &mut [u8], o: usize, v: u16) {
    r[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(r: &mut [u8], o: usize, v: u32) {
    r[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// The halfword fields of `ActorState` by record offset.
macro_rules! halves {
    ($m:ident) => {
        $m!(0x14, world_x, i16);
        $m!(0x16, world_y, i16);
        $m!(0x18, world_z, i16);
        $m!(0x22, y_rot, i16);
        $m!(0x24, render_24, i16);
        $m!(0x26, render_26, i16);
        $m!(0x28, render_28, i16);
        $m!(0x2A, world_y_mirror, i16);
        $m!(0x3C, anim_3c, i16);
        $m!(0x3E, anim_3e, i16);
        $m!(0x40, anim_40, i16);
        $m!(0x42, field_42, u16);
        $m!(0x50, field_50, u16);
        $m!(0x52, field_52, u16);
        $m!(0x54, wait_timer, i16);
        $m!(0x56, move_substate, i16);
        $m!(0x5A, move_submode, i16);
        $m!(0x5C, field_5c, i16);
        $m!(0x62, local_flags, u16);
        $m!(0x68, field_68, i16);
        $m!(0x6A, field_6a, i16);
        $m!(0x70, pc, i16);
        $m!(0x72, field_72, u16);
        $m!(0x78, field_78, u16);
        $m!(0x7A, field_7a, u16);
        $m!(0x80, anim_80, i16);
        $m!(0x82, anim_82, i16);
        $m!(0x84, anim_84, i16);
        $m!(0x86, field_86, u16);
        $m!(0x88, field_88, u16);
        $m!(0x8A, field_8a, u16);
        $m!(0x8C, field_8c, u16);
        $m!(0x8E, field_8e, u16);
        $m!(0x90, tween_src_x, i16);
        $m!(0x92, tween_src_y, i16);
        $m!(0x94, tween_src_z, i16);
        $m!(0x96, tween_scale_x, i16);
        $m!(0x98, tween_scale_y, i16);
        $m!(0x9A, tween_scale_z, i16);
    };
}

fn from_record(r: &[u8]) -> ActorState {
    let mut s = ActorState::new();
    macro_rules! rd {
        ($o:expr, $f:ident, $t:ty) => {
            s.$f = h(r, $o) as $t;
        };
    }
    halves!(rd);
    s.flags = w(r, 0x10);
    s.keyframe_count = r[0x6C];
    s.face_rotation = r[0x6D];
    s.field_74 = w(r, 0x74);
    s.field_7c = w(r, 0x7C);
    for o in (0x9C..RECORD).step_by(2) {
        s.set_actor_u16(o, h(r, o));
    }
    s
}

fn to_record(s: &ActorState, r: &mut [u8]) {
    macro_rules! wr {
        ($o:expr, $f:ident, $t:ty) => {
            put16(r, $o, s.$f as u16);
        };
    }
    halves!(wr);
    put32(r, 0x10, s.flags);
    r[0x6C] = s.keyframe_count;
    r[0x6D] = s.face_rotation;
    put32(r, 0x74, s.field_74);
    put32(r, 0x7C, s.field_7c);
    for o in (0x9C..RECORD).step_by(2) {
        put16(r, o, s.actor_u16(o));
    }
}

/// The words an op takes, for a program's layout, from the port's own step on
/// a scratch actor (a loop op and HALT take one). A wrong width only changes
/// which op comes next, and the run then shows it.
fn width(op: u16, operands: &[u16]) -> usize {
    struct Null;
    impl MoveHost for Null {}
    if matches!(op, 0x08 | 0x19 | 0x1B) {
        return 1;
    }
    let mut code = vec![op];
    code.extend_from_slice(operands);
    let mut s = ActorState::new();
    legaia_engine_vm::move_vm::step(&mut Null, &mut s, &code);
    (s.pc as usize).max(1)
}

/// Whether a program seats the pose (op `0x3C`), walking it by widths.
fn seen_3c(prog: &[u16]) -> bool {
    let mut i = 0;
    while i < prog.len() {
        if prog[i] == 0x3C {
            return true;
        }
        i += width(prog[i], &prog[i + 1..]);
    }
    false
}

#[test]
fn the_move_vm_writes_what_retail_writes() {
    differential!(scus(VM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((VM, VM_END));
    cpu.step_budget = 200_000;
    let mut rng = Rng(0x4D4F_5645);
    let sin: Vec<i16> = (0..0x1000).map(|_| rng.next() as i16).collect();
    let cos: Vec<i16> = (0..0x1000).map(|_| rng.next() as i16).collect();
    for i in 0..0x1000u32 {
        cpu.mem.wr16(SIN + i * 2, sin[i as usize] as u16);
        cpu.mem.wr16(COS + i * 2, cos[i as usize] as u16);
    }
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    cpu.mem.wr32(MOVE_TABLE_PTR, MOVE_TABLE);
    let ops: Vec<u16> = (0..=0x46u16).filter(|&o| o != 0x0B && o != 0x2F).collect();
    let mut seen = [0u32; 0x47];
    let (mut spins, mut skips) = (0, 0);
    for run in 0..60_000 {
        // The record, with its PC near the buffer's head and the loop
        // registers pointing into the program.
        let mut rec: Vec<u8> = (0..RECORD).map(|_| rng.next() as u8).collect();
        let pc0 = rng.below(8) as u16;
        put16(&mut rec, 0x70, pc0);
        put16(&mut rec, 0x54, 0x8000 | rng.next() as u16);
        put32(&mut rec, 0x48, BUF);
        put32(&mut rec, 0x44, MODEL);
        put32(&mut rec, 0x4C, HEAP);
        // The loop ops jump back to the program's head; a HALT bit left up
        // would read as this frame's.
        put16(&mut rec, 0x88, pc0.wrapping_sub(2));
        put16(&mut rec, 0x8A, pc0.wrapping_sub(2));
        for o in [0x8C, 0x8E] {
            let n = match rng.below(4) {
                // `0x4000` loops for ever; past `40001` the decrement retires
                // at once (`0x800235F8`); a small count loops that often.
                0 => 0x4000 | rng.next() as u16,
                1 => 40_002 + rng.below(0x10000 - 40_002) as u16,
                _ => rng.below(3) as u16,
            };
            put16(&mut rec, o, n);
        }
        let f = w(&rec, 0x10) & !8;
        put32(&mut rec, 0x10, f);
        // The program: a few ops with random operands, then the end.
        let mut buf = vec![0xFFFFu16; BUF_WORDS];
        let mut at = usize::from(pc0);
        // Ops `0x3C` / `0x3D` find the block's entries past a head sized by
        // their own count operand, so a program keeps one part count.
        let mut parts = 3usize;
        let pose0: Vec<[i16; 12]> = (0..parts)
            .map(|_| std::array::from_fn(|_| rng.next() as i16))
            .collect();
        for (t, kf) in pose0.iter().enumerate() {
            for (k, v) in kf.iter().enumerate() {
                cpu.mem
                    .wr16(pose_at(parts) + t as u32 * 0x18 + k as u32 * 2, *v as u16);
            }
        }
        cpu.mem.wr32(MODEL, parts as u32);
        let mut last = 0;
        for k in 0..1 + rng.below(4) {
            if k > 0 && rng.below(4) == 0 {
                // Skipped by the last op's width: the words under it go unread.
                buf[at] = [0x0B, 0x47, 0xFFFF, rng.next() as u16 | 0x47][rng.below(4) as usize];
                at += last;
                skips += 1;
            }
            let op = ops[rng.below(ops.len() as u32) as usize];
            let mut operands: Vec<u16> = (0..0x30)
                .map(|_| match rng.below(4) {
                    0 => rng.below(8) as u16,
                    1 => rng.below(0x40) as u16,
                    _ => rng.next() as u16,
                })
                .collect();
            match op {
                // A row id past the table walks the deformation write over
                // the executable, and a child id past the move table reads
                // off the map; ids stay in their tables.
                0x21 | 0x25 => operands[0] &= 0xFF,
                // A lane count the operand window holds, or a negative one.
                0x0A => {
                    operands[1] = if rng.below(4) == 0 {
                        0x8000 | rng.next() as u16
                    } else {
                        rng.below(16) as u16
                    }
                }
                // A loop count that ends inside the instruction budget.
                0x18 | 0x1A => {
                    operands[0] = match rng.below(3) {
                        0 => 0x4000 | rng.next() as u16,
                        1 => 40_002 + rng.below(0x10000 - 40_002) as u16,
                        _ => rng.below(4) as u16,
                    }
                }
                0x3C => {
                    parts = rng.below(4) as usize;
                    operands[0] = parts as u16;
                }
                0x3D => operands[1] = parts as u16,
                _ => {}
            }
            let n = width(op, &operands);
            buf[at] = op;
            buf[at + 1..at + n].copy_from_slice(&operands[..n - 1]);
            at += n;
            seen[usize::from(op)] += 1;
            last = n;
        }
        // The frame's end.
        buf[at] = 0x08 + rng.below(2) as u16;
        buf[at + 1] = rng.next() as u16;
        at += 2;
        let curve = rng.next() as u8;
        cpu.mem.wr8(CURVE_MUL, curve);
        for (i, v) in buf.iter().enumerate() {
            cpu.mem.wr16(BUF + i as u32 * 2, *v);
        }
        cpu.mem.load(ACTOR, &rec);
        let g1d = rng.next() as u16;
        cpu.mem.wr16(GLOBAL_1D, g1d);
        let faces_before = cpu.mem.read(FACE_ROWS, 0x100 * 12);

        let stop = cpu.call(VM, &[ACTOR]);
        let mut host = Host {
            sin: sin.clone(),
            cos: cos.clone(),
            curve,
            global_1d: None,
            faces: Vec::new(),
        };
        let mut state = from_record(&rec);
        state.keyframe_pose = pose0;
        let outcome = actor_tick(&mut host, &mut state, &buf, 10_000);
        if matches!(stop, Stop::StepLimit { .. }) {
            let rpc = cpu.mem.rd16(ACTOR + 0x70);
            let rop = cpu.mem.rd16(BUF + 2 * u32::from(rpc));
            assert_eq!(
                outcome,
                ActorTickOutcome::BudgetExhausted,
                "run {run}: {stop:?} {:04X?} pc {rpc} op {rop:04X}",
                &buf[..at]
            );
            spins += 1;
            continue;
        }
        assert_eq!(
            stop,
            Stop::Returned,
            "run {run}: {:04X?}",
            &buf[usize::from(pc0)..at]
        );
        let label = format!("run {run}: {:04X?}", &buf[usize::from(pc0)..at]);
        assert!(
            !matches!(
                outcome,
                ActorTickOutcome::BudgetExhausted | ActorTickOutcome::Waiting
            ),
            "{label}: {outcome:?}"
        );
        let mut port = rec.clone();
        to_record(&state, &mut port);
        let retail = cpu.mem.read(ACTOR, RECORD);
        assert!(
            retail == port,
            "{label}: {:?}",
            diff_offsets(&retail, &port)
        );
        let held = cpu.mem.rd32(MODEL) as usize;
        if seen_3c(&buf[usize::from(pc0)..at]) {
            assert_eq!(held, state.keyframe_pose.len(), "{label}: part count");
        }
        for (t, kf) in state.keyframe_pose.iter().enumerate() {
            for (k, v) in kf.iter().enumerate() {
                let a = pose_at(held) + t as u32 * 0x18 + k as u32 * 2;
                assert_eq!(cpu.mem.rd16(a) as i16, *v, "{label}: part {t} word {k}");
            }
        }
        assert_eq!(
            cpu.mem.rd16(GLOBAL_1D),
            host.global_1d.unwrap_or(g1d),
            "{label}"
        );
        let mut faces = faces_before;
        for (id, p, t) in host.faces {
            let o = usize::from(id) * 12;
            for (k, v) in p.iter().enumerate() {
                faces[o + k * 2..o + k * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
            faces[o + 8..o + 12].copy_from_slice(&t.to_le_bytes());
        }
        assert!(
            cpu.mem.read(FACE_ROWS, 0x100 * 12) == faces,
            "{label}: deformation rows"
        );
    }
    let thin: Vec<String> = ops
        .iter()
        .filter(|&&o| seen[usize::from(o)] < 500)
        .map(|o| format!("{o:#04x}"))
        .collect();
    assert!(
        thin.is_empty() && spins < 2000 && skips > 10_000,
        "{thin:?} {spins} {skips}"
    );
}
