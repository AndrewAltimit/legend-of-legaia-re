//! `legaia-engine load --card`: reading a save out of a real PSX memory-card
//! image on the native host, the way the browser play page already imports
//! one.
//!
//! **Why this ladder spawns the binary.** Everything under
//! `crates/engine-shell/src/bin/legaia-engine/` is the native host's own
//! composition layer, and no integration test links against a `bin/` target -
//! so the subcommand is reached by running it, exactly as
//! `w5_native_minigame_ladder` reaches the window. The mounted-card model the
//! save screen's rack is built on is unit-tested beside it, in
//! `window/save_select_helpers.rs`, where it can be called.
//!
//! **No disc, no real card.** Every card here is composed in memory from the
//! `legaia_save` writers: an `MC` header, one claimed directory frame, and a
//! block written by the same composer the engine's own card Save uses. The
//! ladder passes with `LEGAIA_DISC_BIN` unset because nothing in it reads a
//! disc.

use std::path::Path;
use std::process::Command;

use legaia_save::{CharacterRecord, HpMpSp, Party, SaveExt, SaveFile, SaveResume};

/// The block a synthetic card claims. Deliberately not block 1: a reader that
/// assumed the first block, or that confused the block with its grid cell,
/// would still pass against a one-block card.
const CLAIMED_BLOCK: u8 = 3;

/// The save the claimed block carries.
fn a_save() -> SaveFile {
    let mut r = CharacterRecord::zeroed();
    r.set_name("Noa");
    r.set_magic_rank(23);
    r.set_hp_mp_sp(HpMpSp {
        hp_cur: 210,
        hp_max: 240,
        mp_cur: 11,
        mp_max: 30,
        sp_cur: 0,
        sp_max: 0,
    });
    let mut story_flag_bits = vec![0u8; legaia_save::card::RETAIL_STORY_FLAGS_SIZE];
    story_flag_bits[..4].copy_from_slice(&0x0000_1234u32.to_le_bytes());
    SaveFile {
        party: Party { members: vec![r] },
        ext: SaveExt {
            story_flags: 0x0000_1234,
            story_flag_bits,
            money: 4321,
            inventory: vec![(3, 2), (9, 1)],
            ..SaveExt::default()
        },
        ..SaveFile::default()
    }
}

fn a_resume() -> SaveResume {
    SaveResume {
        scene: "town01".into(),
        location: "Rim Elm".into(),
    }
}

/// A raw `.mcr` image built in memory: the `MC` header, one block claimed for
/// a Legaia save, and that block composed by `SaveFile` /
/// `SaveResume`'s own retail-block writers. Every other frame stays zero, so
/// no other block reads as a chain start.
fn synthetic_card(block: u8) -> Vec<u8> {
    let mut card = vec![0u8; legaia_save::card::CARD_SIZE];
    card[..2].copy_from_slice(&legaia_save::card::CARD_MAGIC);
    let view = legaia_save::emu::detect(&card).expect("the MC header makes it a raw card");
    view.claim_block(&mut card, block, "BASCUS-94254PRO-00")
        .expect("the block is addressable");
    let sc = view
        .sc_block_mut(&mut card, block)
        .expect("the block is addressable");
    a_save()
        .write_into_retail_sc_block(sc)
        .expect("compose the block");
    a_resume()
        .write_into_retail_sc_block(sc)
        .expect("stamp the resume point");
    card
}

/// Run `legaia-engine` with `args` and hand back `(success, stdout, stderr)`.
fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_legaia-engine"))
        .args(args)
        .output()
        .expect("spawn legaia-engine");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write_card(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write the synthetic card");
    path
}

/// The parenthesised world summary a `load` line ends in - everything after
/// the carrier it came out of.
fn world_summary(stdout: &str) -> String {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("loaded "))
        .unwrap_or_else(|| panic!("no load summary in: {stdout}"));
    let at = line
        .find(" (")
        .unwrap_or_else(|| panic!("summary has no world tail: {line}"));
    line[at + 1..].to_string()
}

