//! The battle party panels' anchors, the cross-out mark, the arts-list
//! text actor, and the battle-result messages.
//!
//! REF: FUN_801DBB8C (label-actor open) -> [`LabelState::opened`]
//! REF: FUN_801DBC30 (cross-out mark blit) -> [`cross_out_mark`]
//! REF: FUN_801D84C0 (result messages + panel publish + teardown) -> [`result_subject`]
//!
//! Each `PORT` tag sits on the routine it names rather than here. At module
//! scope the reach report's anchor fallback resolves all three to *the next
//! function in the file* - [`name_field_ptr`], a pointer-arithmetic
//! one-liner - so one call to that helper would have read as all three
//! battle-overlay leaves having run.
//!
//! Three battle-overlay leaves that share one piece of state - the eight-byte
//! block at `0x801F4E08` - and turn out to be a matched set: `FUN_801DBB8C`
//! **opens** the label actor and stashes its handle at `0x801F4E0C`, and
//! `FUN_801D84C0` **clears** exactly that block (`+0x01`, `+0x02` and the handle
//! word) as its last act. That pairing is what fixes them as one subsystem
//! rather than three unrelated bodies.
//!
//! Every byte here comes from a disassembly of the mapped `0898` image at the
//! base [`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml)
//! records, **not** from the corpus dumps - two of the three have dumps at the
//! same VA that cannot both be right:
//!
//! | VA | `overlay_0897` dump | battle-action image |
//! |---|---|---|
//! | `801DBB8C` | a 4-instruction label-call slice leaving via `j 0x801EA7AC` | 41 instructions, own frame, `jr ra` |
//! | `801DBC30` | mis-based | 53 instructions, own frame, `jr ra` |
//! | `801D84C0` | 212 instructions | 259 instructions |
//!
//! ```bash
//! scripts/ghidra-analysis/disasm-overlay-fn.py \
//!     extracted/overlays/overlay_battle_action_0898.bin \
//!     --base 0x801CE818 --addr 0x801d84c0
//! ```
//!
//! ## The name pointer confirms the save record
//!
//! Both `FUN_801D84C0` arms resolve a party member's name as
//! `0x8008459B + id * 0x414`. That is exactly
//! `0x80084708 + (id - 1) * 0x414 + 0x2A7` - the live character record's
//! **display name** field, at the offset
//! [`save-record.md`](../../docs/formats/save-record.md) documents. An
//! independent arrival at `+0x2A7` from battle code, and the reason
//! [`name_field_ptr`] is expressed against the record base rather than against
//! the magic constant.
//!
//! ## Panel anchors are centre-weighted
//!
//! The per-party-size X anchors below put a solo member in the middle and split
//! a pair to the outsides - the same centring rule
//! [`field_party_cursor`](crate::field_party_cursor) found in the field VM's
//! member picker. Two independent subsystems agreeing on the layout convention.
//!
//! # Wiring status
//!
//! Split by what each piece needs, because the three leaves do not share a
//! blocker after all.
//!
//! **Wired.** The per-party-size X anchors ([`panel_anchors`]) are on the
//! production path: `engine-ui`'s `party_panel_stage_x` reads them directly
//! for the roster panels' name pens, and both battle-HUD hosts (the native
//! `play-window` and the browser play page) build their draw lists through
//! that one builder (`battle_hud_draws_for`). No mirrored literals remain;
//! an `engine-shell` test asserts the production function returns this
//! kernel's values. A packet walk of retail's own display list confirms
//! the anchors and says what they anchor - they are the panel's
//! **name pen**, five pixels inside a 102x48 panel plate, not the plate's
//! own edge ([`crate::battle_chrome::panel_seats`]).
//!
//! **Wired, and re-read.** [`result_subject`] is the port of
//! `FUN_801D84C0`'s two build arms, and the four buffers they fill are the
//! **battle-result messages** - victory with its spoils sentence, defeat,
//! escaped, escape failed - not panel labels, which is what this module
//! called them until the six pool strings the arms copy were resolved. The
//! engine's post-battle report words its victory line from it on both play
//! hosts (`engine-ui::battle_spoils_windows`).
//!
//! **Mis-attributed, now corrected.** [`cross_out_mark`] was read as the
//! panels' name-plate blit and `engine-ui` drew a filled rect at its
//! geometry. Resolving its two texture constants shows it samples the `etim`
//! effect page's red cross-out X instead, so the plate under a battle name
//! has no retail source at that rect at all - the real name plates are the
//! system-UI sheet's 3-slice runs in [`crate::battle_chrome`].
//!
//! **No engine analogue: the text-actor handle.** `FUN_801DBB8C` opens a SCUS
//! *text actor* (`FUN_8003541C` register-and-draw) and stashes its handle at
//! `0x801F4E0C`, and `FUN_801D84C0` closes the handle as its last act. (The
//! string helpers `FUN_801D84C0` also calls - `FUN_8003CA78` copy,
//! `FUN_8003CAC4` append, `FUN_8003CBF8` escape locator - write its own
//! result buffers in the battle context, not that actor.)
//! That is a retained-mode registry: the caller hands a string to a
//! persistent object which redraws itself every frame until torn down.
//! `engine-ui` is immediate-mode - `battle_hud_draws_for` rebuilds every
//! `TextDraw` from the live model each frame - so there is no object to open,
//! no handle to store, and nothing for the teardown to clear. [`LabelState`]
//! and [`OPEN_LAYOUT_ARGS`] therefore stay a documented record of the retail
//! lifecycle, not a port waiting on a caller: wiring them would mean adding a
//! retained text-actor layer the port has deliberately not got.
//!
//! **Still open.** The result text itself is the pool's, and the port does
//! not replay it: the report sentence is built from typed state, so a
//! translation that lifts the `0898` pool does not reach it, and the defeat
//! and escape messages have no engine caption at all. The layout arguments
//! `FUN_801DBB8C` passes the register call encode the panel band's
//! *vertical* placement, which is why `engine-ui`'s panel Y is still an
//! approximation.

