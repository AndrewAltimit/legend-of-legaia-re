//! The developer action-table dump.

use legaia_minigame_formats::baka_opponents::BakaActionSet;

// ------------------------------------------------- developer action dump

/// Actions per fighter the dump walks (`9`).
pub const DUMP_ACTIONS_PER_FIGHTER: usize = 9;
/// Fighter action tables the dump walks (`0x11`).
pub const DUMP_FIGHTERS: usize = 0x11;

/// PORT: FUN_801d553c - the developer action-table dump.
///
/// Retail allocates a `0x64000`-byte scratch buffer, walks all `0x11` fighter
/// action tables (`PTR_DAT_801DB8B8[i]`) and all `9` `0x60`-byte action
/// records inside each, appends one line per action and one per sub-keyframe,
/// and writes the whole thing out as `ot5stat.txt` (`ot5` = `other5`, the
/// battle pack the fighters come from) before freeing the buffer. Its return
/// value is the byte count written.
///
/// The port builds the same text and hands it back instead of touching a
/// filesystem. Two columns are narrower than retail's: retail prints all eight
/// leading words of the action record and the four halfwords of every
/// sub-keyframe, while `legaia_minigame_formats::baka_opponents::parse_actions` decodes
/// only the record's power (`+0x18`) and sub-keyframe count (`+0x1C`) - so the
/// per-keyframe TRS rows are absent until that parser widens. Retail's own
/// format strings are overlay rodata and are not reproduced; the labels here
/// are the port's.
pub fn action_table_dump(tables: &[BakaActionSet]) -> String {
    let mut out = String::new();
    for (i, set) in tables.iter().take(DUMP_FIGHTERS).enumerate() {
        out.push_str(&format!("ACT_TBL {i}\n"));
        for action in 0..DUMP_ACTIONS_PER_FIGHTER {
            let power = set.power.get(action).copied().unwrap_or(0);
            let keyframes = set.keyframes.get(action).copied().unwrap_or(0);
            out.push_str(&format!(
                "  act {action} power {power} keyframes {keyframes}\n"
            ));
        }
        out.push('\n');
    }
    out
}
