//! Dev convenience: build a **saturated** test image exercising all three
//! unused-content test points at once, so the work is trivially visible in an
//! emulator: every random-encounter formation slot is set to the Evil Bat (id
//! 176), every treasure chest to the (now-named) "Seru Bell" accessory (id
//! 0xFD), and a New Game's starting inventory holds "Something Good" (id 0x6B)
//! plus a few random consumables. This is NOT the randomizer (which places these
//! *probabilistically*); it forces them everywhere for eyeball testing.
//!
//! A dev generator, not part of the randomizer's shipped surface. Writes a
//! patched `.bin` + a matching `.cue` next to the chosen output path. The image
//! contains Sony bytes - local play only, never redistribute.
//!
//! ```bash
//! cargo run --release -p legaia-patcher --example evilbat_serubell_testbuild -- \
//!     "/path/to/Legend of Legaia (USA).bin" /path/to/out.bin
//! ```

use std::path::Path;

use legaia_patcher::apply;
use legaia_patcher::chest::SceneChests;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::encounter::SceneEncounters;

const EVIL_BAT: u8 = 176; // unused Evil Bat clone
const SERU_BELL: u8 = 0xFD; // unnamed accessory, named "Seru Bell" by the injection
const SOMETHING_GOOD: u8 = 0x6B; // unused 50,000 G sell item
const ITEMS_SEED: u64 = 0x5EED_600D; // fixed seed for the random starting consumables

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(out_bin)) = (args.next(), args.next()) else {
        eprintln!(
            "usage: cargo run -p legaia-patcher --example evilbat_serubell_testbuild -- \
             <input-disc.bin> <output.bin>"
        );
        std::process::exit(2);
    };

    let image = std::fs::read(&input).expect("read input disc");
    let original_len = image.len();
    let mut patcher = DiscPatcher::open(image).expect("parse disc");

    // 1. Name the accessory so every chest reads "Seru Bell".
    match apply::inject_seru_bell_name(&mut patcher).expect("inject name") {
        Some(name) => println!("named accessory 0x{SERU_BELL:02x} -> {name:?}"),
        None => println!("accessory 0x{SERU_BELL:02x} already named"),
    }

    // 2. Every encounter formation slot -> Evil Bat. (Re-read each entry fresh so
    //    later passes see earlier edits.)
    let (mut enc_scenes, mut enc_ids, mut enc_skipped) = (0usize, 0usize, 0usize);
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read entry");
        let Some(mut sc) = SceneEncounters::locate(&entry, idx) else {
            continue;
        };
        let mut changed = 0usize;
        for row in 0..sc.formation_count() {
            let n = sc.formation_ids(row).len();
            for slot in 0..n {
                if let Some(off) = sc.formation_id_offset(row, slot)
                    && sc.decoded[off] != EVIL_BAT
                {
                    sc.decoded[off] = EVIL_BAT;
                    changed += 1;
                }
            }
        }
        if changed == 0 {
            continue;
        }
        match sc.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, sc.man_offset as u64, &stream)
                    .expect("write encounter MAN");
                enc_scenes += 1;
                enc_ids += changed;
            }
            None => enc_skipped += 1,
        }
    }
    println!(
        "encounters: {enc_ids} formation slots -> Evil Bat across {enc_scenes} scenes ({enc_skipped} too tight, skipped)"
    );

    // 3. Every chest give -> Seru Bell (operand + the 0xC2 announcement token).
    let (mut chest_scenes, mut chest_sites, mut chest_skipped) = (0usize, 0usize, 0usize);
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read entry");
        let Some(mut sc) = SceneChests::locate(&entry, idx) else {
            continue;
        };
        if sc.sites.is_empty() {
            continue;
        }
        let n = sc.sites.len();
        for k in 0..n {
            sc.set_site(k, SERU_BELL);
        }
        match sc.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, sc.man_offset as u64, &stream)
                    .expect("write chest MAN");
                chest_scenes += 1;
                chest_sites += n;
            }
            None => chest_skipped += 1,
        }
    }
    println!(
        "chests: {chest_sites} sites -> Seru Bell across {chest_scenes} scenes ({chest_skipped} too tight, skipped)"
    );

    // 4. Starting inventory: Something Good (0x6B) + random consumables, so a
    //    New Game begins with all three test items in the bag. 0x6B is outside
    //    the starting-items randomizer's consumable pool, so force it into the
    //    seed directly (the seed is a flat list of [id, count] bag slots, so any
    //    id is a valid slot entry). Same-size code patch via the seed region.
    {
        use legaia_patcher::starting_items::{
            MAX_STARTING_ITEMS, build_seed_patch, plan_starting_items,
        };
        let scus = patcher.read_named_file("SCUS_942.54").expect("read SCUS");
        let off = legaia_asset::new_game::starting_inv_seed_file_offset(&scus)
            .expect("locate starting-inventory seed region");
        let mut items = vec![(SOMETHING_GOOD, 1u8)];
        for it in plan_starting_items(ITEMS_SEED, MAX_STARTING_ITEMS) {
            if items.len() >= MAX_STARTING_ITEMS {
                break;
            }
            if it.0 != SOMETHING_GOOD {
                items.push(it);
            }
        }
        let patch = build_seed_patch(&items);
        patcher
            .patch_named_file("SCUS_942.54", off as u64, &patch)
            .expect("write starting-inventory seed");
        println!("starting items: {items:02X?} (slot 0 = Something Good 0x{SOMETHING_GOOD:02X})");
    }

    // 5. Write the patched image + a matching single-track cue.
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original_len, "image size must not change");
    std::fs::write(&out_bin, &patched).expect("write patched bin");
    let bin_name = Path::new(&out_bin)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("out.bin");
    let out_cue = Path::new(&out_bin).with_extension("cue");
    let cue = format!("FILE \"{bin_name}\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n");
    std::fs::write(&out_cue, cue).expect("write cue");
    println!(
        "wrote {} ({} bytes) + {}",
        out_bin,
        patched.len(),
        out_cue.display()
    );
    println!("Sony bytes - local play only, do not redistribute.");
}
