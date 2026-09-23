//! Disc-gated oracle for the auto-combo's art insertion tail
//! (`legaia_engine_vm::battle_arts_auto_combo::insert_arts`, `FUN_801F0450`
//! `0x801F0B4C..0x801F1274`) over the retail art-animation banks.
//!
//! The tail reads each record's combo string (`+0x00..+0x0A`) and the
//! zero-terminated run at `+0x0B` straight out of the player battle files'
//! bank (`record[0] +0x58`, `0xD0` stride). This drives it over every
//! Vahn / Noa / Gala bank with many seeds and checks what the disassembly
//! guarantees of every outcome:
//!
//! - each splice is a learned art (`index - 0xB` in the list) whose combo has
//!   at least two arrows, written as `arrow + 0xB` over the last `len` slots
//!   of the still-free region, and later splices land strictly in front of
//!   earlier ones;
//! - the queue stays a direction string (`0x0C..=0x0F`) throughout;
//! - the Spirit budget is a local copy: what is left is the gauge minus the
//!   summed costs, and it never pays for a cost it cannot cover;
//! - without the Miracle marker nothing is spliced on the first four passes,
//!   so no first splice is charged the first-pass rate.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use std::path::PathBuf;

use legaia_engine_vm::battle_arts_auto_combo::{
    ArtsTailInput, TAIL_COST_FIRST, TAIL_FIRST_ART, insert_arts,
};

fn extracted_root() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("SCUS_942.54").is_file() && p.join("PROT").is_dir())
}

const PLAYER_FILES: [(&str, u8, &str); 3] = [
    ("Vahn", 1, "0863_edstati3.BIN"),
    ("Noa", 2, "0864_edstati3.BIN"),
    ("Gala", 3, "0865_battle_data.BIN"),
];

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    Some(root)
}

/// The bank's raw records and its count byte, read the way the tail reads
/// them: the `+0x58` word locates `[u32 count][records]`.
fn bank(root: &std::path::Path, file: &str) -> (u8, Vec<[u8; 0xD0]>) {
    let raw = std::fs::read(root.join("PROT").join(file)).expect("read player file");
    let record0 =
        legaia_asset::battle_char_assembly::decode_record0(&raw).expect("decode record[0]");
    let off = u32::from_le_bytes(record0[0x58..0x5C].try_into().unwrap()) as usize;
    let count = record0[off];
    let recs = (0..usize::from(count))
        .map(|i| {
            let b = off + 4 + i * 0xD0;
            record0[b..b + 0xD0].try_into().unwrap()
        })
        .collect();
    (count, recs)
}

/// BIOS-shaped `rand()`: 15-bit non-negative draws from a seeded LCG.
fn lcg(seed: u32) -> impl FnMut() -> i32 {
    let mut s = seed;
    move || {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((s >> 16) & 0x7FFF) as i32
    }
}

#[test]
fn the_tail_splices_learned_combos_over_the_free_region() {
    let Some(root) = gate() else { return };
    let mut total_splices = 0usize;
    for (name, char_id, file) in PLAYER_FILES {
        let (count, recs) = bank(&root, file);
        assert!(usize::from(count) > usize::from(TAIL_FIRST_ART), "{name}");
        // Every art the bank names is learned.
        let learned: Vec<u8> = (0..count - TAIL_FIRST_ART).collect();
        let mut spliced = 0usize;
        for seed in 0..400u32 {
            for &(marker, spirit) in &[(true, 100u16), (false, 100), (true, 40)] {
                let mut fill = lcg(seed ^ 0xA5A5);
                let n = 6 + (seed as usize % 11);
                let mut queue: Vec<u8> = (0..n).map(|_| 0x0C + (fill() % 4) as u8).collect();
                let input = ArtsTailInput {
                    char_id,
                    spirit,
                    ability_high: 0,
                    learned: &learned,
                    miracle_marker: marker,
                    records: &recs,
                    art_count: count,
                };
                let out = insert_arts(&mut queue, &input, lcg(seed));
                assert!(
                    queue.iter().all(|b| (0x0C..=0x0F).contains(b)),
                    "{name} seed {seed}: queue left the direction alphabet: {queue:02X?}"
                );
                let spent: i32 = out.inserted.iter().map(|i| i.cost).sum();
                assert_eq!(out.budget_left, i32::from(spirit) - spent, "{name}");
                assert!(out.budget_left >= 0, "{name} seed {seed}: overspent");
                let mut front = n;
                for ins in &out.inserted {
                    let rec = &recs[usize::from(ins.art)];
                    assert!(learned.contains(&(ins.art - TAIL_FIRST_ART)));
                    assert!(ins.len >= 2 && rec[1] != 0, "{name}: one-arrow splice");
                    if ins.clipped {
                        // Head index -1: every free slot took combo[i + 1].
                        for k in 0..ins.len - 1 {
                            assert_eq!(queue[k], rec[k + 1] + 0x0B, "{name} clipped");
                        }
                        front = 0;
                        continue;
                    }
                    assert!(ins.at + ins.len <= front, "{name}: splice overlap");
                    front = ins.at;
                    for k in 0..ins.len {
                        assert_eq!(
                            queue[ins.at + k],
                            rec[k] + 0x0B,
                            "{name} art {:#x}",
                            ins.art
                        );
                    }
                    if !marker {
                        assert_ne!(
                            ins.cost,
                            ins.len as i32 * i32::from(TAIL_COST_FIRST),
                            "{name}: a marker-less first pass spliced"
                        );
                    }
                }
                spliced += out.inserted.len();
            }
        }
        eprintln!("[ok] {name}: bank count {count}, {spliced} splices over 1200 runs");
        assert!(spliced > 0, "{name}: the tail never spliced anything");
        total_splices += spliced;
    }
    assert!(total_splices > 0);
}

/// The reject arm's skip is twice the `+0x0B` run length; census the run
/// across the retail banks so a disc whose run spills into the name at
/// `+0x10` shows up here rather than as a runaway walk.
#[test]
fn the_reject_skip_run_is_short_on_every_retail_record() {
    let Some(root) = gate() else { return };
    for (name, _, file) in PLAYER_FILES {
        let (_, recs) = bank(&root, file);
        let mut hist = [0usize; 8];
        for r in &recs {
            let run = r[0x0B..].iter().take_while(|&&b| b != 0).count();
            hist[run.min(7)] += 1;
        }
        eprintln!("[ok] {name}: +0x0B run-length histogram {hist:?}");
        assert!(hist[7] == 0, "{name}: a +0x0B run reached 7+ bytes");
    }
}
