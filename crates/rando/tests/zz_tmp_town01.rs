use legaia_asset::{field_disasm, man_section, scene_asset_table};
use legaia_rando::disc::DiscPatcher;

#[test]
fn explore_town01_entry_script() {
    let Some(p) = std::env::var_os("LEGAIA_DISC_BIN") else {
        return;
    };
    let img = std::fs::read(p).unwrap();
    let patcher = DiscPatcher::open(img).unwrap();
    let map = patcher.cdname().expect("cdname");
    let (raw_start, raw_end) =
        legaia_prot::cdname::block_range_for_name(&map, "town01").expect("town01");
    println!(
        "town01 raw-TOC range [{raw_start},{raw_end}) -> extraction [{},{})",
        raw_start as i64 - 2,
        raw_end as i64 - 2
    );
    const MAN_TYPE: u8 = 0x03;
    for ext in (raw_start as i64 - 2)..(raw_end as i64 - 2) {
        let ext = ext as usize;
        let Ok(entry) = patcher.read_entry(ext) else {
            continue;
        };
        let Some(table) = scene_asset_table::detect(&entry) else {
            continue;
        };
        let Some(man) = table
            .used()
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()
        else {
            continue;
        };
        if man.size == 0 || man.data_offset == 0 {
            continue;
        }
        let body = &entry[man.data_offset as usize..];
        let Ok((decoded, _)) = legaia_lzs::decompress_tracked(body, man.size as usize) else {
            continue;
        };
        let Ok(mf) = man_section::parse(&decoded) else {
            println!("ext {ext}: MAN parse fail");
            continue;
        };
        println!(
            "ext {ext}: MAN size {} parts: p0={} p1={} p2={} data_region={:#x}",
            decoded.len(),
            mf.partitions[0].len(),
            mf.partitions[1].len(),
            mf.partitions[2].len(),
            mf.data_region_offset
        );
        // Disassemble partition-1 record 0 (candidate entry script) + partition-0 record 0.
        for (pi, label) in [(1usize, "p1r0"), (0usize, "p0r0")] {
            if let Some(&off) = mf.partitions[pi].first() {
                let rstart = mf.data_region_offset + off as usize;
                let locals = decoded.get(rstart).copied().unwrap_or(0) as usize;
                let pc0 = rstart + 1 + locals * 2 + 4;
                print!(
                    "  {label} rec_start={:#x} locals={} pc0={:#x}: ",
                    rstart, locals, pc0
                );
                let mut pc = pc0;
                let mut n = 0;
                while n < 14 {
                    let Ok(insn) = field_disasm::decode(&decoded, pc) else {
                        print!("[decode-end] ");
                        break;
                    };
                    if insn.size == 0 {
                        break;
                    }
                    print!("{} ", field_disasm::format_instruction(&insn, &decoded));
                    pc += insn.size;
                    n += 1;
                }
                println!();
            }
        }
    }
}
