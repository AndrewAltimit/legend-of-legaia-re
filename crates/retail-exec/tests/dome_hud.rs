//! The Muscle Dome hub screens' sprite path (PROT 0977) against
//! `engine-ui::muscle_dome_hud`: the centred and corner emitters
//! `FUN_801D050C` / `FUN_801D08EC` over generated records and arguments, the
//! decimal readout `FUN_801D1308`, the ROUND banner `FUN_801D02F0` with its
//! digit emitter `FUN_801D15C8`, and the course card `FUN_801D042C`.
//!
//! Every `POLY_GT4` retail writes is decoded and held to the port's
//! `HudQuad`s, and so is what each call leaves in the shared sprite table:
//! the emitters write their variant back into the record, and the next call
//! reads it.

use crate::common::gt4::{self, Gt4};
use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_ui::muscle_dome_hud::{
    HUD_SPRITE_STRIDE, HudQuad, HudSprite, SPRITE_TABLE_LEN, SPRITE_TABLE_VA, course_card_draws,
    decimal_quads, hub_screen_quads, hud_quad_centred, hud_quad_corner, hud_sel_split,
    round_banner_draws,
};
use psx_retail_exec::{Cpu, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const CENTRED: u32 = 0x801D_050C;
const CORNER: u32 = 0x801D_08EC;
const DECIMAL: u32 = 0x801D_1308;
const ROUND_BANNER: u32 = 0x801D_02F0;
const ROUND_DIGIT: u32 = 0x801D_15C8;
const COURSE_CARD: u32 = 0x801D_042C;
/// The hub's ordering-table bucket word `_DAT_801D1AA8`.
const BUCKET: u32 = 0x801D_1AA8;
const ROUND: u32 = 0x801D_1A94;
const CARD_LEVEL: u32 = 0x801D_1A84;
const COURSE: u32 = 0x801D_1A90;

fn rig() -> Option<Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(977).expect("PROT 0977"));
    Some(cpu)
}

fn table(cpu: &mut Cpu) -> Vec<HudSprite> {
    (0..SPRITE_TABLE_LEN as u32)
        .map(|i| {
            let rec = cpu.mem.read(
                SPRITE_TABLE_VA + i * HUD_SPRITE_STRIDE as u32,
                HUD_SPRITE_STRIDE,
            );
            HudSprite::parse(&rec).expect("record")
        })
        .collect()
}

fn quad(q: &HudQuad) -> Gt4 {
    Gt4 {
        code: 0x3C | if q.semi_transparent { 2 } else { 0 },
        rgb: q.rgb,
        xy: q.xy,
        uv: q.uv,
        clut: q.clut,
        tpage: q.tpage,
    }
}

fn call(cpu: &mut Cpu, entry: u32, args: &[u32], stack: &[u32]) {
    for (i, v) in stack.iter().enumerate() {
        cpu.mem.wr32(STACK_TOP + 0x10 + 4 * i as u32, *v);
    }
    gt4::reset(cpu, BUCKET);
    assert_eq!(
        cpu.call(entry, args),
        Stop::Returned,
        "{entry:#x} {args:x?}"
    );
}

/// The retail packets against the port's quads (`paint` = the port gave
/// them in paint order, the emit order reversed), then the two tables.
fn same(cpu: &mut Cpu, port: &[HudQuad], paint: bool, after: &[HudSprite], what: &str) {
    let retail = gt4::packets(cpu, BUCKET);
    let mut port: Vec<Gt4> = port.iter().map(quad).collect();
    if paint {
        port.reverse();
    }
    assert_eq!(retail, port, "{what}");
    assert_eq!(table(cpu), after, "{what}: the sprite table after");
}

