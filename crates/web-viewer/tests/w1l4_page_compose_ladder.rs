//! Composition ladder for the browser surfaces `play_compose_ladder` is
//! structurally blind to: the **memory-card rack**, the save screen's info-panel
//! modes, the Items target panel, and the three standalone minigame pages.
//!
//! `play_compose_ladder` drives the play page's frame loop, and its Load rung
//! already opens the card rack - but with **no card inserted**, so
//! `card_block_snapshots` takes its `self.card(slot) == None` early return and
//! the retail directory walk behind it never runs. The minigame pages are
//! outside the union for a blunter reason: the union is a list of named
//! binaries, and nothing drives `LegaiaMinigames`' composition at all.
//!
//! | # | rung | reaches |
//! |---|---|---|
//! | 1 | Load / Save over two **inserted** cards | `FUN_801E3AF0` directory scan + `FUN_801E3BA0` free-block budget, and `FUN_801E1208`'s class walk through the Save commit's filename pick |
//! | 2 | the info panel's three modes | `FUN_801E3F74` - preview / free / foreign, told apart by what the panel draws |
//! | 3 | Items -> Use -> target select | `FUN_801D6A54` via `target_panel_view_model`, over a party hurt by a card load |
//! | 4 | the dance page | `FUN_801D4098` clip-driver gate, `FUN_801D3D78` hit sting |
//! | 5 | the Baka Fighter page | `FUN_801D5ED0` HUD widget quad |
//! | 6 | the Muscle Dome page | `FUN_80053CB8` battle-load stat init |
//!
//! Every rung scores on **values**, not on "it ran": a compose that returns an
//! empty draw list, a directory walk that prices every cell the same, or a
//! fighter built from the fallback constants instead of the disc all fail here
//! rather than passing as entered.
//!
//! Coverage export (what wires this into the reach report):
//!
//! ```text
//! cargo llvm-cov -p legaia-web-viewer --test w1l4_page_compose_ladder \
//!     --json --output-path target/cov-w1l4_page_compose_ladder.json
//! ```
//!
//! Exported **without** `--release`: an optimised export loses inlined callees
//! to a zero-count out-of-line record, which reads exactly like never-called.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_save::card;
use legaia_web_viewer::minigames::LegaiaMinigames;
use legaia_web_viewer::runtime::LegaiaRuntime;

/// 320x240 keeps the boot-UI stage transform at identity, so the JSON draw
/// coordinates are the raw `legaia-engine-ui` output.
const W: u32 = 320;
const H: u32 = 240;

const UP: u16 = 0x0010;
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;

/// Root command rows (Items / Magic / Equip / Status / Options / Load / Save).
const ROW_LOAD: usize = 5;
const ROW_SAVE: usize = 6;

/// A kingdom world map - the only scene family whose MAN header sets the
/// save-allow bit, so the only place the Save row opens at all.
const SAVE_ALLOWED_SCENE: &str = "map01";

// ---------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

fn loaded_in(scene: &str) -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field(scene).ok()?;
    Some(rt)
}

fn minigames() -> Option<LegaiaMinigames> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).ok()?;
    Some(mg)
}

/// `(sprites, texts)` of the current menu draw list.
fn menu_draws(rt: &LegaiaRuntime) -> (usize, usize) {
    let v = json(&rt.play_menu_draws_json(W, H));
    assert_eq!(v["open"], true, "the menu must be open to compose");
    (
        v["sprites"].as_array().map(|a| a.len()).unwrap_or(0),
        v["texts"].as_array().map(|a| a.len()).unwrap_or(0),
    )
}

fn open_row(rt: &mut LegaiaRuntime, row: usize) {
    rt.play_menu_close();
    rt.play_menu_open();
    for _ in 0..row {
        rt.play_menu_input(DOWN);
    }
    rt.play_menu_input(CROSS);
}

/// Sit on the "Now checking" beat until the grid comes up.
fn settle_card_read(rt: &mut LegaiaRuntime) {
    for _ in 0..240 {
        rt.play_menu_input(0);
    }
}

fn unwind(rt: &mut LegaiaRuntime) {
    for _ in 0..10 {
        if !rt.play_menu_is_open() {
            return;
        }
        rt.play_menu_input(CIRCLE);
    }
    rt.play_menu_close();
}

// ---------------------------------------------------------------------------
// Card fixtures
// ---------------------------------------------------------------------------