/// Battle-context byte offsets of the four label buffers `FUN_801D84C0` fills.
pub const LABEL_BUFFERS: [usize; 4] = [0xA9, 0x129, 0x159, 0x189];

/// Battle-context offsets `FUN_801D84C0` publishes into the screen-element
/// placement table.
pub const PUBLISHED_BUFFERS: [(usize, usize); 4] = [
    (0x62C, 0xA9),
    (0x644, 0x129),
    (0x65C, 0x159),
    (0x86C, 0x1F9),
];

/// The screen-element placement table the panels are published into
/// (`0x80076C10`). Named "pose-slot table" here once, one of three readings
/// of the same base; `docs/reference/memory-map.md` settles it - a record is a
/// placement, and only the pose reading is contradicted by the layout.
pub const ELEMENT_PLACEMENT_TABLE: u32 = 0x8007_6C10;
/// The label-actor state block `FUN_801DBB8C` opens and `FUN_801D84C0` clears.
pub const LABEL_STATE_BLOCK: u32 = 0x801F_4E08;
/// Live character-record base (`0x80084708 + (id - 1) * 0x414`).
pub const CHAR_RECORD_BASE: u32 = 0x8008_4708;
/// Character-record stride.
pub const CHAR_RECORD_STRIDE: u32 = 0x414;
/// Display-name offset inside a character record.
pub const NAME_OFFSET: u32 = 0x2A7;

/// The name pointer both `FUN_801D84C0` arms hand to the string helpers.
///
/// Retail writes it as `0x8008459B + id * 0x414`; this is the same address
/// expressed against the documented record base, which is what shows the
/// constant to be the display-name field rather than an opaque table.
pub const fn name_field_ptr(participant_id: u8) -> u32 {
    CHAR_RECORD_BASE
        + (participant_id as u32)
            .wrapping_sub(1)
            .wrapping_mul(CHAR_RECORD_STRIDE)
        + NAME_OFFSET
}

/// Layout arguments `FUN_801DBB8C` passes to the SCUS text-actor register
/// `FUN_8003541C`, in call order: four in registers then four on the stack.
pub const OPEN_LAYOUT_ARGS: [i32; 8] = [0, 0xC, 0, -0x92, 0x24, 0x8A, 0x90, 3];

