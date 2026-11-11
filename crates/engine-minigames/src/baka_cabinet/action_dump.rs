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
/// and hands the buffer, named `ot5stat.txt` (`ot5` = `other5`, the battle
/// pack the fighters come from), to the write `FUN_8003E7F0` before freeing
/// it. Retail ships that write empty (`addiu sp,-0x90; addiu sp,0x90; jr ra`),
/// so on a retail disc the dump changes nothing; the cabinet calls it on an
/// L1 edge (`_DAT_8007B874 & 4`, `0x801D1E90`).
///
/// The port builds the same text and hands it back instead of touching a
/// filesystem, line for line: per table a header, per action record one line
/// of its eight leading words (`+0x00..+0x1C`, [`BakaActionSet::head`]), one
/// line of the four halfwords of each live sub-keyframe (as many as the
/// record's `+0x1C` count), and a closing line, then the table's closing line
/// (`0x801D55A0..0x801D5744`). The numbers and the commas between them are
/// retail's; retail's labels are overlay rodata and are not reproduced, so the
/// header names the table by its index where retail prints its name.
pub fn action_table_dump(tables: &[BakaActionSet]) -> String {
    let mut out = String::new();
    for (i, set) in tables.iter().take(DUMP_FIGHTERS).enumerate() {
        out.push_str(&format!("ACT_TBL {i}\n"));
        for action in 0..DUMP_ACTIONS_PER_FIGHTER {
            let words = set.head.get(action).copied().unwrap_or_default();
            let words: Vec<String> = words.iter().map(i32::to_string).collect();
            out.push_str(&format!("  {{{}\n", words.join(",")));
            for k in set.sub_keyframes.get(action).into_iter().flatten() {
                out.push_str(&format!(
                    "  ,{},{},{},{}\n",
                    k.offset[0], k.offset[1], k.offset[2], k.frame
                ));
            }
            out.push_str("  },\n");
        }
        out.push_str("};\n");
    }
    out
}