/// A raw 128 KiB card with a well-formed directory: every block free, every
/// frame's XOR checksum right.
fn blank_card() -> Vec<u8> {
    let mut buf = vec![0u8; card::CARD_SIZE];
    buf[..2].copy_from_slice(&card::CARD_MAGIC);
    for i in 1..=card::DIR_FRAMES {
        let off = card::DIR_FRAME_SIZE * i;
        buf[off..off + 4].copy_from_slice(&card::state::FREE.to_le_bytes());
        let ck = buf[off..off + 0x7F].iter().fold(0u8, |a, &b| a ^ b);
        buf[off + 0x7F] = ck;
    }
    buf
}

/// A card carrying one Legaia save in `block`, filed under save number
/// `save_index` - the shape retail writes, where the filename's number is the
/// save-select list position and *not* the block.
fn card_with_save(block: u8, save_index: u32, name: &str, gold: i32, hp: (u16, u16)) -> Vec<u8> {
    card_with_bag(block, save_index, name, gold, hp, &[])
}

/// The same, with an inventory seeded into the save's 72-slot retail bag - the
/// only way a host-driven test can put a chosen item in the player's hands, and
/// a path a player actually walks (resume a save, open Items).
fn card_with_bag(
    block: u8,
    save_index: u32,
    name: &str,
    gold: i32,
    hp: (u16, u16),
    bag: &[(u8, u8)],
) -> Vec<u8> {
    let mut buf = blank_card();
    let f = card::DIR_FRAME_SIZE * block as usize;
    buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
    buf[f + 4..f + 8].copy_from_slice(&(card::BLOCK_SIZE as u32).to_le_bytes());
    buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    let filename = card::legaia_save_filename(save_index);
    buf[f + 10..f + 10 + filename.len()].copy_from_slice(filename.as_bytes());
    let ck = buf[f..f + 0x7F].iter().fold(0u8, |a, &b| a ^ b);
    buf[f + 0x7F] = ck;

    let b = card::BLOCK_SIZE * block as usize;
    let sc = &mut buf[b..b + card::BLOCK_SIZE];
    sc[..2].copy_from_slice(&card::SAVE_BLOCK_MAGIC);
    let title = &mut sc[card::RETAIL_TITLE_OFFSET..card::RETAIL_ICON_CLUT_OFFSET];
    title.fill(0x20);
    let digits = card::save_title_digits(save_index);
    for (off, d) in card::RETAIL_TITLE_DIGIT_OFFSETS.iter().zip(digits.iter()) {
        title[*off] = *d;
    }
    let mut rec = legaia_save::CharacterRecord::zeroed();
    rec.set_name(name);
    rec.set_magic_rank(12);
    rec.set_hp_mp_sp(legaia_save::HpMpSp {
        hp_cur: hp.0,
        hp_max: hp.1,
        mp_cur: 8,
        mp_max: 30,
        sp_cur: 0,
        sp_max: 0,
    });
    legaia_save::write_retail_char_records(sc, std::slice::from_ref(&rec.raw)).unwrap();
    legaia_save::write_retail_gold(sc, gold).unwrap();
    if !bag.is_empty() {
        card::write_retail_inventory(sc, bag).unwrap();
    }
    let scene = b"town01";
    let at = card::RETAIL_SCENE_LABEL_OFFSET;
    sc[at..at + scene.len()].copy_from_slice(scene);
    let loc = b"Rim Elm";
    let at = card::RETAIL_LOCATION_NAME_OFFSET;
    sc[at..at + loc.len()].copy_from_slice(loc);
    buf
}

/// The same card with its directory frame declaring the save as spanning the
/// whole card, so `card_free_blocks` prices **zero** blocks free and every
/// unclaimed cell captions foreign instead of inviting an overwrite.
fn card_with_no_free_budget(block: u8, save_index: u32) -> Vec<u8> {
    let mut buf = card_with_save(block, save_index, "Gala", 77, (90, 90));
    let f = card::DIR_FRAME_SIZE * block as usize;
    buf[f + 4..f + 8].copy_from_slice(&(15u32 * card::BLOCK_SIZE as u32).to_le_bytes());
    let ck = buf[f..f + 0x7F].iter().fold(0u8, |a, &b| a ^ b);
    buf[f + 0x7F] = ck;
    buf
}

// ---------------------------------------------------------------------------
// Rung 1 - the card directory walk, priced and classified
// ---------------------------------------------------------------------------