/// The state `FUN_801DBB8C` writes before the register call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabelState {
    /// `0x801F4E08`.
    pub kind: u8,
    /// `0x801F4E09` - `0x80` while open, `0` once torn down.
    pub flags: u8,
    /// `0x801F4E0A`.
    pub phase: u8,
    /// `0x801F4E0C` - the handle `FUN_8003541C` returned.
    pub handle: u32,
}

impl LabelState {
    /// The block as `FUN_801DBB8C` leaves it, given the register call's result.
    ///
    /// REPLACED-BY: `legaia_engine_ui::battle_hud_draws_for`'s immediate-mode
    /// rebuild. `FUN_801DBB8C` registers a **retained-mode** SCUS text actor
    /// and stashes its handle at `0x801F4E0C`; the port's battle HUD builds
    /// every `TextDraw` afresh from the live model on each frame, on both
    /// hosts, so there is no handle to hold and nothing to tear down. There
    /// is no object to open, and no host is owed one - the retained widget
    /// list is a representation the port replaced, not a call it has not
    /// made. This block stays as the record of the retail lifecycle.
    ///
    /// What that actor **is** was mis-read once: it is not the party readout.
    /// The routine's one caller is the menu SM's `0x28 -> 0x50` arm
    /// (`0x801D1660`, right after `FUN_801D388C(9)` builds the arts-entry
    /// screen), and the actor it registers is `FUN_8003541C(0, 0xC, 0, -146,
    /// 36, 138, 144, 3)` - a 138x144 box parked one screen to the left, the
    /// arts-list window the Triangle page slides in. The party bar and the
    /// roster panels are placement records 7 and 6 / 78 / 79, opened by the
    /// sub-draw script through `FUN_801D8DE8` (`engine-core::battle_hud`).
    ///
    /// PORT: FUN_801DBB8C
    pub const fn opened(handle: u32) -> Self {
        Self {
            kind: 0,
            flags: 0x80,
            phase: 0,
            handle,
        }
    }

    /// The block as `FUN_801D84C0` leaves it. Note `kind` is **not** written by
    /// the teardown - only `+0x01`, `+0x02` and the handle word are cleared.
    pub const fn torn_down(self) -> Self {
        Self {
            kind: self.kind,
            flags: 0,
            phase: 0,
            handle: 0,
        }
    }

    /// Is a label actor currently registered?
    pub const fn is_open(self) -> bool {
        self.handle != 0
    }
}

/// The selector `FUN_801DBB8C` publishes at `0x8007BB8C` before registering:
/// the active slot's participant id, minus one.
///
/// Zero-based because the layout builder indexes character records with it.
pub const fn open_selector(active_participant_id: u8) -> i32 {
    active_participant_id as i32 - 1
}

// ---------------------------------------------------------------------------
// FUN_801DBC30 - the label-strip blit
// ---------------------------------------------------------------------------

/// Ordering-table tag the strip carries (`0x09000000`).
pub const STRIP_OT_TAG: u32 = 0x0900_0000;
/// Command + colour word (`0x2C808080`).
pub const STRIP_CODE_COLOUR: u32 = 0x2C80_8080;
/// CLUT the strip samples (`0x7704`).
pub const STRIP_CLUT: u16 = 0x7704;
/// Texture page the strip samples (`7`).
pub const STRIP_TPAGE: u16 = 7;
/// Prim size in bytes (`0x28` - a ten-word textured quad).
pub const STRIP_PRIM_BYTES: usize = 0x28;

/// One textured quad, in the vertex order the prim words impose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripQuad {
    /// `+0x00`.
    pub tag: u32,
    /// `+0x04`.
    pub code_colour: u32,
    /// `+0x08`, `+0x10`, `+0x18`, `+0x20`.
    pub xy: [(i16, i16); 4],
    /// `+0x0C`, `+0x14`, `+0x1C`, `+0x24`.
    pub uv: [(u8, u8); 4],
    /// `+0x0E`.
    pub clut: u16,
    /// `+0x16`.
    pub tpage: u16,
}

/// Does the strip draw this frame?
///
/// Retail's only gate is `ctx[+0x6CE] != 0` -> return without touching the
/// primitive cursor, so a non-zero value suppresses the whole draw.
pub const fn strip_draws(ctx_6ce: i16) -> bool {
    ctx_6ce == 0
}

