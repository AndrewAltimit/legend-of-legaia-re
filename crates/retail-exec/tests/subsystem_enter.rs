//! The field overlay's subsystem actor installer `FUN_801F1278` (PROT 0897)
//! against the kernels of `legaia_engine_field::field_subsystem_enter`.
//!
//! The port is a set of slices (the context writes, the member re-resolve,
//! the handler install, the pager rewind), so the test runs the whole routine
//! with its two callees hooked and composes the slices in the routine's
//! order. The refresh hook checks the state the routine brackets it with.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_subsystem_enter::{
    CTX_SUBMODE_BUSY, MaskedBytes, PAD_LATCH_BIT, enter_context, installer_entry,
    pending_is_consumed, rearm_state, resolve_current_member,
};
use legaia_engine_field::submode_hub_actors::OP49_SUBOP_SLOTS;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const ENTER: u32 = 0x801F_1278;
const SUSPEND: u32 = 0x801D_E190;
const REFRESH: u32 = 0x801D_9E1C;

const PAD_LATCH: u32 = 0x1F80_0394;
const PLAYER_PTR: u32 = 0x8007_C364;
const LEADER: u32 = 0x8007_B8F8;
const CLIP_BASE: u32 = 0x8007_BDD8;
const GRID_PTR: u32 = 0x801C_6EA4;
const WORD_BB74: u32 = 0x8007_BB74;
const MEMBER: u32 = 0x8007_B469;
const ROSTER_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const OPERAND_PTR: u32 = 0x8007_B450;
const PAGER_STATE: u32 = 0x801F_2734;

const ACTOR: u32 = 0x8012_0000;
const GRID: u32 = 0x8012_1000;
const OPERAND: u32 = 0x8012_2000;
const PLAYER: u32 = 0x8012_3000;