/// The rack reads a real card image's own directory frames, prices its free
/// blocks off them, and picks a save number no file on the card is already
/// using.
///
/// Three retail kernels are behind one flow here, and each has a value the
/// walk must produce rather than merely execute:
///
/// * `card_directory_scan` (`FUN_801E3AF0`) fills the fifteen-slot table - the
///   grid must show the card's save where the card put it, not everywhere;
/// * `card_free_blocks` (`FUN_801E3BA0`) prices the budget - a card whose
///   directory over-declares must stop paying for "free" cells;
/// * `classify_card_directory` (`FUN_801E1208`) says which save *numbers* the
///   card already files, which is what stops the Save commit writing a
///   duplicate filename onto a retail-written card.
#[test]
fn rung1_the_card_rack_walks_a_real_cards_directory() {
    let Some(mut rt) = loaded_in(SAVE_ALLOWED_SCENE) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Retail-shaped card: the save sits in block 1 but is filed as `-03`.
    // Deriving the number from the block would collide the moment a second
    // block is claimed - which is exactly what the commit below checks.
    let original = card_with_save(1, 3, "Vahn", 1234, (140, 200));
    rt.insert_card(0, original.clone(), "card A".into())
        .expect("insert port 1");
    rt.insert_card(1, card_with_no_free_budget(3, 5), "card B".into())
        .expect("insert port 2");

    // The rack's own JSON: the directory walk runs here too (the page's card
    // picker calls it before the menu is ever opened).
    let slots = json(&rt.card_slots_json());
    let arr = slots.as_array().expect("two ports");
    assert_eq!(arr.len(), 2);
    for (port, want) in [(0usize, 1u64), (1, 3)] {
        let blocks = arr[port]["blocks"].as_array().expect("15 block cells");
        assert_eq!(blocks.len(), 15, "the 5x3 preview grid");
        let present: Vec<u64> = blocks
            .iter()
            .filter(|b| b["present"] == true)
            .map(|b| b["block"].as_u64().unwrap())
            .collect();
        assert_eq!(
            present,
            vec![want],
            "port {port}: the walk must find the card's save where the card \
             filed it, and nowhere else"
        );
    }
    assert_eq!(arr[0]["blocks"][0]["name"], "Vahn", "block 1 previews Vahn");
    assert_eq!(arr[0]["blocks"][0]["money"], 1234);
    assert_eq!(arr[0]["blocks"][0]["location"], "Rim Elm");

    // --- the menu flow, composed per edge ---------------------------------
    assert!(
        rt.play_scene_save_allowed(),
        "{SAVE_ALLOWED_SCENE} must permit a menu save, or this rung walks an \
         unopened screen"
    );
    open_row(&mut rt, ROW_SAVE);
    let (pill_sprites, _) = menu_draws(&rt);
    assert!(pill_sprites > 0, "the pill row draws its chrome");
    rt.play_menu_input(CROSS); // pick SLOT 1
    settle_card_read(&mut rt);
    let (grid_sprites, grid_texts) = menu_draws(&rt);
    assert!(
        grid_sprites > pill_sprites,
        "the block grid draws more chrome than the pill row ({grid_sprites} \
         vs {pill_sprites})"
    );
    assert!(grid_texts > 0, "the grid's info panel drew no text");

    // Walk to a free cell (cell 4 = block 5) and commit a save there. The
    // preferred number for block 5 is 4, which this card does not file, so
    // the classification must leave it alone; what it must NOT do is collide
    // with the `-03` already on the card.
    for _ in 0..4 {
        rt.play_menu_input(RIGHT);
        let _ = rt.play_menu_draws_json(W, H);
    }
    rt.play_menu_input(CROSS);
    let _ = rt.play_menu_draws_json(W, H);
    // A free block confirms straight through on some builds and raises the
    // overwrite prompt on others; drive the prompt's Yes either way.
    if !rt.card_slot_dirty(0) {
        rt.play_menu_input(LEFT);
        rt.play_menu_input(CROSS);
    }
    if !rt.card_slot_dirty(0) {
        rt.play_menu_input(UP);
        rt.play_menu_input(CROSS);
    }
    assert!(
        rt.card_slot_dirty(0),
        "confirming a free cell must write into the card"
    );
    unwind(&mut rt);

    // The classification's whole job: two saves, two distinct filenames.
    let exported = rt.export_card(0);
    let names: Vec<String> = (1..=15u8)
        .filter_map(|b| {
            let f = card::DIR_FRAME_SIZE * b as usize;
            let n: String = exported[f + 10..f + 30]
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as char)
                .collect();
            (!n.is_empty()).then_some(n)
        })
        .collect();
    assert_eq!(names.len(), 2, "two files on the card now: {names:?}");
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(
        uniq.len(),
        names.len(),
        "the save-number classification let two files collide: {names:?}"
    );
    assert!(
        names.contains(&card::legaia_save_filename(3)),
        "the card's original `-03` must be untouched: {names:?}"
    );
    // The generic card walker (what an emulator uses) must still see both.
    assert_eq!(card::parse_card(&exported).expect("card parses").len(), 2);

    // Only the newly-claimed block and its directory frame may have moved.
    let b5 = card::BLOCK_SIZE * 5;
    let f5 = card::DIR_FRAME_SIZE * 5;
    let escaped: Vec<usize> = original
        .iter()
        .zip(exported.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .filter(|i| {
            !(b5..b5 + card::BLOCK_SIZE).contains(i) && !(f5..f5 + card::DIR_FRAME_SIZE).contains(i)
        })
        .collect();
    assert!(escaped.is_empty(), "writes escaped block 5: {escaped:?}");
}

