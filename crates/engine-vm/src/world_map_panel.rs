//! The world-map / dev-menu **panel window system** - the three shared
//! leaves every `ctx[+0x54]`-keyed panel actor in the field overlay's
//! world-map band funnels through.
//!
//! The panel actors themselves (`FUN_801ED308`, `FUN_801ED590`,
//! `FUN_801EDF00`, `FUN_801EE5D4`, `FUN_801EE90C`, `FUN_801EF014`, ...) are
//! each a small phase machine, but they do almost no work of their own: they
//! open and close their windows by running a **command script** through
//! `FUN_801E9B3C`, they read the pad through the shared list-cursor helper
//! `FUN_801E9DC8`, and the developer menu's row actions run through
//! `FUN_801EA9B0`. Those three are what this module ports.
//!
//! All three live in the field overlay (PROT 0897, base `0x801CE818`). The
//! capture dumps tagged `overlay_world_map*` / `overlay_cutscene_dialogue` /
//! `overlay_debug_menu` are byte-identical to that image at these VAs - the
//! world map is a 0897-hosted *mode*, not an overlay of its own.
//!
//! ## Panel descriptor table
//!
//! Every window is a 28-byte descriptor in the array at `0x801F2B98`,
//! indexed by the panel index the script record carries. The two fields the
//! script system reads back are `+0x08` (x) and `+0x0A` (y); `+0x0E` is the
//! height the sizing arms write. That is the same descriptor
//! [`crate::world_map_overlay::panel_geometry`] sizes from the list-picker
//! side.
//!
//! ## Source
//!
//! - `ghidra/scripts/funcs/overlay_cutscene_dialogue_801e9b3c.txt`
//! - `ghidra/scripts/funcs/overlay_cutscene_dialogue_801e9dc8.txt`
//! - `ghidra/scripts/funcs/overlay_cutscene_dialogue_801ea9b0.txt`
//!
//! The same-VA dumps under the `overlay_0896_*` / `overlay_0897_*` prefixes
//! are **not** usable here: at each of these three addresses they print a
//! body that resolves against no extracted image, and at `0x801E9B3C` /
//! `0x801E9DC8` the printed body starts mid-instruction-stream (first word a
//! delay slot, callee-saved registers read before any write). The bodies used
//! above are the ones that match PROT 0897 at the queried VA.
//!
//! ## NOT WIRED
//!
//! Nothing in the engine hosts a panel window. `WorldMapController` models
//! the retail controller's view-mode, screen-dim and horizon-gate state, but
//! it owns no window list, no panel-descriptor array and no `ctx[+0x54]`
//! phase for a panel actor; `SceneMode` has no dev-menu or panel mode to
//! enter one from. The renderer-side counterpart is in the same position -
//! `legaia_engine_ui::dev_menu_list_draws_for` has no caller either. Wiring
//! is therefore "stand the debug/panel screen up", not "connect two halves",
//! and a synthetic descriptor array would only re-exercise the unit tests
//! below. The prerequisite is a panel-window host on `WorldMapController`.

// ---------------------------------------------------------------------------
// FUN_801E9B3C - panel command-script interpreter
// ---------------------------------------------------------------------------

/// Stride of one panel descriptor in the array at `0x801F2B98`.
pub const PANEL_DESCRIPTOR_STRIDE: usize = 28;

/// Stride of one panel-script record.
pub const PANEL_SCRIPT_STRIDE: usize = 8;

/// The two descriptor fields the script interpreter reads back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelDescriptor {
    /// `desc[+0x08]` - panel origin x.
    pub x: i16,
    /// `desc[+0x0A]` - panel origin y.
    pub y: i16,
    /// `desc[+0x0E]` - panel height.
    pub height: i16,
}

/// One 8-byte panel-script record: `[u16 op][i16 panel][u32 operand]`.
///
/// A record whose `op` is zero terminates the script.
///
/// PORT: FUN_801e9b3c (record layout - `lh/lhu 0x0(s5)`, `lh -0x2(s3)`,
/// `lw 0x0(s3)`, both cursors stepping by 8)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelCommand {
    /// Opcode. `0` terminates; `1..=13` select an arm, anything else is a
    /// no-op that still consumes the record.
    pub op: u16,
    /// Panel index into the `0x801F2B98` descriptor array.
    pub panel: i16,
    /// The record's 32-bit operand. Arms that take a position read it as two
    /// packed `i16`s (`operand & 0xFFFF` = x, `operand >> 16` = y).
    pub operand: u32,
}

