//! Temporary investigation tool: break a monster's decoded block into its real
//! sections (stat record / action entries+anim streams / TMD mesh / texture)
//! by parsing the TMD for its true byte length.
//! Run: cargo run --release -p legaia-asset --example delilas_block_anatomy -- extracted/PROT/0867_battle_data.BIN

use legaia_asset::monster_archive;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "extracted/PROT/0867_battle_data.BIN".into());
    let archive = std::fs::read(&path)?;

    for (id, label) in [
        (162u16, "Gi"),
        (163, "Che"),
        (164, "Lu"),
        (62, "KillerBee"),
        (10, "Gimard"),
        (108, "id108(big pair)"),
        (3, "id3(big pair)"),
    ] {
        let Some(m) = monster_archive::mesh(&archive, id)? else {
            println!("id {id}: no mesh");
            continue;
        };
        let block_len = m.block.len();
        let tmd = legaia_tmd::parse(m.tmd_bytes())?;
        let st = tmd.stats();
        let tmd_end = m.tmd_offset + st.total_bytes_consumed;
        let tex_off = m.texture_pool_offset;
        let pre_tmd = m.tmd_offset;
        let between = tex_off.saturating_sub(tmd_end);
        let tex_len = block_len - tex_off;
        println!(
            "id {id:3} {label:15} block {:6.1} KB | head+actions@0..0x{:X} {:5.1} KB | TMD {:5.1} KB ({} verts {} prims) | post-TMD anim/actions {:5.1} KB | texture {:4.1} KB",
            block_len as f64 / 1024.0,
            pre_tmd,
            pre_tmd as f64 / 1024.0,
            st.total_bytes_consumed as f64 / 1024.0,
            st.total_vertices,
            st.total_primitives,
            between as f64 / 1024.0,
            tex_len as f64 / 1024.0,
        );
        if let Some(anims) = monster_archive::animations(&archive, id)? {
            let total_frames: usize = anims.iter().map(|a| a.frames.len()).sum();
            let total_parts_bytes: usize = anims
                .iter()
                .map(|a| a.frames.len() * a.part_count * 9 + 2)
                .sum();
            println!(
                "        actions {:2}  total keyframes {:4}  packed anim bytes ~{:5.1} KB",
                anims.len(),
                total_frames,
                total_parts_bytes as f64 / 1024.0
            );
        }
    }
    Ok(())
}