// ---------------------------------------------------------------------------
// Rung 2 - the info panel's three modes
// ---------------------------------------------------------------------------

/// The bottom info panel has three faces, and which one it wears is
/// `SlotInfoMode::for_slot` (`FUN_801E3F74`) over the focused cell's content
/// class. All three are reached here through the composed draw list, and they
/// are told apart by what the panel *puts on screen*:
///
/// | cell | class | panel |
/// |---|---|---|
/// | the card's save | `LegaiaSave` | the stats preview (name / level / HP / MP / location / play time) |
/// | an unclaimed cell the budget affords | `Free` | one centred caption, `"Able to save."` on the Save path |
/// | an unclaimed cell past the budget | `Foreign` | one centred caption, `"Not a Legend of Legaia save."` |
///
/// The two captions differ in length, and the preview is a multi-row panel, so
/// the three modes separate on glyph count without the test needing to read
/// strings out of a glyph list.
#[test]
fn rung2_the_save_panel_wears_all_three_info_modes() {
    let Some(mut rt) = loaded_in(SAVE_ALLOWED_SCENE) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    rt.insert_card(
        0,
        card_with_save(1, 3, "Vahn", 1234, (140, 200)),
        "A".into(),
    )
    .expect("insert port 1");
    rt.insert_card(1, card_with_no_free_budget(3, 5), "B".into())
        .expect("insert port 2");

    /// Focus cell `cell` of `port`'s grid and return the composed text count.
    fn panel_texts(rt: &mut LegaiaRuntime, port: usize, cell: usize) -> usize {
        open_row(rt, ROW_SAVE);
        for _ in 0..port {
            rt.play_menu_input(DOWN);
        }
        rt.play_menu_input(CROSS);
        settle_card_read(rt);
        for _ in 0..cell {
            rt.play_menu_input(RIGHT);
        }
        let (_, texts) = menu_draws(rt);
        unwind(rt);
        texts
    }

    // Port 1: cell 0 is the save (preview), cell 4 is free (the budget pays).
    let preview = panel_texts(&mut rt, 0, 0);
    let free = panel_texts(&mut rt, 0, 4);
    // Port 2: cell 0 is unclaimed and the budget is spent, so it is foreign.
    let foreign = panel_texts(&mut rt, 1, 0);

    eprintln!("[w1l4] info panel glyphs: preview={preview} free={free} foreign={foreign}");
    assert!(preview > 0 && free > 0 && foreign > 0, "every mode draws");
    assert!(
        preview > free && preview > foreign,
        "the stats preview is a multi-row panel, the other two are one \
         centred line (preview={preview} free={free} foreign={foreign})"
    );
    assert!(
        foreign > free,
        "\"Not a Legend of Legaia save.\" is longer than \"Able to save.\" \
         - a panel that drew the same caption for both would tie here \
         (free={free} foreign={foreign})"
    );
}

// ---------------------------------------------------------------------------
// Rung 3 - Items -> Use -> the window-14 target panel
// ---------------------------------------------------------------------------