impl PanelCommand {
    /// The operand read as a packed `(x, y)` pair.
    pub fn operand_xy(self) -> (i16, i16) {
        (
            (self.operand & 0xFFFF) as u16 as i16,
            (self.operand >> 16) as u16 as i16,
        )
    }
}

/// What one panel-script record asks the window system to do.
///
/// The arm index is `op - 1` compared unsigned against `13`
/// (`sltiu v0,v1,0xd`), so `op == 0` never reaches here (it terminates) and
/// `op > 13` falls into the shared default.
///
/// PORT: FUN_801e9b3c (the 13-entry jump table at `0x801CF25C`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEffect {
    /// `op 1` - ensure the panel exists, then place it at its **descriptor**
    /// position (`desc.x`, `desc.y`).
    OpenAtDescriptor { panel: i16, x: i16, y: i16 },
    /// `op 2` - ensure the panel exists, then place it at the operand's
    /// explicit position.
    OpenAt { panel: i16, x: i16, y: i16 },
    /// `op 3` - store the operand's low byte into the window object's
    /// `+0x1D` field. No-op when the panel is not open.
    SetStyleByte { panel: i16, value: u8 },
    /// `op 4` - close this panel (`FUN_80035978`).
    Close { panel: i16 },
    /// `op 5` - close every panel (`FUN_80035A4C`); takes no panel index.
    CloseAll,
    /// `op 8` - retire the panel's actor (`FUN_800319A8`).
    Retire { panel: i16 },
    /// `op 6` - zero the window object's `+0x20` halfword. No-op when the
    /// panel is not open.
    ClearCounter { panel: i16 },
    /// `op 9` - ensure the panel exists, then slide it (`FUN_800358C0`) to
    /// the operand position, or to the descriptor position when the operand
    /// is zero.
    SlideTo { panel: i16, x: i16, y: i16 },
    /// `op 10` - retire and respawn the panel, sliding it back to the
    /// position the live object was already at (`obj[+0x0A]`, `obj[+0x0C]`).
    /// No-op when the panel is not open.
    Respawn { panel: i16 },
    /// `op 12` - resize the party panel from the live party size and run the
    /// nested script at `0x801F3170`. See [`party_panel_geometry`].
    PartyPanel {
        /// Descriptor index the retail arm writes: `+0xCE/+0xD2/+0xD6` off
        /// the table base, i.e. descriptor 7.
        panel: i16,
        geometry: PanelDescriptor,
    },
    /// `op 7`, `op 11`, `op 13` and every `op > 13`: the shared default arm -
    /// the record is consumed and nothing happens.
    Nop,
}

/// Descriptor index the party-panel arm (`op 12`) rewrites: the stores are
/// `sh .., 0xCE/0xD2/0xD6(s4)` with `s4 = 0x801F2B98`, and `0xCE = 7*28 + 10`.
pub const PARTY_PANEL_INDEX: i16 = 7;

/// The party panel's height/`y` kernel: `height = members*56 - 7` and
/// `y = 202 - height`. Both the `+0x0A` (y) and `+0x12` stores take the same
/// value, so the panel is bottom-anchored at 202 the way the list picker's
/// panels are bottom-anchored at 208.
///
/// `members` is the live party count at `0x80084594`, read as a byte.
///
/// PORT: FUN_801e9b3c (case `0x0B` body at `0x801E9D50..0x801E9D84`)
pub fn party_panel_geometry(members: u8) -> PanelDescriptor {
    let height = i32::from(members) * 56 - 7;
    let y = 202 - height;
    PanelDescriptor {
        x: 0,
        y: y as i16,
        height: height as i16,
    }
}

