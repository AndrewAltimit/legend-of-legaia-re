fn main() {
    let raw = std::fs::read("extracted/PROT/1054_music_01.BIN").unwrap();
    let c = legaia_lzs::parse_container(&raw).unwrap();
    eprintln!("sections={}", c.sections.len());
    for (i, sec) in c.sections.iter().enumerate() {
        let start = sec.byte_offset as usize;
        let stream = &raw[start..];
        let upper = if i + 1 < c.sections.len() {
            c.sections[i + 1].byte_offset as usize
        } else {
            raw.len()
        };
        let max_consume = upper.saturating_sub(start);
        match legaia_lzs::decompress_tracked(stream, sec.size as usize) {
            Ok((out, consumed)) => {
                let status = if consumed > max_consume {
                    "OVERRUN"
                } else {
                    "ok"
                };
                eprintln!(
                    "  s{:02}: off=0x{:06X} target={:>8}  decoded={:>8}  consumed={:>8}/{:<8}  {}",
                    i,
                    start,
                    sec.size,
                    out.len(),
                    consumed,
                    max_consume,
                    status
                );
            }
            Err(e) => {
                eprintln!("  s{:02}: err {}", i, e);
            }
        }
    }
}