/// The Items screen's Use row, driven into **target select**, which is the only
/// state that builds the window-14 party panel and therefore the only one that
/// runs `target_panel_mode` (`FUN_801D6A54`).
///
/// Two things had to be arranged before a pad could reach it, and both are
/// retail's rules rather than harness scaffolding:
///
/// * **The bag.** A page has no give-item affordance, so the items come in the
///   way a player's do - on a **resumed save**, whose 72-slot retail inventory
///   region this rung seeds.
/// * **The party.** Retail's usability gate omits a heal while every ally is at
///   full HP (`item_has_valid_target`), which is why a ladder that boots into a
///   town has no confirmable row and its Use confirm is a legitimate buzz. The
///   same save carries a lead record below max HP.
///
/// The scored value is the **preview word reaching the screen**. The two seeded
/// rows are both class-6 Waters, so every other input to the panel is equal
/// between them - same usability class, same target list, same party - and only
/// their effect arg differs. A host that dropped the word and always passed `0`
/// would compose the identical panel twice and tie here.
///
/// A note on picking the rows off the tables: **`is_usable_consumable` is not
/// enough**. The first field-usable class-0 record on this disc is a Ra-Seru
/// Meta, which the Items screen lists and then buzzes on, because the engine's
/// own use decoder classifies it as a key item. That is exactly why the confirm
/// below is scored on a **before/after** pair rather than against the command
/// window: a buzz leaves the item list byte-identical, and comparing to the
/// wrong reference would have scored the ordinary list-open as a panel.
#[test]
fn rung3_items_use_opens_the_target_panel_over_a_hurt_party() {
    let Some(disc_path) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc_path) else {
        eprintln!("[skip] disc image unreadable (disc-gated)");
        return;
    };
    // Pick the two bag rows off the disc's own tables rather than by id: two
    // **Waters whose preview words differ**. Both are class 6 permanent
    // stat-ups, so retail's usability gate treats them identically
    // (`effect_gated_by_target_state` is false for a stat-up) - the *only*
    // thing that can make their panels differ is the preview word.
    let scus = legaia_web_viewer::disc::extract_scus(&bytes).expect("SCUS on the disc");
    let effects =
        legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).expect("item-effect table");
    use legaia_engine_core::pause_screens::target_panel_mode;
    let previewing: Vec<(u8, u32)> = (0u8..=255)
        .filter_map(|id| {
            let e = effects.effect(id)?;
            let mode = target_panel_mode(effects.kind(id), e.class, e.tier);
            (mode != 0).then_some((id, mode))
        })
        .collect();
    let (id_a, mode_a) = previewing[0];
    let (id_b, mode_b) = *previewing
        .iter()
        .find(|&&(_, m)| m != mode_a)
        .expect("two Waters with different preview words");
    eprintln!("[w1l4] bag rows: {id_a:#04x} (mode {mode_a}), {id_b:#04x} (mode {mode_b})");
    let (plain, water) = (id_a, id_b);

    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.enter_field("town01").expect("enter town01");
    rt.insert_card(
        0,
        card_with_bag(1, 0, "Vahn", 500, (90, 200), &[(plain, 3), (water, 2)]),
        "A".into(),
    )
    .expect("insert");
    open_row(&mut rt, ROW_LOAD);
    rt.play_menu_input(CROSS);
    settle_card_read(&mut rt);
    rt.play_menu_input(CROSS);
    for _ in 0..8 {
        rt.play_menu_input(0);
    }
    unwind(&mut rt);

    let model = json(&rt.field_menu_model_json());
    eprintln!("[w1l4] after card load: {}", model);
    let lead = &model["party"][0];
    assert!(
        lead["hp"].as_i64() < lead["hp_max"].as_i64(),
        "the resumed lead must be hurt, or every heal row is correctly hidden"
    );
    let ids: Vec<u64> = model["items"]
        .as_array()
        .map(|a| a.iter().filter_map(|i| i["id"].as_u64()).collect())
        .unwrap_or_default();
    assert!(
        ids.contains(&(plain as u64)) && ids.contains(&(water as u64)),
        "the resumed bag lost the seeded rows: {ids:?}"
    );

    /// Open Items -> Use, walk `row` rows down the list, and return the
    /// composed draw list **immediately before and after** the row's confirm.
    ///
    /// The before/after pair is what makes this falsifiable: a buzzed confirm
    /// leaves the item list exactly where it was, so `before == after`, while a
    /// confirm that opens target select replaces the list with window 14.
    /// Comparing against the *command window* instead would score the ordinary
    /// list-open as a panel.
    fn use_row(rt: &mut LegaiaRuntime, row: usize) -> (serde_json::Value, serde_json::Value) {
        assert!(rt.play_menu_open_row("Items"), "Items opens");
        rt.play_menu_input(CROSS); // Use
        for _ in 0..row {
            rt.play_menu_input(DOWN);
        }
        let before = json(&rt.play_menu_draws_json(W, H));
        rt.play_menu_input(CROSS);
        let after = json(&rt.play_menu_draws_json(W, H));
        assert_eq!(after["open"], true, "the confirm closed the menu");
        (before, after)
    }

    let mut panels: Vec<(usize, serde_json::Value)> = Vec::new();
    for row in 0..ids.len() {
        let (before, after) = use_row(&mut rt, row);
        let n = |v: &serde_json::Value, k: &str| v[k].as_array().map(|a| a.len()).unwrap_or(0);
        eprintln!(
            "[w1l4] Use row {row}: list sprites={} texts={} -> confirm sprites={} texts={}",
            n(&before, "sprites"),
            n(&before, "texts"),
            n(&after, "sprites"),
            n(&after, "texts"),
        );
        if after != before {
            assert!(n(&after, "texts") > 0, "row {row}'s panel composed no text");
            assert!(
                n(&after, "sprites") > 0,
                "row {row}'s panel composed no chrome"
            );
            panels.push((row, after));
        }
        unwind(&mut rt);
    }
    assert!(
        panels.len() >= 2,
        "fewer than two bag rows opened a target panel ({} did), so the mode \
         contrast below cannot be drawn",
        panels.len()
    );
    // The two rows resolve to different preview words, so their panels must
    // not be the same draw list.
    assert_ne!(
        panels[0].1, panels[1].1,
        "two Waters with preview words {mode_a} and {mode_b} composed the \
         identical panel - the word never reached the screen"
    );
}

