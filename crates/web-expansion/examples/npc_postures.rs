//! Print `legaia-expansion-format`'s `data/npc_postures.toml` for the scenes
//! named on the command line, measured off an extracted disc
//! (`legaia_web_expansion::npc_posture`):
//!
//! ```text
//! cargo run -p legaia-web-expansion --example npc_postures -- extracted tower koin1 ...
//! ```

use legaia_engine_scene::scene::ProtIndex;
use legaia_expansion_format::npc_postures::PostureTable;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .expect("usage: npc_postures <extracted> <scene>...");
    let index = ProtIndex::open_extracted(std::path::Path::new(&root))?;
    let mut table = PostureTable::default();
    for scene in args {
        for p in legaia_web_expansion::npc_posture::measure_scene(&index, &scene)? {
            if std::env::var_os("NPC_POSTURES_VERBOSE").is_some() {
                for c in &p.clips {
                    eprintln!(
                        "{scene} {:3} model {:3} clip {:3} frames {:3} height {:6.1} raise {:6.1} legs {:6.1} {:?}",
                        p.index,
                        p.model_index,
                        c.clip,
                        c.frames,
                        c.height,
                        c.raise,
                        c.legs,
                        c.posture
                    );
                }
            }
            table.npc.push(p.postures(&scene));
        }
    }
    print!("{}", legaia_web_expansion::npc_posture::table_toml(&table));
    Ok(())
}