/// The cross-out mark at `(x, y)` (`FUN_801DBC30`).
///
/// A `0x40 x 0x10` blit: screen span `x-8 ..= x+0x37` by `y-4 ..= y+0xB`
/// against texel span `0 ..= 0x3F` by `0x60 ..= 0x6F` - both `0x3F` wide and
/// `0x0F` tall, so the mark is drawn 1:1 with no scaling.
///
/// It is **not** a name plate. Resolving the two constants the quad carries
/// settles what it samples: `tpage 7` is page `(448, 0)` and CLUT `0x7704` is
/// `(64, 476)`, which is the `etim` effect page's red **cross-out X** at
/// texels `(0, 96)`-`(63, 111)` - the same rect
/// [`minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md)
/// names for the mark retail lays over a forbidden command chip, confirmed by
/// decoding those texels out of a battle VRAM dump. The battle name plates
/// come off a different sheet entirely; see [`crate::battle_chrome`].
///
/// The one surface that wants this mark is the Muscle Dome's command ring,
/// whose phase-`0x28` arm calls it on a forbidden chip's anchor
/// (`0x801D12D4` / `0x801D12F0`). The standalone minigames page draws that
/// ring and places the X from this quad (`minigames_muscle`'s chip rows carry
/// it as `mark_quad`, sampling the `etim` page through the CLUT's
/// sub-palette); the two play hosts draw the dome's ring as text rows and
/// carry no chip sprites to mark.
///
/// PORT: FUN_801DBC30
pub fn cross_out_mark(x: i16, y: i16) -> StripQuad {
    let x0 = x.wrapping_sub(8);
    let x1 = x.wrapping_add(0x37);
    let y0 = y.wrapping_sub(4);
    let y1 = y.wrapping_add(0x0B);
    StripQuad {
        tag: STRIP_OT_TAG,
        code_colour: STRIP_CODE_COLOUR,
        xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        uv: [(0x00, 0x60), (0x3F, 0x60), (0x00, 0x6F), (0x3F, 0x6F)],
        clut: STRIP_CLUT,
        tpage: STRIP_TPAGE,
    }
}

// ---------------------------------------------------------------------------
// FUN_801D84C0 - panel build + teardown
// ---------------------------------------------------------------------------

/// Per-party-size X anchors for the two positioned panels.
///
/// `(primary, secondary)`; `None` means retail writes nothing for that slot at
/// that party size, leaving whatever the previous build left behind. Party
/// sizes outside `1..=3` take no arm at all.
pub const fn panel_anchors(party_size: u8) -> Option<(i16, Option<i16>)> {
    match party_size {
        1 => Some((0x72, None)),
        2 => Some((0x3F, Some(0xA5))),
        3 => Some((0x0C, Some(0x72))),
        _ => None,
    }
}

/// Per-actor reset `FUN_801D84C0` applies to all three party slots before the
/// panels go up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyActorReset {
    /// `+0x1DD` - target slot, forced to the first monster.
    pub target_slot: u8,
    /// `+0x1DE` - action category, forced to Martial Arts.
    pub category: u8,
}

/// The reset every party actor takes: target the first monster slot, category
/// back to Martial Arts. Unconditional - it is applied to all three pool slots
/// whether or not the party is that large.
pub const PARTY_RESET: PartyActorReset = PartyActorReset {
    target_slot: 3,
    category: 0,
};

/// Bias added to a participant id for the per-panel portrait cell
/// (`id + 0x32`, written to both bytes of the cell pair).
pub const PORTRAIT_BIAS: u8 = 0x32;

/// The portrait cell value for a participant id.
pub const fn portrait_cell(participant_id: u8) -> u8 {
    participant_id.wrapping_add(PORTRAIT_BIAS)
}

/// Which build arm `FUN_801D84C0` takes.
///
/// The discriminator is the **second** party slot's participant id
/// (`lbu 1(0x8007BD10)` at `0x801D8504`): zero takes the solo arm, which
/// opens every buffer with the first slot's display name; non-zero takes the
/// roster arm, which opens them with a fixed string carrying a `0xC1` name
/// escape instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildArm {
    /// `DAT_8007BD11 == 0`.
    Solo,
    /// `DAT_8007BD11 != 0`.
    Roster,
}