/// The preview word `target_panel_mode` (`FUN_801D6A54`) derives, checked
/// against the **disc's own** item tables rather than against a restatement of
/// the mapping.
///
/// The rung above is what reaches the kernel from a host; this is what pins its
/// answer. Only an item whose record kind byte is `2` **and** whose effect class
/// is `6` - the permanent-stat Waters - previews at all, and each one's effect
/// arg selects which panel: `0`/`5` the HP/MP panel, `1..=4` the four stat
/// panels. Everything else is the plain `cur/max` panel, mode `0`.
///
/// The falsifiable half is the second clause: a kernel that keyed on the effect
/// class alone would preview every class-6 record regardless of its kind byte,
/// and a kernel that keyed on the kind alone would preview every ordinary
/// equipment row.
#[test]
fn rung3b_the_target_panel_preview_word_matches_the_discs_item_tables() {
    let Some(rt) = loaded_in("town01") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    drop(rt);
    let disc = std::env::var("LEGAIA_DISC_BIN").expect("gated above");
    let bytes = std::fs::read(&disc).expect("disc readable");
    let scus = legaia_web_viewer::disc::extract_scus(&bytes).expect("SCUS on the disc");
    let effects =
        legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).expect("item-effect table");

    use legaia_engine_core::pause_screens::target_panel_mode;
    let mut previews = 0usize;
    let mut seen_modes = std::collections::BTreeSet::new();
    for id in 0u8..=255 {
        let Some(eff) = effects.effect(id) else {
            continue;
        };
        let kind = effects.kind(id);
        let mode = target_panel_mode(kind, eff.class, eff.tier);
        if mode != 0 {
            previews += 1;
            seen_modes.insert(mode);
            assert_eq!(kind, 2, "item {id:#04x} previews but is not kind 2");
            assert_eq!(eff.class, 6, "item {id:#04x} previews but is not class 6");
            assert!((1..=5).contains(&mode), "item {id:#04x} mode {mode}");
        }
        // The two independent clauses: neither the class nor the kind alone
        // may open a preview.
        if kind != 2 {
            assert_eq!(
                target_panel_mode(kind, eff.class, eff.tier),
                0,
                "item {id:#04x}: a non-kind-2 record must not preview"
            );
        }
        if eff.class != 6 {
            assert_eq!(
                target_panel_mode(kind, eff.class, eff.tier),
                0,
                "item {id:#04x}: a non-class-6 record must not preview"
            );
        }
    }
    eprintln!(
        "[w1l4] target-panel previews on this disc: {previews} item(s), modes {seen_modes:?}"
    );
    assert!(
        previews > 0,
        "the disc's item tables carry no previewing record at all - the \
         preview word would then be unreachable by construction"
    );
    assert!(
        seen_modes.len() > 1,
        "every previewing item resolved to the same panel ({seen_modes:?}), \
         so the effect-arg map is not being read"
    );
}

// ---------------------------------------------------------------------------
// Rungs 4-6 - the standalone minigame pages
// ---------------------------------------------------------------------------

