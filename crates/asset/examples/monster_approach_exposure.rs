//! Roster sweep for the `0x19` approach park
//! (`docs/subsystems/battle-action-exit-gates.md`): per monster, the clip
//! numbers that decide whether a stale exit-to-idle flag can strand an
//! approach, and how far one Move cycle carries the monster.
//!
//! One cycle of a clip lasts `frame_count * 16` cursor units, and the tick
//! adds `frame_dt * scale * rate >> 2` per battle frame (`FUN_80047430`
//! `0x80047730..0x80047764`) while the root-motion term adds
//! `entry[+0xC] * frame_dt * scale >> 15` of a unit vector (`0x80047D20..`).
//! The speed scale cancels, so one cycle moves the monster
//! `8 * frame_count * speed / rate` units whatever its `+0x21D`.
//!
//! The idle cycle bounds how long a stale exit-to-idle flag survives once
//! Kemaro (PROT 0918) has put the target back through the event-path
//! commit at speed scale 8.
//!
//! Usage:
//!   cargo run -p legaia-asset --example monster_approach_exposure -- \
//!       extracted/PROT/0867_battle_data.BIN [--all]
//! Without `--all` only the monsters that use the walk-less fallback print.
use legaia_actor_formats::monster_archive;

/// Entry `+0x78`: the clip's cursor rate.
const RATE: usize = 0x78;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: monster_approach_exposure <0867 entry> [--all]");
    let all = args.any(|a| a == "--all");
    let entry = std::fs::read(&path).expect("read archive");
    println!(
        "id,name,hp,size,reach,walk_tag,move_frames,move_rate,move_speed,move_cycle_units,idle_frames,idle_rate,idle_window,idle_cycle_vsyncs_scale8,react_tags"
    );
    for id in 0..512u16 {
        let Ok(Some(rec)) = monster_archive::record(&entry, id) else {
            continue;
        };
        let Ok(Some(tags)) = monster_archive::action_tags(&entry, id) else {
            continue;
        };
        if tags.is_empty() {
            continue;
        }
        let has_walk = tags.contains(&0x20);
        if has_walk && !all {
            continue;
        }
        let anims = monster_archive::animations_by_entry(&entry, id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let windows = monster_archive::animation_loop_windows(&entry, id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let by_tag = |t: u8| -> Option<(usize, &monster_archive::MonsterAnimation)> {
            let i = tags.iter().position(|&x| x == t)?;
            Some((i, anims.get(i)?.as_ref()?))
        };
        let size = rec.size_class;
        let reach: u32 = match size {
            0 => 256,
            1 => 384,
            2 => 1024,
            s => u32::from(s) << 4,
        };
        let mv = by_tag(0x01);
        let (mf, mr, ms) = mv.map_or((0, 0, 0), |(_, a)| {
            (
                a.frame_count,
                a.effect_script.get(RATE).copied().unwrap_or(0),
                a.entry_root_speed().unwrap_or(0),
            )
        });
        let cycle = if mr == 0 {
            0
        } else {
            8 * mf as i64 * i64::from(ms) / i64::from(mr)
        };
        let idle = by_tag(0x00);
        let (idf, idr) = idle.map_or((0, 0), |(_, a)| {
            (
                a.frame_count,
                a.effect_script.get(RATE).copied().unwrap_or(0),
            )
        });
        // Kemaro's restore leaves the target at speed scale 8 (`0x801F8978`):
        // one battle frame advances the cursor `2 * 8 * rate >> 2`, so a
        // cycle of `frame_count * 16` units lasts `8 * frame_count / rate`
        // vsyncs.
        let idle_cycle8 = if idr == 0 {
            0
        } else {
            8 * idf / usize::from(idr)
        };
        // `animation_loop_windows` is index-aligned with the compacted list,
        // so look the idle window up by its position among decodable entries.
        let idle_window = idle.and_then(|(i, _)| {
            let compact = anims[..i].iter().filter(|a| a.is_some()).count();
            windows.get(compact).copied().flatten()
        });
        let react: Vec<String> = tags
            .iter()
            .filter(|&&t| (0x02..=0x05).contains(&t))
            .map(|t| format!("{t:02X}"))
            .collect();
        println!(
            "{id},{},{},{size},{reach},{},{mf},{mr},{ms},{cycle},{idf},{idr},{},{idle_cycle8},{}",
            rec.name.replace(',', " "),
            rec.hp,
            u8::from(has_walk),
            idle_window.map_or("-".into(), |(c, s, e)| format!("{c}:{s}-{e}")),
            react.join(" ")
        );
    }
}
