//! The art-name banner placement `FUN_8004C650` against
//! `battle_name_banner::{find_arts_record, banner_x}`: generated arts-name
//! tables (records, sentinel, misses), names leading with the nudge marker
//! `0xCF`, the character-name marker `0xC1` or neither, and every roster
//! slot. The measurer `FUN_80035F04` is hooked to the port's own
//! `Font::measure` (which `text_width` holds to it), so both sides place the
//! same widths; what is compared is the record walk, the four X fields and
//! the two name-pointer fields the routine writes into the placement table.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_ui::battle_name_banner::{
    ARTS_RECORD_STRIDE, ARTS_TABLE_SENTINEL, BANNER_X_FIELD_OFFSETS, CHARACTER_NAME_OFFSET,
    CHARACTER_RECORD_STRIDE, MARKER_CHAR_NAME_PREFIX, MARKER_NUDGE, banner_x, find_arts_record,
};
use legaia_font::{Font, MeasureOptions};
use psx_retail_exec::{A0, Memory, Stop, V0};

const BANNER: u32 = 0x8004_C650;
const MEASURE: u32 = 0x8003_5F04;
const TABLE: u32 = 0x8007_5EC4;
const PLACEMENT: u32 = 0x8007_6C10;
const RECORDS: u32 = 0x8008_4708;
const NAMES: u32 = 0x8013_0000;

fn cstr(m: &mut Memory, mut a: u32) -> Vec<u8> {
    let mut s = Vec::new();
    loop {
        let b = m.rd8(a);
        if b == 0 {
            return s;
        }
        s.push(b);
        a += 1;
    }
}

fn word(rng: &mut Rng) -> Vec<u8> {
    (0..1 + rng.below(14))
        .map(|_| b"abcdefghijklmnopqrstuvwxyz ABCDEFG-"[rng.below(35) as usize])
        .collect()
}

#[test]
fn the_banner_is_placed_where_retail_places_it() {
    differential!(scus(BANNER));
    let (Some(mut cpu), Some(d), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let tim = d
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let font = std::rc::Rc::new(Font::from_disc_tim_and_scus(&tim, &scus).expect("font"));
    let width = |font: &Font, s: &[u8]| font.measure(s, &MeasureOptions::default()).max_px as i32;
    let f = font.clone();
    cpu.hook(MEASURE, move |r, m| {
        let s = cstr(m, r.r[A0]);
        r.r[V0] = width(&f, &s) as u32;
    });
    let mut rng = Rng(0x4241_4E52);
    let (mut found, mut nudged, mut prefixed) = (0, 0, 0);
    for run in 0..10_000 {
        // A table of up to twelve records, then the sentinel.
        let n = rng.below(13) as usize;
        let mut table = vec![0u8; (n + 1) * ARTS_RECORD_STRIDE];
        let mut names = Vec::new();
        let mut at = NAMES;
        for i in 0..n {
            let rec = &mut table[i * ARTS_RECORD_STRIDE..(i + 1) * ARTS_RECORD_STRIDE];
            rec[0] = [0, 1, 2, rng.below(4) as u8, 0x63 ^ 1][rng.below(5) as usize];
            rec[1] = rng.below(8) as u8;
            let mut name = match rng.below(4) {
                0 => vec![MARKER_NUDGE, 1 + rng.below(8) as u8],
                1 => vec![MARKER_CHAR_NAME_PREFIX],
                _ => Vec::new(),
            };
            name.extend(word(&mut rng));
            rec[0xC..0x10].copy_from_slice(&at.to_le_bytes());
            let mut z = name.clone();
            z.push(0);
            cpu.mem.load(at, &z);
            at += z.len() as u32 + 1;
            names.push(name);
        }
        table[n * ARTS_RECORD_STRIDE] = ARTS_TABLE_SENTINEL;
        cpu.mem.load(TABLE, &table);
        let slot = rng.below(4);
        let char_name = word(&mut rng);
        let mut z = char_name.clone();
        z.push(0);
        cpu.mem.load(
            RECORDS + slot * CHARACTER_RECORD_STRIDE as u32 + CHARACTER_NAME_OFFSET as u32,
            &z,
        );
        let art = rng.below(8);
        let seed = rng.next() as u16;
        for off in BANNER_X_FIELD_OFFSETS {
            cpu.mem.wr16(PLACEMENT + off as u32, seed);
        }
        cpu.mem.wr32(PLACEMENT + 0x734, 0xAAAA_AAAA);
        cpu.mem.wr32(PLACEMENT + 0x74C, 0xAAAA_AAAA);
        assert_eq!(cpu.call(BANNER, &[slot, art]), Stop::Returned, "run {run}");
        let what = format!("run {run} slot {slot} art {art} n {n}");
        let xs: Vec<u16> = BANNER_X_FIELD_OFFSETS
            .iter()
            .map(|&o| cpu.mem.rd16(PLACEMENT + o as u32))
            .collect();
        let ptrs = [
            cpu.mem.rd32(PLACEMENT + 0x734),
            cpu.mem.rd32(PLACEMENT + 0x74C),
        ];
        match find_arts_record(&table, slot as u8, art as u8) {
            Some(i) => {
                let name = &names[i];
                let x = banner_x(width(&font, name), name[0], width(&font, &char_name));
                assert_eq!(xs, vec![x as u16; 4], "{what}: {name:02x?}");
                let va = u32::from_le_bytes(
                    table[i * ARTS_RECORD_STRIDE + 0xC..i * ARTS_RECORD_STRIDE + 0x10]
                        .try_into()
                        .unwrap(),
                );
                assert_eq!(ptrs, [va, va], "{what}");
                found += 1;
                nudged += u32::from(name[0] == MARKER_NUDGE);
                prefixed += u32::from(name[0] == MARKER_CHAR_NAME_PREFIX);
            }
            None => {
                assert_eq!(xs, vec![seed; 4], "{what}: untouched");
                assert_eq!(ptrs, [0xAAAA_AAAA; 2], "{what}: untouched");
            }
        }
    }
    assert!(
        found > 1000 && nudged > 250 && prefixed > 250,
        "{found} {nudged} {prefixed}"
    );
}
