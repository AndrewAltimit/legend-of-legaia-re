//! The battle HUD's retail sub-draw step table walk. Split out of
//! `battle_hud.rs`; no logic change.

// ---------------------------------------------------------------------------
// The sub-draw script table - retail's own per-step element lists
// ---------------------------------------------------------------------------

/// VA of `PTR_DAT_801F4D34`, the per-step sub-draw script pointer table in
/// the battle overlay (PROT 0898) that `FUN_801D388C(step)` indexes.
pub const SUBDRAW_PTR_TABLE_VA: u32 = 0x801F_4D34;

/// Steps the table holds - `FUN_801D388C`'s `sltiu v0,s8,0x32` bound.
pub const SUBDRAW_STEP_COUNT: usize = 0x32;

/// The `FUN_801D388C` steps the predicates above encode, by the menu-SM
/// transition that runs them (`ghidra/scripts/funcs/overlay_0898_801d0748.txt`).
pub mod subdraw_steps {
    /// `0x14` round start -> `0x1E` round prompt (`0x801D0EE4`).
    pub const ROUND_PROMPT: usize = 0x00;
    /// `0x1E` confirm -> `0x28` ring (`0x801D109C`).
    pub const RING: usize = 0x01;
    /// `0x28` -> `0x3C` item window (`0x801D13F0`).
    pub const ITEM_WINDOW: usize = 0x05;
    /// `0x28` -> `0x46` magic window (`0x801D14C8`).
    pub const MAGIC_WINDOW: usize = 0x07;
    /// `0x28` -> `0x50` arts command entry (`0x801D1658`).
    pub const ARTS_INPUT: usize = 0x09;
    /// `0x3C` -> `0x64` item target step (`0x801D1958`).
    pub const ITEM_TARGET_OPEN: usize = 0x12;
    /// `0x64` cursor move: the bar re-pointed at the pointed member
    /// (`0x801D2D1C`).
    pub const ITEM_TARGET_CURSOR: usize = 0x18;
    /// `0x46` -> magic target step (`0x801D2B74`).
    pub const MAGIC_TARGET: usize = 0x1B;
    /// `0x28` -> `0x6E` all members committed (`0x801D16D8`).
    pub const ALL_COMMITTED: usize = 0x23;
    /// `0x78` Auto -> `0x5A` target cursor (`0x801D179C`).
    pub const TARGET_CURSOR: usize = 0x2D;
    /// `0x28` Attack -> `0x78` attack-mode prompt (option `Select`): the
    /// `li v0,0x78` at `0x801D1604` jumps to the shared `jal 0x801d388c` at
    /// `0x801D31D8` with `a0 = 0x2A` in the delay slot. Its script opens the
    /// `Auto` / `Command` chips (records `0x55` / `0x54`) and snaps the bar and
    /// the plate (`7/3`, `0x52/3`).
    pub const ATTACK_MODE: usize = 0x2A;
    /// `0x28` Attack -> `0x5A` target cursor under the `Automatic` option,
    /// skipping the prompt (`li a0,0x30` at `0x801D1614`, `jal` at
    /// `0x801D161C`, `0x5A` stored in its delay slot). An earlier label here
    /// called this step the attack-mode prompt; the `0x5A` store and its
    /// target plaque (`0x29/0`) say otherwise.
    pub const AUTOMATIC_TARGET: usize = 0x30;
}

/// Screen-element placement records (`0x80076C10 + id * 0x18`) the battle
/// HUD's surfaces live in - the `elem_id` `FUN_801D8DE8` takes.
pub mod placement_record {
    /// The `Begin` chip on its way to the breadcrumb tab seat `(16, 14)`.
    pub const BEGIN_TAB: u8 = 0x01;
    /// The first member's roster panel; 78 / 79 are the second / third.
    pub const PANEL: [u8; 3] = [0x06, 0x4E, 0x4F];
    /// The full-width active-actor bar.
    pub const BAR: u8 = 0x07;
    /// The ring's element (magic) chip.
    pub const MAGIC_CHIP: u8 = 0x0A;
    /// The AP bar the arts-entry screen and the Spirit action raise.
    pub const AP_BAR: u8 = 0x0F;
    /// The plaque behind the `Begin` tab, at `(68, 14)`.
    pub const PLAQUE_BEHIND_TAB: u8 = 0x1A;
    /// The plaque on its action seat `(16, 14)`.
    pub const PLAQUE: u8 = 0x44;
    /// The move-name label (77 is its twin).
    pub const MOVE_NAME: u8 = 0x4C;
    /// The combo counter cluster's anchor.
    pub const COMBO: u8 = 0x50;
    /// The bottom-right target plaque.
    pub const TARGET_PLAQUE: u8 = 0x51;
    /// The ring's AP plate.
    pub const AP_PLATE: u8 = 0x52;
}

/// One decoded sub-draw step: `[count][anim][panel]` + `count` x
/// `(record, mode)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubdrawStep {
    /// `1` / `3` hard-reset the handle list before the pairs run
    /// (`FUN_801D99BC`), `2` tears the sprites down (`FUN_801D9AE8`), `0`
    /// leaves the list alone.
    pub anim: u8,
    /// The `ctx[+0x275]` panel id the step installs (how many leading
    /// pairs bind to the direction slots).
    pub panel: u8,
    /// `(placement record, mode)` in run order.
    pub pairs: Vec<(u8, u8)>,
}

impl SubdrawStep {
    /// The mode `record` is opened with in this step, or `None` when the
    /// step does not touch it.
    pub fn mode_of(&self, record: u8) -> Option<u8> {
        self.pairs
            .iter()
            .find(|(r, _)| *r == record)
            .map(|(_, m)| *m)
    }

    /// Does the step leave `record` on screen? Mode bit 0 clear spawns the
    /// element at its seat A and glides it to seat B - the on-screen seat
    /// for every battle-HUD record listed in [`placement_record`] - while
    /// bit 0 set does the reverse. Bit 1 only suppresses the glide.
    pub fn shows(&self, record: u8) -> bool {
        self.mode_of(record).is_some_and(|m| m & 1 == 0)
    }
}

/// Decode step `step` of the sub-draw table out of a **loaded** battle
/// overlay image (`legaia_asset::static_overlay::as_loaded`, so `image[0]`
/// is `base_va`). `None` when the table or the record falls outside the
/// image.
pub fn subdraw_step(image: &[u8], base_va: u32, step: usize) -> Option<SubdrawStep> {
    if step >= SUBDRAW_STEP_COUNT {
        return None;
    }
    let at = |va: u32| -> Option<usize> { va.checked_sub(base_va).map(|o| o as usize) };
    let slot = at(SUBDRAW_PTR_TABLE_VA)? + step * 4;
    let ptr = u32::from_le_bytes(image.get(slot..slot + 4)?.try_into().ok()?);
    let rec = at(ptr)?;
    let head = image.get(rec..rec + 3)?;
    let count = usize::from(head[0]);
    let body = image.get(rec + 3..rec + 3 + count * 2)?;
    Some(SubdrawStep {
        anim: head[1],
        panel: head[2],
        pairs: body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| (c[0], c[1]))
            .collect(),
    })
}