/// Decode one panel-script record into its effect.
///
/// `descriptor` supplies the panel's `0x801F2B98` entry (the arms that place
/// a window at its descriptor position read `+0x08` / `+0x0A` back), and
/// `party_members` is the live party count the `op 12` arm sizes from.
///
/// Returns `None` for the terminator (`op == 0`).
///
/// PORT: FUN_801e9b3c
///
/// NOT WIRED: no engine host owns a panel-descriptor array - see the module
/// disclosure. `WorldMapController` is the missing host.
pub fn decode_panel_command(
    cmd: PanelCommand,
    descriptor: PanelDescriptor,
    party_members: u8,
) -> Option<PanelEffect> {
    if cmd.op == 0 {
        return None;
    }
    // `v1 = (i16)(op - 1)`, then `sltiu v1, 13`: a negative arm index folds
    // to a huge unsigned and misses the table, so it takes the default arm.
    let arm = (cmd.op as i16).wrapping_sub(1);
    if (arm as u16 as u32) >= 13 {
        return Some(PanelEffect::Nop);
    }
    let panel = cmd.panel;
    let (ox, oy) = cmd.operand_xy();
    Some(match arm {
        0 => PanelEffect::OpenAtDescriptor {
            panel,
            x: descriptor.x,
            y: descriptor.y,
        },
        1 => PanelEffect::OpenAt {
            panel,
            x: ox,
            y: oy,
        },
        2 => PanelEffect::SetStyleByte {
            panel,
            value: cmd.operand as u8,
        },
        3 => PanelEffect::Close { panel },
        4 => PanelEffect::CloseAll,
        5 => PanelEffect::ClearCounter { panel },
        7 => PanelEffect::Retire { panel },
        // The operand-zero fall-through is a literal `beq a2,zero` into the
        // descriptor-position call, not a separate arm.
        8 => {
            if cmd.operand == 0 {
                PanelEffect::SlideTo {
                    panel,
                    x: descriptor.x,
                    y: descriptor.y,
                }
            } else {
                PanelEffect::SlideTo {
                    panel,
                    x: ox,
                    y: oy,
                }
            }
        }
        9 => PanelEffect::Respawn { panel },
        11 => PanelEffect::PartyPanel {
            panel: PARTY_PANEL_INDEX,
            geometry: party_panel_geometry(party_members),
        },
        // Arms 6, 10 and 12 land on the shared default label.
        _ => PanelEffect::Nop,
    })
}

