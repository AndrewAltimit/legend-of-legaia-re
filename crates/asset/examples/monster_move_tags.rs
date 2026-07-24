//! Roster sweep: which monsters lack the tag-0x20 approach-transition clip,
//! and of those, which also lack the tag-1 Move clip (the approach-fix
//! guard's only unrescuable case). Usage:
//!   cargo run -p legaia-asset --example monster_move_tags -- extracted/PROT/0867_battle_data.BIN
use legaia_asset::monster_archive;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: monster_move_tags <0867 entry>");
    let entry = std::fs::read(&path).expect("read archive");
    let (mut total, mut no_walk, mut no_move) = (0u32, 0u32, 0u32);
    for id in 0..512u16 {
        let Ok(Some(tags)) = monster_archive::action_tags(&entry, id) else {
            continue;
        };
        if tags.is_empty() {
            continue;
        }
        total += 1;
        let has_walk = tags.contains(&0x20);
        let has_move = tags.contains(&0x01);
        if !has_walk {
            no_walk += 1;
            if !has_move {
                no_move += 1;
                println!("id {id}: tags {tags:02X?} - NO walk AND NO Move clip");
            }
        }
    }
    println!(
        "{total} monsters; {no_walk} lack tag 0x20 (use the 0x19 fallback); {no_move} of those also lack tag 1"
    );
}
