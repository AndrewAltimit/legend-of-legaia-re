//! Disc-gated: a Muscle Dome round is an **ordinary battle**, so a dome leg
//! ends on a knockout and nothing bounds it by turns.
//!
//! This is the negative result the port's dome session now encodes. It was
//! twice modelled wrongly (first as a card battle, then as a four-turn
//! fight), the second time by reading the formation cell `0x8007BD0C` as a
//! battle-type byte. So the claim is pinned against the bytes rather than
//! restated:
//!
//! 1. **The arena's only way out is an ordinary battle.** The opponent
//!    installer `FUN_801D1510` is the arena overlay's sole write of the
//!    global game-mode word, and it writes [`GameMode::BattleInit`]. There is
//!    no arena-resident battle loop to carry a turn limit.
//! 2. **The arena stages one monster into the shared formation cell.** The
//!    arena overlay holds exactly one store to `0x8007BD0C`; the battle
//!    overlay holds none and only reads it. So the cell is an input to the
//!    battle, not a mode selector the arena could branch a timeout on.
//! 3. **No dome round can raise the timed-fight HUD.** Every course-ladder
//!    monster id is strictly below [`md::TIMED_FIGHT_MONSTER_ID`], the id the
//!    `Turns Left / HP Left` strip's draw sites gate on.
//!
//! No Sony bytes are asserted - only instruction *shapes* and counts, and the
//! ordering of ids. Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::mode::GameMode;
use legaia_engine_core::muscle_dome as md;

/// Battle-action overlay: the image that draws the timed-fight strip and
/// owns the battle round driver. Loads at the same base as the arena's.
const BATTLE_OVERLAY_PROT_INDEX: u32 = 898;

/// `0x8007BD0C`, the four-slot monster-id formation cell. Reachable as
/// `lui 0x8008` + `-0x42F4` or `lui 0x8007` + `+0xBD0C`; both encode the same
/// low halfword, so one immediate matches either form.
const FORMATION_CELL_IMM: u16 = 0xBD0C;

/// `0x8007B83C`, the global game-mode word, by the same argument.
const GAME_MODE_IMM: u16 = 0xB83C;

fn prot_entry(index: u32) -> Option<Vec<u8>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    host.index.entry_bytes_extended(index).ok()
}

/// One matched memory reference: its VA, whether it stores, and the immediate
/// most recently loaded into the stored register (`addiu rt, zero, N`).
struct Ref {
    va: u32,
    stores: bool,
    stored_imm: Option<u16>,
}

/// Every load/store whose displacement is `imm` off a register a preceding
/// `lui` set to `0x8007` / `0x8008` - i.e. every direct reference to the
/// global that immediate names.
///
/// The `lui` is tracked across the whole image rather than within a fixed
/// window: retail separates the pair by up to eight instructions here, and a
/// short window silently misses the write this test exists to find.
fn refs_to(entry: &[u8], base: u32, imm: u16) -> Vec<Ref> {
    let mut lui = [None; 32];
    let mut li = [None; 32];
    let mut out = Vec::new();
    for off in (0..entry.len().saturating_sub(3)).step_by(4) {
        let w = u32::from_le_bytes(entry[off..off + 4].try_into().unwrap());
        let (op, rs, rt, i) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31, (w & 0xFFFF) as u16);
        match op {
            // lui rt, imm
            0x0F => lui[rt as usize] = Some(i),
            // addiu rt, zero, N - the value a following store writes
            0x09 if rs == 0 => li[rt as usize] = Some(i),
            _ => {}
        }
        let is_store = matches!(op, 0x28 | 0x29 | 0x2B);
        let is_load = matches!(op, 0x20 | 0x21 | 0x23 | 0x24 | 0x25);
        if (is_store || is_load) && i == imm && matches!(lui[rs as usize], Some(0x8007 | 0x8008)) {
            out.push(Ref {
                va: base + off as u32,
                stores: is_store,
                stored_imm: li[rt as usize],
            });
        }
    }
    out
}

#[test]
fn dome_round_hands_off_to_an_ordinary_battle_and_ends_on_a_ko() {
    let (Some(arena), Some(battle)) = (
        prot_entry(md::ARENA_OVERLAY_PROT_INDEX as u32),
        prot_entry(BATTLE_OVERLAY_PROT_INDEX),
    ) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let base = md::ARENA_OVERLAY_BASE_VA;

    // 1. The arena's only game-mode write hands the round to BattleInit.
    let mode_writes: Vec<Ref> = refs_to(&arena, base, GAME_MODE_IMM)
        .into_iter()
        .filter(|r| r.stores)
        .collect();
    assert_eq!(
        mode_writes.len(),
        1,
        "the arena overlay has exactly one game-mode write, found {}",
        mode_writes.len()
    );
    let mode = mode_writes[0]
        .stored_imm
        .expect("mode write stores a literal");
    assert_eq!(
        GameMode::from_index(mode as usize),
        Some(GameMode::BattleInit),
        "the arena hands its round to an ordinary battle, not to a loop of its own \
         (write at {:#010x} stores {mode:#x})",
        mode_writes[0].va
    );

    // 2. The arena stages the opponent into the shared formation cell; the
    //    battle overlay only ever reads that cell.
    let arena_cell = refs_to(&arena, base, FORMATION_CELL_IMM);
    let battle_cell = refs_to(&battle, base, FORMATION_CELL_IMM);
    assert_eq!(
        arena_cell.iter().filter(|r| r.stores).count(),
        1,
        "one arena writer installs the round's monster"
    );
    let battle_writes: Vec<u32> = battle_cell
        .iter()
        .filter(|r| r.stores)
        .map(|r| r.va)
        .collect();
    assert!(
        battle_writes.is_empty(),
        "the battle overlay never writes the formation cell - it is an input, \
         but found writes at {battle_writes:#010x?}"
    );
    assert!(
        battle_cell.iter().any(|r| !r.stores),
        "the battle overlay reads the formation cell"
    );

    // 3. The timed-fight strip is out of the dome's reach: its gate wants
    //    formation slot 0 == TIMED_FIGHT_MONSTER_ID, and no ladder round
    //    installs an id that high.
    let ladder = md::parse_course_ladder(&arena).expect("the course ladder decodes off the disc");
    let top = ladder
        .iter()
        .flat_map(|c| c.rounds.iter())
        .map(|r| r.monster_id)
        .max()
        .expect("the ladder has rounds");
    assert!(
        top < md::TIMED_FIGHT_MONSTER_ID,
        "ladder tops out at {top:#x}, below the timed fight's {:#x}",
        md::TIMED_FIGHT_MONSTER_ID
    );
}
