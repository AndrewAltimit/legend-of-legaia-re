//! Chapter-2 local puzzle flags: treasure chests, the stone-symbol puzzle and the Rayman hub.

use super::*;

/// The `tunnela` `0x96..=0x9C` chain - a **seven-chest treasure set**, NOT a
/// story-spine gate family. A poll traversal caught all seven set in-scene in
/// strict order at distinct tiles; static mining resolves what they are.
///
/// tunnela carries ONE (bundle) MAN, no streaming variant, and its 12
/// partition-2 records hold **zero C1/C2 header gates** - so `0x96..=0x9C` are
/// not `FUN_8003BDE0` record gates. They live inline in seven byte-identical
/// partition-1 walk-on-trigger records (`P1[1..=7]`), one per flag, each a
/// treasure-chest: **TEST own flag (already opened?) -> give item + SET own
/// flag**. The records are laid out on a fixed `0xA2` stride with TEST->SET at
/// `+0x5D` (`0x96`: TEST @0x457 / SET @0x4B4; `0x9C`: TEST @0x823 / SET @0x880);
/// none carry a CLEAR (a chest never re-locks). The field-VM disassembler
/// desyncs into each record's chest dialogue after the opening TEST, so the
/// SET for `0x96`/`0x97`/`0x9A` is invisible to `system_flag_census` (which
/// reports it for the other four) - the byte-exact `50 XX` op is the robust
/// signal, and the poll confirmed all seven fire. The ending scene `edlast`
/// (`P2[1]`) TESTs all seven together in a collection/completion battery - the
/// only cross-scene consumer, and why these persist to the endgame.
///
/// This is the "dungeon-LOCAL puzzle/objective state" class (cf. taiku's
/// `0x505..=0x519`), the mining frontier the poll's `[lead]` markers surface.
#[test]
fn chapter2_tunnela_treasure_chest_flags() {
    let Some(index) = open_index() else { return };
    let man_of = |scene_name: &str| -> Vec<u8> {
        let scene = Scene::load(&index, scene_name).expect("load");
        scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let count = |hay: &[u8], needle: [u8; 2]| hay.windows(2).filter(|w| *w == needle).count();

    // tunnela: each 0x96+k is SET (`50 XX`) and TEST (`70 XX`) exactly once,
    // never CLEARed - seven one-way treasure-chest flags.
    let tunnela = man_of("tunnela");
    for f in 0x96u8..=0x9C {
        assert_eq!(count(&tunnela, [0x50, f]), 1, "tunnela SET 0x{f:02X} once");
        assert_eq!(count(&tunnela, [0x70, f]), 1, "tunnela TEST 0x{f:02X} once");
        assert_eq!(
            count(&tunnela, [0x60, f]),
            0,
            "tunnela never CLEAR 0x{f:02X}"
        );
    }

    // edlast (the ending): TESTs all seven (the completion battery), sets none.
    let edlast = man_of("edlast");
    for f in 0x96u8..=0x9C {
        assert_eq!(count(&edlast, [0x70, f]), 1, "edlast TEST 0x{f:02X}");
        assert_eq!(count(&edlast, [0x50, f]), 0, "edlast never SET 0x{f:02X}");
    }

    // Structural: tunnela's partition-2 records carry NO header gates (this is
    // inline chest state, not a spine gate family).
    let mf = legaia_scene_formats::man_section::parse(&tunnela).expect("parse");
    let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
    for r in 0..n_p2 {
        let (c1, c2) =
            legaia_engine_field::man_field_scripts::partition2_record_gates(&mf, &tunnela, r)
                .unwrap_or_default();
        assert!(
            c1.is_empty() && c2.is_empty(),
            "tunnela P2[{r}] has no gates"
        );
    }
}

/// The `stone` `0x1D8..=0x1E8` cluster - the in-game **"Gate of Shadows"
/// symbol-code puzzle**, the richest dungeon-LOCAL objective family the poll
/// surfaced. Static mining decodes both the mechanic and the flag layout.
///
/// The puzzle (from the scene's own dialogue): a statue with a **row of four
/// symbols** you set via four directional **Keys** (North/South/East/West),
/// each key pressing one of four elemental **Symbols** (Wind/Fire/Water/Earth).
/// Enter the right four-symbol code to open the Gate of Shadows ("to visit the
/// past"); a wrong answer spawns a punishment battle.
///
/// Flag layout - `0x1D8..=0x1E7` = **16 state flags = 4 keys x 4 symbols**, in
/// four contiguous mutually-exclusive groups of four
/// (`0x1D8..=0x1DB` / `0x1DC..=0x1DF` / `0x1E0..=0x1E3` / `0x1E4..=0x1E7`).
/// Each flag is SET when pressed and CLEARed **six** times: pressing a symbol
/// batch-clears its key's whole group then sets the chosen one, so each group
/// always holds exactly one bit = "the symbol currently on this key".
///
/// The disc **encodes the solution**: the solved record contains a block of
/// four `51 XX` SET ops re-asserting exactly one symbol per key -
/// `{0x1D9, 0x1DE, 0x1E3, 0x1E4}` (so those four flags are SET twice; the
/// other twelve once). This is the four-symbol code, and the poll caught
/// exactly this as the solved resting state (+ `0x1E8`). `0x1E8` = the
/// **puzzle-SOLVED gate**: set by the
/// solved-cutscene record `P2[5]`, TESTed ~20x across stone's scene-entry
/// scripts (`P0[0..5]`, `P1[0]`) to gate the opened-gate behavior, and - the
/// only cross-scene consumer - TESTed once in `map02 P0[32]` (the Sebucus hub
/// reads whether the Gate is open). `0x492` is a shared region-progress flag
/// (SET in both stone `P1[0]` and `map02 P2[13]`), NOT part of the mechanic.
///
/// Not story spine - inline puzzle state (the taiku-`0x505` class), but far
/// richer, and with a real cross-scene payoff gate (`0x1E8` in `map02`).
#[test]
fn chapter2_stone_symbol_puzzle_flags() {
    let Some(index) = open_index() else { return };
    let man_of = |scene_name: &str| -> Vec<u8> {
        let scene = Scene::load(&index, scene_name).expect("load");
        scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    // Count a 2-byte field-VM flag op. The opcode carries the flag's high byte
    // in its low nibble (`base | (idx>>8)`), the next byte is `idx & 0xFF`.
    let count_op = |hay: &[u8], base: u8, idx: u16| -> usize {
        let op = [base | (idx >> 8) as u8, (idx & 0xFF) as u8];
        hay.windows(2).filter(|w| *w == op).count()
    };

    let stone = man_of("stone");
    // The 16 selector flags: each SET (once when pressed; the four SOLUTION
    // symbols a second time in the solved record) and CLEARed six times
    // (batch-reset = the four 4-way mutually-exclusive symbol groups).
    let mut solution: Vec<u16> = Vec::new();
    for f in 0x1D8u16..=0x1E7 {
        let sets = count_op(&stone, 0x50, f);
        assert!(sets >= 1, "stone SETs selector 0x{f:X}");
        assert_eq!(
            count_op(&stone, 0x60, f),
            6,
            "stone CLEAR 0x{f:X} x6 (batch)"
        );
        if sets == 2 {
            solution.push(f);
        }
    }
    // The disc ENCODES the answer: the solved record re-asserts exactly one
    // symbol per key group -> the four-symbol code. (Poll-confirmed: this is
    // the solved resting state the traversal reached.)
    assert_eq!(
        solution,
        vec![0x1D9, 0x1DE, 0x1E3, 0x1E4],
        "encoded solution: one symbol per key (A..D)"
    );
    // The SOLVED gate is a one-way completion flag, NOT a batch-cleared
    // selector: heavily tested, at most one clear.
    assert!(
        count_op(&stone, 0x50, 0x1E8) >= 1,
        "stone SETs 0x1E8 (solved)"
    );
    assert!(
        count_op(&stone, 0x60, 0x1E8) <= 1,
        "0x1E8 is not batch-cleared"
    );
    assert!(
        count_op(&stone, 0x70, 0x1E8) >= 10,
        "0x1E8 tested pervasively"
    );

    // Cross-scene: the Sebucus hub map02 reads the solved gate, shares 0x492,
    // and carries NONE of the 16 stone-local selector flags.
    let map02 = man_of("map02");
    assert!(
        count_op(&map02, 0x70, 0x1E8) >= 1,
        "map02 TESTs the solved gate"
    );
    assert!(count_op(&map02, 0x50, 0x492) >= 1, "map02 shares 0x492");
    for f in 0x1D8u16..=0x1E7 {
        assert_eq!(count_op(&map02, 0x50, f), 0, "0x{f:X} is stone-local");
    }
}

/// The `rayman` `0x1FD..=0x1FF` cluster - the **"three Haris" oracle
/// interaction hub**. Distinct from rayman's header-gate spine chain
/// (`0x201`/`0x1FB`/`0x200`/`0x1FC` off the `0x1EB`/`0x1EC` arc, covered by
/// `chapter2_dungeon_gate_families`): this is a self-contained **field-VM
/// AND-gate**, not a C1/C2 header chain.
///
/// The mechanic (from the scene's own dialogue): three "Hari" statues -
/// "from the left, teller of the past, of the present and of the future" -
/// that "remain awake only for a short time". Talking to each of the three
/// oracles sets one flag; combined gates then check that all three have been
/// consulted before advancing ("Hari has entrusted you with the future").
///
/// Flag layout (byte-exact off the decoded MAN):
/// - `0x1FD`/`0x1FE`/`0x1FF` = one flag per oracle. Each is SET **twice**:
///   once in its own per-actor NPC script (census: `P1[44]`/`P1[45]`/`P1[46]`,
///   three consecutive actor records) and once in a dev **reset arm** ("End
///   flag setting" / "Back=") that SETs all three consecutively
///   (`51 FF 51 FE 51 FD`) then CLEARs all three consecutively
///   (`61 FF 61 FE 61 FD`) - hence CLEAR exactly once each.
/// - All three are TESTed together in tight AND-gate clusters (census records
///   `P1[42]`/`P1[43]`/`P1[47]` + the completion cutscene `P2[18]`), the
///   "have you consulted all three oracles" check. Six TESTs per flag.
///
/// **Self-contained**: no genuine cross-scene consumer. The one out-of-scene
/// census hit (`geremi P1[22]` "CLEAR 0x1FE") is a disassembler-desync false
/// positive - the byte pair `61 FE` is consumed as the operand of a 3-byte
/// `0x26` op sitting inside Shift-JIS text (`26 61 FE 0D 83 4A ...`), not a
/// standalone CLEAR at an instruction boundary. Unlike `stone`'s `0x1E8`, this
/// hub has no cross-scene payoff gate.
#[test]
fn chapter2_rayman_three_haris_hub_flags() {
    let Some(index) = open_index() else { return };
    let man_of = |scene_name: &str| -> Vec<u8> {
        let scene = Scene::load(&index, scene_name).expect("load");
        scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let count_op = |hay: &[u8], base: u8, idx: u16| -> usize {
        let op = [base | (idx >> 8) as u8, (idx & 0xFF) as u8];
        hay.windows(2).filter(|w| *w == op).count()
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();

    let rayman = man_of("rayman");

    // Each oracle flag: SET twice (own NPC script + the reset arm), CLEARed
    // exactly once (the reset arm), TESTed pervasively by the AND-gates.
    for f in 0x1FDu16..=0x1FF {
        assert_eq!(count_op(&rayman, 0x50, f), 2, "rayman SET 0x{f:X} twice");
        assert_eq!(count_op(&rayman, 0x60, f), 1, "rayman CLEAR 0x{f:X} once");
        assert!(
            count_op(&rayman, 0x70, f) >= 4,
            "rayman TESTs 0x{f:X} in the AND-gates"
        );
    }

    // The dev reset arm SETs all three consecutively then CLEARs all three
    // consecutively - one such block each.
    assert_eq!(
        count_sub(&rayman, &[0x51, 0xFF, 0x51, 0xFE, 0x51, 0xFD]),
        1,
        "reset arm SETs all three oracles"
    );
    assert_eq!(
        count_sub(&rayman, &[0x61, 0xFF, 0x61, 0xFE, 0x61, 0xFD]),
        1,
        "reset arm CLEARs all three oracles"
    );

    // The AND-gate: distinct sites where all three flags are TESTed together
    // (the "consulted all three oracles" conjunction). Count non-overlapping
    // 24-byte windows carrying a TEST of each of the three.
    let tf: [[u8; 2]; 3] = [[0x71, 0xFD], [0x71, 0xFE], [0x71, 0xFF]];
    let has = |w: &[u8], s: &[u8; 2]| w.windows(2).any(|x| x == *s);
    let mut clusters = 0usize;
    let mut o = 0usize;
    while o + 24 <= rayman.len() {
        let w = &rayman[o..o + 24];
        if tf.iter().all(|s| has(w, s)) {
            clusters += 1;
            o += 24;
        } else {
            o += 1;
        }
    }
    assert!(
        clusters >= 4,
        "at least four AND-gate sites, got {clusters}"
    );
}
