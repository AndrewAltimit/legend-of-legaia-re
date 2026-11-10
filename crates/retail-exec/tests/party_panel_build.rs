//! The battle-result build and panel publish `FUN_801D84C0` against the
//! port's pieces of it: `result_subject` and the pool strings
//! `VictoryText` / `DefeatText` read off the same PROT 0898 image, the
//! placement-table fields (`PUBLISHED_BUFFERS`, `NAME_POINTER_FIELDS`,
//! `PORTRAIT_CELL_FIELDS`, `ANCHOR_FIELDS` with `panel_anchors`), the party
//! reset `PARTY_RESET` and the label block's teardown `LabelState::torn_down`.
//!
//! The SCUS string helpers it calls (`FUN_8003CA78` copy, `FUN_8003CAC4`
//! append, `FUN_8003CBF8` escape locator) run live. Solo and roster parties
//! with every party-size byte, random names, and the whole placement table,
//! the context's four result buffers, the three party actors and the label
//! block are compared. The two escape messages have no port text; only the
//! subject that opens them is compared.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_party_panel::{
    ANCHOR_FIELDS, DefeatText, LabelState, NAME_ESCAPE, NAME_POINTER_FIELDS, PARTY_RESET,
    PORTRAIT_CELL_FIELDS, PUBLISHED_BUFFERS, ResultSubject, VictoryText, name_field_ptr,
    panel_anchors, portrait_cell, result_subject,
};
use psx_retail_exec::Stop;

const BUILD: u32 = 0x801D_84C0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x300;
const PARTY_IDS: u32 = 0x8007_BD10;
const PLACEMENTS: u32 = 0x8007_6C10;
const PLACEMENTS_LEN: usize = 0xA00;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const LABEL_BLOCK: u32 = 0x801F_4E08;

fn bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

#[test]
fn the_result_build_writes_what_the_port_says() {
    differential!(prot(898, BUILD));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &image);
    let victory = VictoryText::parse(&image).expect("victory text");
    let defeat = DefeatText::parse(&image).expect("defeat text");
    let sentence = bytes(&format!("{}|{}", victory.opening_tail, victory.report));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..3 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let mut rng = Rng(0x5041_4E4C);
    let mut seen = [0u32; 8];
    for n in 0..4_000 {
        let solo = rng.below(2) == 0;
        let lead = 1 + rng.below(3) as u8;
        let slots = [
            lead,
            if solo { 0 } else { 1 + rng.below(3) as u8 },
            if solo || rng.below(3) == 0 {
                0
            } else {
                1 + rng.below(3) as u8
            },
        ];
        let size = rng.below(5) as u8;
        let mut names = Vec::new();
        for id in 1..=3u8 {
            let len = 1 + rng.below(8) as usize;
            let name: Vec<u8> = (0..len).map(|_| 0x41 + rng.below(26) as u8).collect();
            let mut z = name.clone();
            z.push(0);
            cpu.mem.load(name_field_ptr(id), &z);
            names.push(name);
        }
        let ctx: Vec<u8> = (0..CTX_LEN).map(|_| rng.next() as u8).collect();
        let mut ctx = ctx;
        ctx[0] = size;
        cpu.mem.load(CTX, &ctx);
        let table: Vec<u8> = (0..PLACEMENTS_LEN).map(|_| rng.next() as u8).collect();
        cpu.mem.load(PLACEMENTS, &table);
        cpu.mem.load(PARTY_IDS, &slots);
        for s in 0..3 {
            cpu.mem.wr16(ACTOR + s * 0x400 + 0x1DD, rng.next() as u16);
        }
        let block: [u8; 8] = std::array::from_fn(|_| rng.next() as u8);
        cpu.mem.load(LABEL_BLOCK, &block);
        assert_eq!(cpu.call(BUILD, &[]), Stop::Returned, "n {n}");

        // The context's result buffers.
        let subject = result_subject(slots);
        let head = match subject {
            ResultSubject::Lead(id) => names[usize::from(id) - 1].clone(),
            ResultSubject::LeadsTeam { escape_operand } => vec![NAME_ESCAPE, escape_operand],
        };
        let mut want_ctx = ctx.clone();
        let mut put = |at: usize, s: &[u8]| {
            want_ctx[at..at + s.len()].copy_from_slice(s);
            want_ctx[at + s.len()] = 0;
        };
        let (vic, def) = match subject {
            ResultSubject::Lead(_) => (
                [head.clone(), sentence.clone()].concat(),
                [head.clone(), bytes(&defeat.solo_suffix)].concat(),
            ),
            ResultSubject::LeadsTeam { .. } => (
                [head.clone(), bytes(&victory.team_tail), sentence.clone()].concat(),
                [head.clone(), bytes(&defeat.team_tail)].concat(),
            ),
        };
        put(0xA9, &vic);
        put(0x129, &def);
        let got_ctx = cpu.mem.read(CTX, CTX_LEN);
        // The escape messages: the port carries their subject only.
        for at in [0x159, 0x189] {
            assert_eq!(&got_ctx[at..at + head.len()], &head[..], "n {n} {at:#x}");
            let from = at + head.len();
            let end = from + got_ctx[from..].iter().position(|&b| b == 0).expect("NUL");
            want_ctx[at..=end].copy_from_slice(&got_ctx[at..=end]);
        }
        assert!(
            got_ctx == want_ctx,
            "n {n} {slots:?}: ctx {:?}",
            diff_offsets(&got_ctx, &want_ctx)
        );

        // The placement table.
        let mut want = table.clone();
        let w32 =
            |t: &mut Vec<u8>, at: usize, v: u32| t[at..at + 4].copy_from_slice(&v.to_le_bytes());
        for (at, buf) in PUBLISHED_BUFFERS {
            w32(&mut want, at, CTX + buf as u32);
        }
        for (at, slot) in NAME_POINTER_FIELDS {
            w32(&mut want, at, name_field_ptr(slots[slot]));
        }
        for (at, slot) in PORTRAIT_CELL_FIELDS {
            want[at] = portrait_cell(slots[slot]);
        }
        if let Some((primary, secondary)) = panel_anchors(size) {
            for at in ANCHOR_FIELDS.0 {
                want[at..at + 2].copy_from_slice(&primary.to_le_bytes());
            }
            if let Some(x) = secondary {
                for at in ANCHOR_FIELDS.1 {
                    want[at..at + 2].copy_from_slice(&x.to_le_bytes());
                }
            }
        }
        let got = cpu.mem.read(PLACEMENTS, PLACEMENTS_LEN);
        assert!(
            got == want,
            "n {n} {slots:?} size {size}: placements {:?}",
            diff_offsets(&got, &want)
        );

        for s in 0..3 {
            assert_eq!(
                (
                    cpu.mem.rd8(ACTOR + s * 0x400 + 0x1DD),
                    cpu.mem.rd8(ACTOR + s * 0x400 + 0x1DE)
                ),
                (PARTY_RESET.target_slot, PARTY_RESET.category),
                "n {n}"
            );
        }
        let before = LabelState {
            kind: block[0],
            flags: block[1],
            phase: block[2],
            handle: u32::from_le_bytes([block[4], block[5], block[6], block[7]]),
        };
        let after = before.torn_down();
        assert_eq!(
            cpu.mem.read(LABEL_BLOCK, 8),
            [
                &[after.kind, after.flags, after.phase, block[3]][..],
                &after.handle.to_le_bytes()
            ]
            .concat(),
            "n {n}"
        );
        seen[usize::from(solo) * 4 + usize::from(size.min(3))] += 1;
    }
    assert!(seen.iter().all(|&c| c > 100), "{seen:?}");
}