/// Pick the build arm.
pub const fn build_arm(second_slot_id: u8) -> BuildArm {
    if second_slot_id == 0 {
        BuildArm::Solo
    } else {
        BuildArm::Roster
    }
}

/// The escape byte the roster arm locates in each buffer
/// (`FUN_8003CBF8(buffer, 0xC1, 1)` - `a1` is the byte searched for, not a
/// width). `0xC1` is the text engine's character-name escape; the arm writes
/// its operand, the byte after it, as `participant_id - 1`.
pub const NAME_ESCAPE: u8 = 0xC1;

/// The buffer order the roster arm patches in - not the declaration order:
/// the defeat buffer is patched last.
pub const MEASURE_ORDER: [usize; 4] = [0xA9, 0x159, 0x189, 0x129];

/// Which battle-end message a buffer holds.
///
/// The four buffers are the **battle-result messages**, not panel labels:
/// resolving the six pool strings the two arms copy and append
/// (`0x801F4C2C`, `0x801F4C38`, `0x801F4C78`, `0x801F4C94`, `0x801F4CAC`,
/// `0x801F4CC4` in the `0898` image) shows a victory line with its spoils
/// sentence, a defeat line, and an escape line in its two outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultMessage {
    /// `+0xA9` - the win and the spoils that follow it.
    Victory,
    /// `+0x129` - the party is beaten. The solo arm appends a suffix to the
    /// name (`0x801F4C94`); the roster arm copies one whole team string
    /// (`0x801F4C78`) instead.
    Defeated,
    /// `+0x159` - a successful escape.
    Escaped,
    /// `+0x189` - an escape that failed.
    EscapeFailed,
}

/// The four buffers, in [`LABEL_BUFFERS`] order, and what each holds.
pub const RESULT_BUFFERS: [(usize, ResultMessage); 4] = [
    (0xA9, ResultMessage::Victory),
    (0x129, ResultMessage::Defeated),
    (0x159, ResultMessage::Escaped),
    (0x189, ResultMessage::EscapeFailed),
];

/// Who a battle-result message names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultSubject {
    /// The solo arm: the lead's display name, read straight out of its
    /// record through [`name_field_ptr`].
    Lead(u8),
    /// The roster arm: the fixed team string, whose `0xC1` escape operand is
    /// patched to the **lead's** index - every one of the four patches reads
    /// the first slot (`lbu -0x42f0(s2)` at `0x801D86B4` / `0x801D86DC` /
    /// `0x801D8704` / `0x801D8728`), so the team is always named after the
    /// lead, never after the member in that seat.
    LeadsTeam { escape_operand: u8 },
}

impl Default for ResultSubject {
    /// A party of more than one - the roster arm, naming slot `1`'s team.
    fn default() -> Self {
        ResultSubject::LeadsTeam { escape_operand: 0 }
    }
}