/// The dance page's per-frame actor records and its hit sting.
///
/// `dance_actors_json` is `dance_clip_driver_gate` (`FUN_801D4098`) over the
/// spawned dancer pool: retail runs the shared clip driver only for an actor
/// whose `+0x5C` bound-clip word is positive, so a page that reported
/// `clip_driver: true` for every dancer would be describing a different gate.
///
/// `dance_sting_pcm` is `FUN_801D3D78` - the good-step sting, which bypasses
/// the cue ring and keys two voices itself; both layers must decode, and to
/// different samples.
#[test]
fn rung4_the_dance_page_gates_its_clip_driver_and_keys_its_sting() {
    let Some(mut mg) = minigames() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.dance_start(false), "the dance run starts off the disc");

    let mut judged = 0usize;
    let mut with_driver = 0usize;
    let mut seen_dancers = 0usize;
    for frame in 0..600u32 {
        mg.dance_tick(1);
        // Press on the beat the chart asks for; a miss is still a judge.
        if frame.is_multiple_of(8) {
            let verdict = mg.dance_press((frame / 8 % 4) as u8);
            if verdict != "ignored" {
                judged += 1;
            }
        }
        let actors = json(&mg.dance_actors_json());
        let dancers = actors["dancers"].as_array().cloned().unwrap_or_default();
        seen_dancers = seen_dancers.max(dancers.len());
        for d in &dancers {
            if d["clip_driver"] == true {
                with_driver += 1;
            }
            // Every record the gate emits must carry the clip it gated on.
            assert!(d["clip"].is_number(), "a dancer record with no clip id");
            assert!(d["rate"].is_number(), "a dancer record with no clip rate");
        }
    }
    assert!(seen_dancers > 0, "the page spawned no dancers");
    assert!(judged > 0, "no press was judged - the beat clock never ran");
    assert!(
        with_driver > 0,
        "the clip-driver gate never opened for any dancer"
    );

    // The sting: `r` picks a pair of tones (`2r`, `2r + 1`) in program 1 and a
    // note of `0x3c + r`. Both layers must resolve, and the variant space must
    // actually vary - in particular the tier variant `5` the three groovy
    // moves key is a different sting from the tier-2 random pick, and anything
    // enumerating only `0..3` misses it.
    let mut keyed = Vec::new();
    for r in 0..=legaia_engine_core::dance::STING_TIER_VARIANT as u8 {
        let a = mg.dance_sting_pcm(r, 0);
        let b = mg.dance_sting_pcm(r, 1);
        if a.is_empty() && b.is_empty() {
            continue;
        }
        assert!(
            !a.is_empty() && !b.is_empty(),
            "sting r={r} keyed only one of its two voices"
        );
        let (ra, rb) = (mg.dance_sting_rate(r, 0), mg.dance_sting_rate(r, 1));
        assert!(ra > 0 && rb > 0, "sting r={r} decoded at rate 0");
        keyed.push((r, a, ra));
    }
    assert!(!keyed.is_empty(), "no sting variant resolved off the bank");
    assert!(
        keyed
            .iter()
            .any(|&(r, _, _)| r == legaia_engine_core::dance::STING_TIER_VARIANT as u8),
        "the groovy-move tier variant r={} did not resolve - the bank was \
         enumerated over the random space only",
        legaia_engine_core::dance::STING_TIER_VARIANT
    );
    let distinct: std::collections::BTreeSet<_> = keyed
        .iter()
        .map(|(_, pcm, rate)| (pcm.len(), *rate))
        .collect();
    assert!(
        distinct.len() > 1,
        "every sting variant decoded the same (length, rate) pair {distinct:?} \
         - `r` is not selecting a tone"
    );
}