#[test]
fn the_two_emitters_write_the_ports_quads() {
    differential!(prot(977, CENTRED), prot(977, CORNER));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x444F_4D45);
    let mut variants = 0;
    for run in 0..20_000 {
        let idx = rng.below(SPRITE_TABLE_LEN as u32);
        let mut rec = [0u8; HUD_SPRITE_STRIDE];
        for b in rec.iter_mut() {
            *b = rng.next() as u8;
        }
        let size: i32 = [
            0x1000,
            0x2000,
            rng.below(0x4000) as i32,
            -(rng.below(0x2000) as i32),
        ][rng.below(4) as usize];
        rec[..4].copy_from_slice(&size.to_le_bytes());
        rec[0xF] = rng.below(2) as u8;
        rec[0x13] = rng.below(4) as u8;
        cpu.mem
            .load(SPRITE_TABLE_VA + idx * HUD_SPRITE_STRIDE as u32, &rec);
        let mut port_table = table(&mut cpu);
        let variant = [0u32, 0, 1, 2, 3][rng.below(5) as usize];
        let sel = (variant << 10) | idx;
        let (x, y) = (
            rng.below(0x200) as i16 - 0x80,
            rng.below(0x200) as i16 - 0x80,
        );
        let brightness = [
            0x80,
            0xFF,
            rng.below(0x200) as i32,
            -(rng.below(0x40) as i32),
        ][rng.below(4) as usize];
        let scale = [
            0x1000,
            rng.below(0x3000) as i32,
            -(rng.below(0x1000) as i32),
        ][rng.below(3) as usize];
        let corner = rng.below(2) == 1;
        let entry = if corner { CORNER } else { CENTRED };
        call(
            &mut cpu,
            entry,
            &[x as u16 as u32, y as u16 as u32, sel, brightness as u32],
            &[scale as u32],
        );
        let (i, v) = hud_sel_split(sel as i32);
        let rec = &mut port_table[i];
        let q = if corner {
            hud_quad_corner(rec, x, y, v, brightness, scale)
        } else {
            hud_quad_centred(rec, x, y, v, brightness, scale)
        };
        same(
            &mut cpu,
            &[q],
            false,
            &port_table,
            &format!("run {run} {entry:#x} sel {sel:#x}"),
        );
        variants += u32::from(v != 0);
    }
    assert!(variants > 10_000, "{variants}");
}

#[test]
fn the_decimal_readout_matches() {
    differential!(prot(977, DECIMAL));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4445_4349);
    for run in 0..10_000 {
        let mut port_table = table(&mut cpu);
        let value = match rng.below(4) {
            0 => rng.below(10) as i32,
            1 => rng.below(1_000_000) as i32,
            2 => -(rng.below(1000) as i32),
            _ => [0, 99_999_999, 123_456_789][rng.below(3) as usize],
        };
        let (x, y) = (rng.below(0x140) as i16, rng.below(0xF0) as i16);
        let brightness = rng.below(0x100) as i32;
        let palette = rng.below(4) as i16;
        call(
            &mut cpu,
            DECIMAL,
            &[x as u32, y as u32, value as u32, brightness as u32],
            &[palette as u32],
        );
        let q = decimal_quads(&mut port_table[9], x, y, value, brightness, palette);
        same(
            &mut cpu,
            &q,
            false,
            &port_table,
            &format!("run {run} {value}"),
        );
    }
}

#[test]
fn the_round_banner_and_the_course_card_match() {
    differential!(
        prot(977, ROUND_BANNER),
        prot(977, ROUND_DIGIT),
        prot(977, COURSE_CARD)
    );
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x524F_554E);
    for run in 0..4_000 {
        let brightness = rng.below(0x100) as i32;
        // The banner: the displayed round is the stored one plus one.
        let round = rng.below(99) as i32;
        cpu.mem.wr32(ROUND, (round - 1) as u32);
        let mut port_table = table(&mut cpu);
        call(&mut cpu, ROUND_BANNER, &[brightness as u32], &[]);
        let q = hub_screen_quads(&mut port_table, &round_banner_draws(round), brightness);
        same(
            &mut cpu,
            &q,
            true,
            &port_table,
            &format!("run {run} round {round}"),
        );
        // The digit emitter on its own.
        let digit = rng.below(10);
        let (x, y) = (rng.below(0x140), rng.below(0xF0));
        let variant = 1 + rng.below(2);
        let mut port_table = table(&mut cpu);
        call(
            &mut cpu,
            ROUND_DIGIT,
            &[x, y, (variant << 10) | digit, brightness as u32],
            &[0x1000],
        );
        let draw = legaia_engine_ui::muscle_dome_hud::HubDraw {
            sel: (variant << 10) as i32,
            x: x as i16,
            y: y as i16,
            anchor: legaia_engine_ui::muscle_dome_hud::HubAnchor::RoundDigit(digit as u8),
            scale: 0x1000,
            call_site: 0,
        };
        let q = hub_screen_quads(&mut port_table, &[draw], brightness);
        same(
            &mut cpu,
            &q,
            true,
            &port_table,
            &format!("run {run} digit {digit}"),
        );
        // The card: course and level come off the hub's words.
        let course = rng.below(3) as i32;
        cpu.mem.wr32(COURSE, course as u32);
        cpu.mem.wr32(CARD_LEVEL, brightness as u32);
        let mut port_table = table(&mut cpu);
        call(&mut cpu, COURSE_CARD, &[], &[]);
        let q = hub_screen_quads(&mut port_table, &course_card_draws(course), brightness);
        same(
            &mut cpu,
            &q,
            true,
            &port_table,
            &format!("run {run} course {course}"),
        );
    }
}