/// Who the four battle-result messages name, for a party in panel order
/// (`DAT_8007BD10..`, an absent seat `0`).
///
/// This is the whole of the two build arms' decision; the text itself is the
/// pool's. The engine's post-battle report (`engine-ui::battle_spoils_windows`)
/// words its victory line from it, which is what makes a lone lead's win read
/// "<name> won the battle!" rather than naming a team of one.
///
/// PORT: FUN_801D84C0 (the two build arms, `0x801D8504..0x801D8734`)
pub const fn result_subject(slots: [u8; 3]) -> ResultSubject {
    let lead = slots[0];
    match build_arm(slots[1]) {
        BuildArm::Solo => ResultSubject::Lead(lead),
        BuildArm::Roster => ResultSubject::LeadsTeam {
            escape_operand: lead.wrapping_sub(1),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_pointer_matches_retails_constant() {
        // Retail's literal is `0x8008459B + id * 0x414`.
        for id in 1u8..=4 {
            let retail = 0x8008_459Bu32 + id as u32 * 0x414;
            assert_eq!(name_field_ptr(id), retail, "id {id}");
        }
    }

    #[test]
    fn name_pointer_is_the_records_display_name_field() {
        // Slot 0's record starts at CHAR_RECORD_BASE; its name is +0x2A7.
        assert_eq!(name_field_ptr(1), CHAR_RECORD_BASE + NAME_OFFSET);
        assert_eq!(
            name_field_ptr(2),
            CHAR_RECORD_BASE + CHAR_RECORD_STRIDE + NAME_OFFSET
        );
    }

    #[test]
    fn open_writes_the_handle_and_raises_the_flag() {
        let s = LabelState::opened(0x1234);
        assert_eq!(s.flags, 0x80);
        assert_eq!(s.handle, 0x1234);
        assert!(s.is_open());
    }

    #[test]
    fn teardown_clears_the_handle_but_not_the_kind_byte() {
        let open = LabelState {
            kind: 9,
            flags: 0x80,
            phase: 5,
            handle: 0x1234,
        };
        let closed = open.torn_down();
        assert_eq!(closed.handle, 0);
        assert_eq!(closed.flags, 0);
        assert_eq!(closed.phase, 0);
        assert_eq!(closed.kind, 9, "the teardown never writes +0x00");
        assert!(!closed.is_open());
    }

    #[test]
    fn open_and_teardown_are_a_matched_pair_on_one_block() {
        let s = LabelState::opened(0x40).torn_down();
        assert_eq!(s, LabelState::default().torn_down());
    }

    #[test]
    fn open_selector_is_zero_based() {
        assert_eq!(open_selector(1), 0);
        assert_eq!(open_selector(3), 2);
        // Retail does not guard the subtraction.
        assert_eq!(open_selector(0), -1);
    }

    #[test]
    fn open_layout_args_carry_the_negative_register_argument() {
        assert_eq!(OPEN_LAYOUT_ARGS.len(), 8);
        assert_eq!(OPEN_LAYOUT_ARGS[3], -0x92, "a3 is signed");
        assert_eq!(&OPEN_LAYOUT_ARGS[4..], &[0x24, 0x8A, 0x90, 3]);
    }

    #[test]
    fn strip_is_suppressed_by_a_nonzero_gate() {
        assert!(strip_draws(0));
        assert!(!strip_draws(1));
        assert!(!strip_draws(-1));
    }

    #[test]
    fn strip_is_a_one_to_one_blit_of_a_sixty_four_by_sixteen_cell() {
        let q = cross_out_mark(100, 80);
        assert_eq!(q.xy[0], (92, 76));
        assert_eq!(q.xy[3], (155, 91));
        assert_eq!(q.xy[1].0 - q.xy[0].0, (q.uv[1].0 - q.uv[0].0) as i16);
        assert_eq!(q.xy[2].1 - q.xy[0].1, (q.uv[2].1 - q.uv[0].1) as i16);
        assert_eq!(q.xy[1].0 - q.xy[0].0, 0x3F);
        assert_eq!(q.xy[2].1 - q.xy[0].1, 0x0F);
        assert_eq!(q.clut, STRIP_CLUT);
        assert_eq!(q.tpage, STRIP_TPAGE);
        assert_eq!(q.tag, STRIP_OT_TAG);
        assert_eq!(q.code_colour, STRIP_CODE_COLOUR);
    }

    #[test]
    fn strip_corners_share_edges() {
        let q = cross_out_mark(0, 0);
        assert_eq!(q.xy[0].1, q.xy[1].1);
        assert_eq!(q.xy[2].1, q.xy[3].1);
        assert_eq!(q.xy[0].0, q.xy[2].0);
        assert_eq!(q.xy[1].0, q.xy[3].0);
        assert_eq!(q.uv[0].1, q.uv[1].1);
        assert_eq!(q.uv[0].0, q.uv[2].0);
    }

    #[test]
    fn strip_samples_a_different_atlas_row_than_the_value_readout_label() {
        // FUN_801E805C's label is CLUT 0x7703 / tpage 0x27 at v 0xE0..0xEF;
        // this one is CLUT 0x7704 / tpage 7 at v 0x60..0x6F. Same shape, two
        // distinct atlas cells - so the two are not the same blit.
        assert_ne!(STRIP_CLUT, crate::battle_value_readout::GLYPH_CLUT);
        assert_ne!(STRIP_TPAGE, crate::battle_value_readout::GLYPH_TPAGE);
    }

    #[test]
    fn solo_party_is_centred_and_a_pair_splits_outward() {
        assert_eq!(panel_anchors(1), Some((0x72, None)));
        let (a, b) = panel_anchors(2).unwrap();
        let b = b.unwrap();
        assert!(a < 0x72 && b > 0x72, "the pair straddles the solo anchor");
        assert_eq!((a, b), (0x3F, 0xA5));
    }

    #[test]
    fn a_full_party_shifts_the_row_left() {
        assert_eq!(panel_anchors(3), Some((0x0C, Some(0x72))));
        // Each larger party moves the primary anchor further left.
        let one = panel_anchors(1).unwrap().0;
        let two = panel_anchors(2).unwrap().0;
        let three = panel_anchors(3).unwrap().0;
        assert!(three < two && two < one);
    }

    #[test]
    fn party_sizes_outside_one_to_three_take_no_arm() {
        assert_eq!(panel_anchors(0), None);
        assert_eq!(panel_anchors(4), None);
        assert_eq!(panel_anchors(0xFF), None);
    }

    #[test]
    fn the_centring_matches_the_field_party_cursor() {
        // FUN_801F1278 seeds a solo member into the *middle* cell and a pair
        // into the outer two. The battle panels do the same with pixels.
        let cells_one = crate::field_party_cursor::seed_member_cells(&[5]);
        let cells_two = crate::field_party_cursor::seed_member_cells(&[5, 6]);
        assert_eq!(cells_one[1], 5, "solo takes the middle cell");
        assert_eq!(
            (cells_two[0], cells_two[2]),
            (5, 6),
            "a pair takes the outers"
        );
        assert_eq!(
            panel_anchors(1).unwrap().1,
            None,
            "solo positions one panel"
        );
        assert!(
            panel_anchors(2).unwrap().1.is_some(),
            "a pair positions two"
        );
    }

    #[test]
    fn every_party_actor_is_reset_to_target_the_first_monster() {
        assert_eq!(PARTY_RESET.target_slot, 3);
        assert_eq!(PARTY_RESET.category, 0);
    }

    #[test]
    fn portrait_cells_are_the_id_plus_a_fixed_bias() {
        assert_eq!(portrait_cell(1), 0x33);
        assert_eq!(portrait_cell(3), 0x35);
        assert_eq!(portrait_cell(0), 0x32);
    }

    #[test]
    fn build_arm_keys_on_the_second_slot() {
        assert_eq!(build_arm(0), BuildArm::Solo);
        assert_eq!(build_arm(1), BuildArm::Roster);
        assert_eq!(build_arm(0xFF), BuildArm::Roster);
    }

    #[test]
    fn the_roster_arm_patches_out_of_declaration_order() {
        let mut declared = LABEL_BUFFERS;
        let mut measured = MEASURE_ORDER;
        declared.sort_unstable();
        measured.sort_unstable();
        assert_eq!(declared, measured);
        assert_ne!(LABEL_BUFFERS, MEASURE_ORDER);
        assert_eq!(RESULT_BUFFERS.map(|(b, _)| b), LABEL_BUFFERS);
    }

    #[test]
    fn a_lone_lead_is_named_and_a_party_is_the_leads_team() {
        assert_eq!(result_subject([2, 0, 0]), ResultSubject::Lead(2));
        // Every patch reads the FIRST seat, whoever sits in the others.
        assert_eq!(
            result_subject([1, 2, 3]),
            ResultSubject::LeadsTeam { escape_operand: 0 }
        );
        assert_eq!(
            result_subject([3, 1, 2]),
            ResultSubject::LeadsTeam { escape_operand: 2 }
        );
        // The discriminator is the SECOND seat, not the party size.
        assert_eq!(result_subject([1, 0, 3]), ResultSubject::Lead(1));
    }

    #[test]
    fn a_named_lead_resolves_to_the_records_display_name() {
        let ResultSubject::Lead(id) = result_subject([3, 0, 0]) else {
            panic!("a lone lead is named");
        };
        assert_eq!(
            name_field_ptr(id),
            CHAR_RECORD_BASE + 2 * CHAR_RECORD_STRIDE + NAME_OFFSET
        );
    }

    #[test]
    fn published_buffers_point_into_the_context() {
        for (slot, buf) in PUBLISHED_BUFFERS {
            assert!(slot > 0x600, "placement-table offset {slot:#x}");
            assert!(buf < 0x200, "context offset {buf:#x}");
        }
    }
}
