//! Inspect the 6335-byte payload that all 77 entries in the 0x0101990C
//! cluster decompress to. Reports non-zero positions, byte distribution,
//! and looks for periodic structure.
use std::collections::BTreeMap;

fn main() {
    let raw = std::fs::read("extracted/PROT/0115_keikoku.BIN").unwrap();
    let decoded = legaia_lzs::decompress_container_strict(&raw).unwrap();
    let s1 = &decoded[1]; // 6335 B section
    println!("section 1: {} bytes", s1.len());
    let nz: Vec<(usize, u8)> = s1
        .iter()
        .enumerate()
        .filter(|(_, b)| **b != 0)
        .map(|(i, b)| (i, *b))
        .collect();
    println!(
        "non-zero bytes: {} / {} ({:.1}%)",
        nz.len(),
        s1.len(),
        100.0 * nz.len() as f64 / s1.len() as f64
    );

    // Distribution of non-zero byte values
    let mut hist: BTreeMap<u8, usize> = BTreeMap::new();
    for (_, b) in &nz {
        *hist.entry(*b).or_insert(0) += 1;
    }
    println!("\nnon-zero byte histogram (top 20):");
    let mut v: Vec<_> = hist.iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (b, c) in v.iter().take(20) {
        println!("  0x{:02X}  ({:>4})", b, c);
    }

    // Gaps between non-zero positions — look for periodicity
    let mut gaps: BTreeMap<usize, usize> = BTreeMap::new();
    for w in nz.windows(2) {
        let g = w[1].0 - w[0].0;
        *gaps.entry(g).or_insert(0) += 1;
    }
    println!("\ntop 10 gap sizes between consecutive non-zero positions:");
    let mut g: Vec<_> = gaps.iter().collect();
    g.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (gap, c) in g.iter().take(10) {
        println!("  gap={:>4}  ({:>4} occurrences)", gap, c);
    }

    // First 32 non-zero positions
    println!("\nfirst 32 non-zero positions:");
    for (i, (pos, b)) in nz.iter().enumerate().take(32) {
        println!(
            "  #{:>2}  pos={:>5} (0x{:04X})  byte=0x{:02X}",
            i, pos, pos, b
        );
    }

    // Try interpreting as 2D grid: search for stride S such that the
    // non-zero "lines" appear at consistent column offsets.
    println!("\npositional density per stride:");
    for stride in &[8, 16, 32, 60, 64, 79, 80, 96, 99, 100, 128] {
        let mut col_hist = vec![0usize; *stride];
        for (pos, _) in &nz {
            col_hist[pos % stride] += 1;
        }
        let max_col = col_hist.iter().max().copied().unwrap_or(0);
        let used_cols = col_hist.iter().filter(|c| **c > 0).count();
        println!(
            "  stride={:>4}  used_cols={:>4}/{:<4}  max_col_density={}",
            stride, used_cols, stride, max_col
        );
    }
}