/// The header line: a card block loads into a world, and the world it loads is
/// the **same world** the save-directory path loads from the same save.
///
/// Comparing the tails rather than a list of literals is what keeps this
/// honest across a model change: whatever the summary counts, both carriers
/// have to count it the same, so a card lift that silently dropped the money
/// or half the party fails here even if the field names move.
#[test]
fn a_card_block_loads_the_same_world_as_the_save_directory() {
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    let dir = tempfile::tempdir().unwrap();
    let card = write_card(dir.path(), "player.mcr", &synthetic_card(CLAIMED_BLOCK));
    std::fs::write(
        dir.path().join(format!("slot_00.{SAVE_EXT}")),
        a_save().write(),
    )
    .unwrap();

    let (ok, from_card, stderr) = run(&[
        "load",
        "--card",
        card.to_str().unwrap(),
        "--block",
        &CLAIMED_BLOCK.to_string(),
    ]);
    assert!(
        ok,
        "load --card failed\nstdout: {from_card}\nstderr: {stderr}"
    );
    assert!(
        from_card.contains(&format!("loaded block {CLAIMED_BLOCK} from")),
        "summary names the wrong carrier: {from_card}"
    );
    // Non-vacuous on its own: these are the save's own values, not defaults.
    for field in ["party=1", "story_flags=0x00001234", "money=4321"] {
        assert!(
            from_card.contains(field),
            "summary is missing {field}: {from_card}"
        );
    }

    let (ok, from_dir, stderr) = run(&[
        "load",
        "--save-dir",
        dir.path().to_str().unwrap(),
        "--slot",
        "0",
    ]);
    assert!(ok, "load failed\nstdout: {from_dir}\nstderr: {stderr}");
    assert_eq!(
        world_summary(&from_card),
        world_summary(&from_dir),
        "the card block and the slot file hold the same save and must load the same world"
    );
}

/// `--block` is optional: with none given the card's own directory picks the
/// block. A card files its saves wherever the BIOS placed them, so the block
/// number is not something a player should have to know.
#[test]
fn an_absent_block_takes_the_one_the_card_files() {
    let dir = tempfile::tempdir().unwrap();
    let card = write_card(dir.path(), "player.mcr", &synthetic_card(CLAIMED_BLOCK));

    let (ok, stdout, stderr) = run(&["load", "--card", card.to_str().unwrap()]);
    assert!(ok, "load --card failed\nstdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains(&format!("loaded block {CLAIMED_BLOCK} from")),
        "the directory walk picked the wrong block: {stdout}"
    );
}

/// A file that is not a container `legaia_save::emu` recognises must **fail**.
/// An unmounted port reads empty, so a wrong file accepted quietly would look
/// like a card with nothing on it rather than like the mistake it is.
#[test]
fn an_unrecognised_container_fails_instead_of_loading_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let junk = write_card(dir.path(), "not-a-card.bin", b"this is not a memory card");

    let (ok, stdout, stderr) = run(&["load", "--card", junk.to_str().unwrap()]);
    assert!(!ok, "an unrecognised container must not load: {stdout}");
    assert!(
        stderr.contains("unrecognised save container"),
        "the failure must name the container problem: {stderr}"
    );
    assert!(
        !stdout.contains("loaded"),
        "nothing may be reported loaded: {stdout}"
    );
}

/// Block 0 is the card directory, never a save. Asking for it is a range
/// error rather than a lift of the directory frames.
#[test]
fn block_zero_is_the_directory_and_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let card = write_card(dir.path(), "player.mcr", &synthetic_card(CLAIMED_BLOCK));

    let (ok, _, stderr) = run(&["load", "--card", card.to_str().unwrap(), "--block", "0"]);
    assert!(!ok, "block 0 must be refused");
    assert!(
        stderr.contains("out of range"),
        "the failure must name the range: {stderr}"
    );
}

/// A block the card never claimed holds no save, and the load says so rather
/// than seeding an empty world under a success line.
#[test]
fn an_unclaimed_block_is_not_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let card = write_card(dir.path(), "player.mcr", &synthetic_card(CLAIMED_BLOCK));

    let (ok, stdout, stderr) = run(&["load", "--card", card.to_str().unwrap(), "--block", "7"]);
    assert!(!ok, "an unclaimed block must not load: {stdout}");
    assert!(
        stderr.contains("no character records"),
        "the failure must name what the block lacks: {stderr}"
    );
}

/// The card path is additive: with no `--card`, `load` is still the
/// save-directory round trip it has always been.
#[test]
fn without_a_card_the_save_directory_path_is_unchanged() {
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(format!("slot_00.{SAVE_EXT}")),
        a_save().write(),
    )
    .unwrap();

    let (ok, stdout, stderr) = run(&[
        "load",
        "--save-dir",
        dir.path().to_str().unwrap(),
        "--slot",
        "0",
    ]);
    assert!(ok, "load failed\nstdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains("loaded slot 0 from"),
        "the save-directory summary changed shape: {stdout}"
    );
    assert!(stdout.contains("money=4321"), "{stdout}");
}