#[test]
fn the_subsystem_installer_matches_retail() {
    differential!(prot(897, ENTER));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    // What the refresh sees: its arguments, the masked pair, the player's
    // flag word and the pad latch.
    type Seen = (u32, u32, (u8, u8), u32, u32);
    let log = Rc::new(RefCell::new(Vec::<Option<Seen>>::new()));
    let l = log.clone();
    cpu.hook(SUSPEND, move |_, _| l.borrow_mut().push(None));
    let l = log.clone();
    cpu.hook(REFRESH, move |r, m| {
        let p = r.r[A0];
        l.borrow_mut().push(Some((
            p,
            r.r[A1],
            (m.rd8(p + 0x8E), m.rd8(p + 0x8F)),
            m.rd32(p + 0x10),
            m.rd32(PAD_LATCH),
        )));
    });
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    cpu.mem.wr32(GRID_PTR, GRID);

    let mut rng = Rng(0x454E_5452);
    let (mut remapped, mut kept, mut consumed, mut rewound, mut fell_back) = (0, 0, 0, 0, 0);
    let mut sizes = [0u32; 6];
    for round in 0..30000 {
        let handler = rng.below(0x34) as u16;
        let flags = rng.next();
        let latch = rng.next();
        let masked = (rng.below(256) as u8, rng.below(256) as u8);
        let leader = match rng.below(4) {
            0 => rng.next() as u16,
            _ => rng.below(3) as u16,
        };
        // The scan needs a roster of at least one: with none the routine
        // stores the stale byte of the first roster cell, which the kernel is
        // not handed.
        let count = 1 + rng.below(5) as usize;
        let roster: Vec<u8> = (0..count).map(|_| rng.below(6) as u8).collect();
        let member = rng.below(7) as u8;
        let sub_op = rng.below(OP49_SUBOP_SLOTS.len() as u32) as u8;
        let pending = match rng.below(4) {
            0 => 0,
            1 => 1,
            _ => OPERAND,
        };
        let pager = match rng.below(3) {
            0 => 4,
            1 => 7,
            _ => rng.below(10),
        };

        for o in (0..0x60).step_by(2) {
            cpu.mem.wr16(GRID + o, 0xA5A5);
        }
        cpu.mem.wr16(ACTOR + 0x1A, 0xA5A5);
        cpu.mem.wr16(ACTOR + 0x50, handler);
        cpu.mem.wr16(ACTOR + 0x54, 0xA5A5);
        cpu.mem.wr32(PLAYER + 0x10, flags);
        cpu.mem.wr8(PLAYER + 0x8E, masked.0);
        cpu.mem.wr8(PLAYER + 0x8F, masked.1);
        cpu.mem.wr16(PLAYER + 0x5C, 0xA5A5);
        cpu.mem.wr32(PAD_LATCH, latch);
        cpu.mem.wr16(LEADER, leader);
        cpu.mem.wr32(CLIP_BASE, 0x5A5A_5A5A);
        cpu.mem.wr32(WORD_BB74, 0x5A5A_5A5A);
        cpu.mem.wr8(MEMBER, member);
        cpu.mem.wr8(ROSTER_COUNT, count as u8);
        for i in 0..6 {
            cpu.mem
                .wr8(ROSTER + i, roster.get(i as usize).copied().unwrap_or(0xEE));
        }
        cpu.mem.wr32(OPERAND_PTR, pending);
        cpu.mem.wr8(OPERAND, sub_op);
        cpu.mem.wr32(PAGER_STATE, pager);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(ENTER, &[ACTOR]), Stop::Returned, "round {round}");

        let ctx = format!(
            "round {round} handler {handler:#x} roster {roster:?} member {member} \
             sub-op {sub_op} pending {pending:#x} pager {pager}"
        );
        let c = enter_context(flags, latch, leader);
        let (mask, during) = MaskedBytes::mask(masked);
        assert_eq!(
            *log.borrow(),
            [
                None,
                Some((
                    PLAYER,
                    0,
                    during,
                    flags | CTX_SUBMODE_BUSY,
                    latch & !PAD_LATCH_BIT
                ))
            ],
            "calls: {ctx}"
        );
        assert_eq!(cpu.mem.rd32(PLAYER + 0x10), c.flags, "flags: {ctx}");
        assert_eq!(cpu.mem.rd32(PAD_LATCH), c.pad_latch, "latch: {ctx}");
        assert_eq!(
            cpu.mem.rd32(CLIP_BASE),
            u32::from(c.clip_base),
            "base: {ctx}"
        );
        assert_eq!(
            cpu.mem.rd16(PLAYER + 0x5C) as i16,
            c.player_clip,
            "clip: {ctx}"
        );
        assert_eq!(
            (cpu.mem.rd8(PLAYER + 0x8E), cpu.mem.rd8(PLAYER + 0x8F)),
            mask.restore(),
            "masked pair: {ctx}"
        );
        assert_eq!(cpu.mem.rd32(WORD_BB74), 0, "{ctx}");
        // Only the first three roster cells are the party's, but the scan
        // runs to the count.
        let want_member = resolve_current_member(member, &roster);
        assert_eq!(cpu.mem.rd8(MEMBER), want_member, "member: {ctx}");
        fell_back += u32::from(want_member != member);

        // The operand word: `1` is consumed, a pointer is a parked op `0x49`.
        let (want_pending, live) = if pending_is_consumed(pending) {
            (0, false)
        } else {
            (pending, pending != 0)
        };
        consumed += u32::from(pending == 1);
        assert_eq!(cpu.mem.rd32(OPERAND_PTR), want_pending, "operand: {ctx}");
        let want_pager = if live { rearm_state(pager) } else { pager };
        assert_eq!(cpu.mem.rd32(PAGER_STATE), want_pager, "pager: {ctx}");
        rewound += u32::from(want_pager != pager);
        let remap = live.then(|| OP49_SUBOP_SLOTS[usize::from(sub_op)]);
        let cells: Vec<u8> = roster.clone();
        let e = installer_entry(handler, &cells, remap);
        remapped += u32::from(e.handler != 7);
        kept += u32::from(live && e.handler == 7);
        sizes[count] += 1;

        let mut grid = [0xA5A5u16; 0x30];
        grid[0x3E / 2] = e.completion_gate;
        grid[0x2E / 2] = e.selection as u16;
        grid[0x40 / 2] = e.saved_handler;
        grid[0x46 / 2] = e.cursor.0;
        grid[0x48 / 2] = e.cursor.1;
        grid[0x36 / 2] = e.cells[0];
        grid[0x38 / 2] = e.cells[1];
        grid[0x3A / 2] = e.cells[2];
        for (i, w) in grid.iter().enumerate() {
            let o = 2 * i as u32;
            assert_eq!(cpu.mem.rd16(GRID + o), *w, "grid +{o:#x}: {ctx}");
        }
        assert_eq!(cpu.mem.rd16(ACTOR + 0x50), e.handler, "handler: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), e.sub_state, "sub: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x1A), e.yield_marker, "yield: {ctx}");
    }
    assert!(
        remapped > 3000
            && kept > 1000
            && consumed > 5000
            && rewound > 5000
            && fell_back > 3000
            && sizes[1..].iter().all(|&n| n > 3000),
        "{remapped} {kept} {consumed} {rewound} {fell_back} {sizes:?}"
    );
}