/// The Baka Fighter page's HUD widget quad (`FUN_801D5ED0`).
///
/// ## One-host waiver
///
/// **Missing host: the native `play-window`.** This is deliberate, and the
/// reason is upstream of the kernel rather than in it. `hud_widget_quad` hands
/// back a `HudWidgetQuad` - four projected corners, a UV span, two vertex
/// shades and a texpage attribute - which is a *packet*, and the native window
/// has no textured-quad surface for the duel HUD to draw one into: its Baka HUD
/// is `window/hud.rs`'s digit strips, and where it does need a widget's texture
/// column it goes through `baka_fighter_chrome::glyph_u` and emits a
/// `ChromeDraw` with the same `(widget, x, y, brightness, size)` arguments. So
/// the two hosts share the descriptor table and the argument tuple and diverge
/// only at the emit, which is the level a renderer difference belongs at.
///
/// Closing it is a renderer change, not a wire: the native window would need a
/// textured-quad path for minigame chrome. Until it has one, routing it through
/// this kernel would mean building a packet nothing consumes - a fake wire, and
/// worse than the gap it hides.
///
/// The **browser** side is real and is what this rung drives: `minigames_baka`'s
/// `baka_hud_quad_json` is the page's only HUD-widget surface and it calls this
/// kernel directly.
///
/// The emitter's arithmetic is what is scored: the span is
/// `x - hw ..= x + hw - 1` (so the width is odd-symmetric about `x`), the UVs
/// cover the cell inclusively, `mirror` swaps the texture columns without
/// moving the quad, and each colour channel is `channel * brightness >> 8`.
#[test]
fn rung5_the_baka_page_emits_its_hud_widget_quad() {
    let Some(mg) = minigames() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(
        mg.baka_presentation_ready(),
        "the Baka widget table + art pack must decode off the disc"
    );

    let base = json(&mg.baka_hud_quad_json(0, 160, 32, 0x80, 0x1000, false));
    assert!(base.is_object() && !base.as_object().unwrap().is_empty());
    let x0 = base["x0"].as_i64().unwrap();
    let x1 = base["x1"].as_i64().unwrap();
    let y0 = base["y0"].as_i64().unwrap();
    let y1 = base["y1"].as_i64().unwrap();
    assert!(x1 > x0 && y1 > y0, "a degenerate quad: {base}");
    // The quad is centred on the caller's x, spanning `-hw ..= hw - 1`.
    assert_eq!(x0 + x1, 2 * 160 - 1, "the span is not centred on x: {base}");
    assert_eq!(y0 + y1, 2 * 32 - 1, "the span is not centred on y: {base}");
    let u0 = base["u0"].as_i64().unwrap();
    let u1 = base["u1"].as_i64().unwrap();
    assert!(u1 >= u0, "inclusive UV span: {base}");

    // Half the size is half the span, through the retail shifts.
    let half = json(&mg.baka_hud_quad_json(0, 160, 32, 0x80, 0x0800, false));
    let half_w = half["x1"].as_i64().unwrap() - half["x0"].as_i64().unwrap();
    assert!(
        half_w < x1 - x0,
        "halving the size scale did not narrow the quad ({half_w} vs {})",
        x1 - x0
    );

    // Mirroring swaps the texture columns and leaves the geometry alone.
    let mirrored = json(&mg.baka_hud_quad_json(0, 160, 32, 0x80, 0x1000, true));
    assert_eq!(mirrored["x0"], base["x0"], "mirror moved the quad");
    assert_eq!(mirrored["x1"], base["x1"], "mirror moved the quad");
    assert_eq!(mirrored["mirror"], true);

    // Brightness modulates the shade, not the geometry.
    let dim = json(&mg.baka_hud_quad_json(0, 160, 32, 0x20, 0x1000, false));
    assert_eq!(dim["x0"], base["x0"], "brightness moved the quad");
    let bright_top = base["rgb_top"].as_array().unwrap()[0].as_i64().unwrap();
    let dim_top = dim["rgb_top"].as_array().unwrap()[0].as_i64().unwrap();
    assert!(
        dim_top < bright_top,
        "a quarter brightness must darken the shade ({dim_top} vs {bright_top})"
    );

    // An id past the table is an empty payload, not a panic or a stub quad.
    assert_eq!(mg.baka_hud_quad_json(999, 0, 0, 0x80, 0x1000, false), "{}");
}

/// The Muscle Dome page's player fighter, built through the battle-load stat
/// init (`FUN_80053CB8`).
///
/// The scored value is `stats_source`: the page falls back to documented
/// constants when SCUS does not resolve, and the fallback path does **not**
/// run the kernel. A rung that only checked "a contest started" would pass on
/// the fallback and report the row as entered while it was not.
#[test]
fn rung6_the_muscle_page_builds_its_fighter_through_battle_load_stat_init() {
    let Some(mut mg) = minigames() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let roster = json(&mg.muscle_roster_json());
    let monster = roster["opponents"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|o| o["id"].as_u64())
        .unwrap_or(1) as u16;

    assert!(
        mg.muscle_start_vs(0, 30, monster, 0x2A),
        "the dome contest starts off the disc's tables"
    );
    let st = json(&mg.muscle_state_json());
    assert_eq!(
        st["source"], "disc",
        "the fighter came from the fallback constants, so the battle-load \
         stat init never ran: {st}"
    );
    let hp30 = st["hp"].as_array().and_then(|a| a[0].as_i64()).unwrap_or(0);
    assert!(hp30 > 0, "the player fighter has no HP: {st}");

    // The kernel folds the growth curve, so a higher level must produce a
    // bigger fighter - a stat block that ignored the record would tie.
    assert!(mg.muscle_start_vs(0, 60, monster, 0x2A));
    let st60 = json(&mg.muscle_state_json());
    let hp60 = st60["hp"]
        .as_array()
        .and_then(|a| a[0].as_i64())
        .unwrap_or(0);
    assert!(
        hp60 > hp30,
        "levelling the record did not move the battle-load stat block \
         ({hp30} at Lv30 vs {hp60} at Lv60)"
    );
    assert_eq!(st60["source"], "disc");
}