/// Run a whole panel script, stopping at the terminator record.
///
/// `descriptor_of` supplies the `0x801F2B98` entry for a panel index. The
/// nested script the `op 12` arm recurses into (`0x801F3170`) is *not*
/// followed here - the caller gets a [`PanelEffect::PartyPanel`] and decides,
/// because the nested script is overlay data this crate does not own.
///
/// PORT: FUN_801e9b3c (the `do { } while (*param_1 != 0)` record loop)
///
/// NOT WIRED: same host gap as [`decode_panel_command`].
pub fn run_panel_script(
    script: &[PanelCommand],
    party_members: u8,
    descriptor_of: impl Fn(i16) -> PanelDescriptor,
) -> Vec<PanelEffect> {
    let mut out = Vec::new();
    for &cmd in script {
        match decode_panel_command(cmd, descriptor_of(cmd.panel), party_members) {
            Some(effect) => out.push(effect),
            None => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// FUN_801E9DC8 - shared vertical list-cursor input helper
// ---------------------------------------------------------------------------

/// The pad state `FUN_801E9DC8` reads, in the two banks retail keeps apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CursorPad {
    /// `_DAT_8007B874` - the held-button mask the two action buttons test.
    pub held: u32,
    /// `_DAT_8007BB84` - the newly-pressed mask the Up/Down steps test.
    pub pressed: u32,
    /// `_DAT_800846D0` - the configurable mask for action button A.
    pub action_a_mask: u32,
    /// `_DAT_800846D4` - the configurable mask for action button B.
    pub action_b_mask: u32,
}

/// D-pad Up bit in the newly-pressed mask.
pub const PAD_UP: u32 = 0x1000;
/// D-pad Down bit in the newly-pressed mask.
pub const PAD_DOWN: u32 = 0x4000;

/// SFX the cursor helper fires, by arm.
pub const SFX_CURSOR_MOVE: u32 = 0x21;
/// SFX for the action-A arm.
pub const SFX_ACTION_A: u32 = 0x36;
/// SFX for the action-B arm.
pub const SFX_ACTION_B: u32 = 0x37;

/// What one call to the cursor helper resolved to - retail's `v0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorOutcome {
    /// `0` - nothing happened this frame.
    Idle,
    /// `1` - action button A (`_DAT_800846D0`), SFX `0x36`.
    ActionA,
    /// `2` - action button B (`_DAT_800846D4`), SFX `0x37`.
    ActionB,
    /// `3` - the cursor moved (or, in wrapping mode, a step was taken), SFX
    /// `0x21`.
    Moved,
}

impl CursorOutcome {
    /// The raw `v0` retail returns.
    pub fn code(self) -> i32 {
        match self {
            CursorOutcome::Idle => 0,
            CursorOutcome::ActionA => 1,
            CursorOutcome::ActionB => 2,
            CursorOutcome::Moved => 3,
        }
    }
}

/// One frame of the shared vertical list cursor.
///
/// This is the picker kernel the world-map band's panel actors all call -
/// the destination list (`FUN_801EF014`), the text-box dispatcher
/// (`FUN_801EE90C`, which calls it as `(cursor, 2, wrap = true)`) and the
/// dev-menu list picker `FUN_801ECA08`, whose swap-wrap cursor
/// ([`crate::world_map_overlay::cursor_step`]) is the *dev menu's own* copy of
/// this behaviour with the range expressed as an inclusive row span.
///
/// Order of business, exactly as retail runs it:
///
/// 1. The two action buttons are tested against the **held** mask first, and
///    either one returns immediately without touching the cursor.
/// 2. Otherwise Up then Down are tested against the **newly-pressed** mask.
///
/// The two modes differ in more than the wrap:
///
/// | | `wrap = false` | `wrap = true` |
/// |---|---|---|
/// | Up | only when `cursor > 0` | always; `0` jumps to `count - 1` |
/// | Down | only when `cursor + 1 < count` | always; `count` folds to `0` |
/// | SFX | only when the cursor actually moves | on every press |
///
/// Returns the outcome and the SFX id to fire, if any. `cursor` is updated
/// in place - retail takes it by pointer (`param_1`).
///
/// PORT: FUN_801e9dc8
///
/// NOT WIRED: the engine's only live list cursors are the pause-menu and
/// shop pickers in `legaia_engine_core`, which are their own retail
/// routines; the world-map band's pickers this kernel serves have no host
/// (see the module disclosure).
pub fn list_cursor_input(
    cursor: &mut i32,
    count: i32,
    wrap: bool,
    pad: &CursorPad,
) -> (CursorOutcome, Option<u32>) {
    if pad.held & pad.action_a_mask != 0 {
        return (CursorOutcome::ActionA, Some(SFX_ACTION_A));
    }
    if pad.held & pad.action_b_mask != 0 {
        return (CursorOutcome::ActionB, Some(SFX_ACTION_B));
    }

    let mut outcome = CursorOutcome::Idle;
    let mut sfx = None;
    let up = pad.pressed & PAD_UP != 0;
    let down = pad.pressed & PAD_DOWN != 0;

    if wrap {
        if up {
            sfx = Some(SFX_CURSOR_MOVE);
            *cursor = if *cursor == 0 { count - 1 } else { *cursor - 1 };
            outcome = CursorOutcome::Moved;
        }
        if down {
            sfx = Some(SFX_CURSOR_MOVE);
            let next = *cursor + 1;
            *cursor = if next == count { 0 } else { next };
            outcome = CursorOutcome::Moved;
        }
    } else {
        if up && *cursor > 0 {
            sfx = Some(SFX_CURSOR_MOVE);
            *cursor -= 1;
            outcome = CursorOutcome::Moved;
        }
        if down && *cursor + 1 < count {
            sfx = Some(SFX_CURSOR_MOVE);
            *cursor += 1;
            outcome = CursorOutcome::Moved;
        }
    }
    (outcome, sfx)
}

// ---------------------------------------------------------------------------
// FUN_801EA9B0 - dev-menu row-action dispatcher
// ---------------------------------------------------------------------------

/// Row count the dev-menu action dispatcher bounds against
/// (`sltiu v0,v1,0x18`).
pub const DEV_MENU_ACTION_ROWS: i16 = 0x18;

/// The phase the out-of-range arm parks the actor in (`ctx[+0x54] = 1`).
pub const DEV_MENU_ACTION_PARK_PHASE: i16 = 1;

/// The value `FUN_801EA9B0` returns. It is `1` on **every** path: `s1` is
/// loaded with `1` in the delay slot of the bound check and every arm exits
/// through `move v0,s1`, including the out-of-range arm. The routine has no
/// second return value.
pub const DEV_MENU_ACTION_RESULT: i32 = 1;

/// One frame of the dev-menu row-action dispatcher.
///
/// `row` is `ctx[+0x9E]`, the cursor row the list picker maintains. Rows
/// `0..DEV_MENU_ACTION_ROWS` select one of the 24 jump-table arms (each a
/// debug cheat: heal the party, cycle the encounter rate at `_DAT_8007B5F8`,
/// max every stat on the three `0x80084140 + n*0x414` records, grant the
/// whole item table, cycle the BGM index at `_DAT_801F2E90`, toggle the
/// `_DAT_8007B606` flag, ...). Out-of-range parks the caller's phase at
/// [`DEV_MENU_ACTION_PARK_PHASE`].
///
/// Returns `(new_phase, result)`: `new_phase` is `Some` only when the arm
/// writes `ctx[+0x54]`, and `result` is always
/// [`DEV_MENU_ACTION_RESULT`].
///
/// The arms' *effects* are not modelled - they poke globals and party
/// records that have no engine counterpart, and which arm sits at which
/// index is a property of the jump table at `0x801CF2E4`, not of the
/// instruction stream. What is ported is the contract the **caller** depends
/// on: the bound, the park phase, and the unconditional `1`.
///
/// PORT: FUN_801ea9b0 (dispatch head + shared epilogue)
///
/// NOT WIRED: the engine has no dev-menu actor, so nothing produces the
/// `ctx[+0x9E]` cursor row this consumes - the same host gap the rest of
/// [`crate::world_map_overlay`] declares.
pub fn dev_menu_action(row: i16) -> (Option<i16>, i32) {
    if (row as u16 as u32) >= DEV_MENU_ACTION_ROWS as u32 {
        return (Some(DEV_MENU_ACTION_PARK_PHASE), DEV_MENU_ACTION_RESULT);
    }
    (None, DEV_MENU_ACTION_RESULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(x: i16, y: i16) -> PanelDescriptor {
        PanelDescriptor { x, y, height: 0 }
    }

    #[test]
    fn zero_op_terminates() {
        let cmd = PanelCommand {
            op: 0,
            panel: 3,
            operand: 0x1234_5678,
        };
        assert_eq!(decode_panel_command(cmd, desc(0, 0), 3), None);
    }

    #[test]
    fn open_arms_pick_descriptor_versus_operand() {
        let at_desc = PanelCommand {
            op: 1,
            panel: 2,
            operand: 0x0011_0022,
        };
        assert_eq!(
            decode_panel_command(at_desc, desc(40, 60), 3),
            Some(PanelEffect::OpenAtDescriptor {
                panel: 2,
                x: 40,
                y: 60
            })
        );
        let explicit = PanelCommand { op: 2, ..at_desc };
        assert_eq!(
            decode_panel_command(explicit, desc(40, 60), 3),
            Some(PanelEffect::OpenAt {
                panel: 2,
                x: 0x22,
                y: 0x11
            })
        );
    }

    #[test]
    fn slide_falls_back_to_the_descriptor_on_a_zero_operand() {
        let zero = PanelCommand {
            op: 9,
            panel: 1,
            operand: 0,
        };
        assert_eq!(
            decode_panel_command(zero, desc(7, 9), 3),
            Some(PanelEffect::SlideTo {
                panel: 1,
                x: 7,
                y: 9
            })
        );
        let explicit = PanelCommand {
            operand: 0x0005_0006,
            ..zero
        };
        assert_eq!(
            decode_panel_command(explicit, desc(7, 9), 3),
            Some(PanelEffect::SlideTo {
                panel: 1,
                x: 6,
                y: 5
            })
        );
    }

    #[test]
    fn out_of_table_ops_take_the_default_arm() {
        for op in [7u16, 11, 13, 14, 0x100, 0xFFFF] {
            let cmd = PanelCommand {
                op,
                panel: 0,
                operand: 0,
            };
            assert_eq!(
                decode_panel_command(cmd, desc(0, 0), 3),
                Some(PanelEffect::Nop),
                "op {op:#x}"
            );
        }
    }

    #[test]
    fn party_panel_is_bottom_anchored_at_202() {
        for members in 1u8..=4 {
            let g = party_panel_geometry(members);
            assert_eq!(g.height, i16::from(members) * 56 - 7);
            assert_eq!(g.y + g.height, 202);
        }
    }

    #[test]
    fn script_stops_at_the_terminator() {
        let script = [
            PanelCommand {
                op: 1,
                panel: 0,
                operand: 0,
            },
            PanelCommand {
                op: 5,
                panel: 0,
                operand: 0,
            },
            PanelCommand {
                op: 0,
                panel: 0,
                operand: 0,
            },
            PanelCommand {
                op: 4,
                panel: 0,
                operand: 0,
            },
        ];
        let out = run_panel_script(&script, 3, |_| desc(1, 2));
        assert_eq!(
            out,
            vec![
                PanelEffect::OpenAtDescriptor {
                    panel: 0,
                    x: 1,
                    y: 2
                },
                PanelEffect::CloseAll,
            ]
        );
    }

    fn pad(held: u32, pressed: u32) -> CursorPad {
        CursorPad {
            held,
            pressed,
            action_a_mask: 0x20,
            action_b_mask: 0x40,
        }
    }

    #[test]
    fn action_buttons_short_circuit_before_the_cursor() {
        let mut c = 1;
        let (o, sfx) = list_cursor_input(&mut c, 4, true, &pad(0x20, PAD_DOWN));
        assert_eq!((o, sfx), (CursorOutcome::ActionA, Some(SFX_ACTION_A)));
        assert_eq!(c, 1, "the cursor must not move on an action press");

        let (o, sfx) = list_cursor_input(&mut c, 4, true, &pad(0x40, PAD_UP));
        assert_eq!((o, sfx), (CursorOutcome::ActionB, Some(SFX_ACTION_B)));
        assert_eq!(c, 1);
    }

    #[test]
    fn action_a_wins_over_action_b() {
        let mut c = 0;
        let (o, _) = list_cursor_input(&mut c, 4, true, &pad(0x60, 0));
        assert_eq!(o, CursorOutcome::ActionA);
    }

    #[test]
    fn wrapping_cursor_folds_at_both_ends() {
        let mut c = 0;
        list_cursor_input(&mut c, 4, true, &pad(0, PAD_UP));
        assert_eq!(c, 3);
        list_cursor_input(&mut c, 4, true, &pad(0, PAD_DOWN));
        assert_eq!(c, 0);
    }

    #[test]
    fn wrapping_cursor_fires_sfx_even_at_the_ends() {
        let mut c = 0;
        let (o, sfx) = list_cursor_input(&mut c, 4, true, &pad(0, PAD_UP));
        assert_eq!((o, sfx), (CursorOutcome::Moved, Some(SFX_CURSOR_MOVE)));
    }

    #[test]
    fn clamping_cursor_is_silent_at_the_ends() {
        let mut c = 0;
        let (o, sfx) = list_cursor_input(&mut c, 4, false, &pad(0, PAD_UP));
        assert_eq!((o, sfx), (CursorOutcome::Idle, None));
        assert_eq!(c, 0);

        let mut c = 3;
        let (o, sfx) = list_cursor_input(&mut c, 4, false, &pad(0, PAD_DOWN));
        assert_eq!((o, sfx), (CursorOutcome::Idle, None));
        assert_eq!(c, 3);
    }

    #[test]
    fn clamping_cursor_steps_inside_the_range() {
        let mut c = 1;
        let (o, _) = list_cursor_input(&mut c, 4, false, &pad(0, PAD_DOWN));
        assert_eq!((o, c), (CursorOutcome::Moved, 2));
        let (o, _) = list_cursor_input(&mut c, 4, false, &pad(0, PAD_UP));
        assert_eq!((o, c), (CursorOutcome::Moved, 1));
    }

    #[test]
    fn both_directions_in_one_frame_apply_in_order() {
        // Retail tests Up then Down without an else, so a frame carrying both
        // edges runs both steps.
        let mut c = 2;
        let (o, _) = list_cursor_input(&mut c, 4, false, &pad(0, PAD_UP | PAD_DOWN));
        assert_eq!((o, c), (CursorOutcome::Moved, 2));
    }

    #[test]
    fn dev_menu_action_always_returns_one() {
        for row in [-1i16, 0, 1, 0x17, 0x18, 0x400] {
            assert_eq!(dev_menu_action(row).1, DEV_MENU_ACTION_RESULT);
        }
    }

    #[test]
    fn dev_menu_action_parks_out_of_range_rows() {
        assert_eq!(dev_menu_action(0).0, None);
        assert_eq!(dev_menu_action(0x17).0, None);
        assert_eq!(dev_menu_action(0x18).0, Some(DEV_MENU_ACTION_PARK_PHASE));
        // The bound is `sltiu`, so a negative row folds to a huge unsigned.
        assert_eq!(dev_menu_action(-1).0, Some(DEV_MENU_ACTION_PARK_PHASE));
    }
}
