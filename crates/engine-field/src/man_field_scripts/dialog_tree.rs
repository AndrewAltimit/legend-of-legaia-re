//! A placed actor's **whole conversation** as a tree: every text page, choice
//! and condition its interaction script can reach.
//!
//! [`classify_placement`] keeps only the first line of an NPC's text, which is
//! a label, not the conversation. The conversation is the field-VM program the
//! dialog SM `FUN_80039B7C` runs from the interaction cursor
//! ([`placement_interaction_entry_pc`]): a stream of `0x1F` text lines, the
//! post-page control bytes between them, option pickers whose jump tables pick
//! the next line, and story-flag / inventory / position tests that branch to
//! another arm of the text pool. [`placement_dialog_tree`] walks that program
//! with the side-effect-free decoder ([`field_disasm::decode`]) and follows
//! every arm instead of executing one:
//!
//! - consecutive `0x1F` lines form one [`DialogNode::Page`] (the pager types
//!   them as one page, `docs/formats/mes.md` § multi-segment box packing); a
//!   `4C E1` text balloon is a page too;
//! - a picker (`0x27..=0x2A`) becomes a [`DialogNode::Choice`] whose options
//!   carry their on-screen label and the arm the option's relative jump lands
//!   on (`FUN_80038050`, [`legaia_mes::picker`]);
//! - every conditional jump in the branch-encoding table of
//!   `docs/subsystems/script-vm.md` becomes a [`DialogNode::Branch`] with the
//!   jump-taken arm and the fall-through arm, labelled by a [`Condition`] that
//!   names exactly what the arm tests - nothing is inferred beyond the op;
//! - item / money / party / flag writes, scene changes, battles, menus and
//!   movies become [`DialogNode::Action`]s;
//! - an op-`0x44` record spawn becomes a [`DialogNode::Event`] holding the
//!   spawned record's own text, when it has any.
//!
//! The walk ends a path at the raw `0x21` the dialog SM stops on (the
//! conversation-end teardown, `0x80039E20`), at a jump that leaves the record,
//! or at an instruction the decoder cannot size ([`EndReason::Undecoded`] -
//! labelled, never guessed past). It is cycle-safe and bounded: every
//! instruction is expanded once, a path that reaches an already-expanded
//! instruction becomes a [`DialogNode::Goto`] to the node that covers it, and
//! [`MAX_NODES`] / [`MAX_DEPTH`] cap the rest.
//!
//! Inside a spawned event record `0x21` is the per-frame yield of an ordinary
//! field-VM context, not an end, so an event's walk steps over it and ends at
//! the record bound or a loop.
//!
//! REF: FUN_80039B7C (the dialog SM and its `0x21` stop), FUN_80038050 (picker
//!      jumps), FUN_801DE840 (the branch arms)

use super::*;
use crate::inline_dialogue::{TalkDispatch, talk_dispatch};
use legaia_scene_formats::field_disasm::{self, InventoryCmpKind, StateResumeKind};
use std::collections::HashMap;

/// Node budget per tree. A town NPC's tree is tens of nodes; the cap only
/// bites on a pathological record.
pub const MAX_NODES: usize = 4000;
/// Nesting budget (branches / choices inside branches).
pub const MAX_DEPTH: usize = 64;

/// One text line: the glyph bytes between its `0x1F` lead and its terminator,
/// escapes included (`0xC1 xx` names and the like are left to the host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogLine {
    /// MAN offset of the line's `0x1F` lead.
    pub lead: usize,
    /// Glyph bytes (no lead, no terminator).
    pub bytes: Vec<u8>,
}

/// How a page is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStyle {
    /// The dialog window (`0x1F` lines typed by the pager).
    Box,
    /// A `4C E1` text balloon over an actor.
    Balloon,
}

/// Which one-bit bank a `4C A0..=A2` test reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitBank {
    /// `4C A0`: the context flag word `ctx[+0x10]`.
    Context,
    /// `4C A1`: the script-local word `ctx[+0x62]`.
    Local,
    /// `4C A2`: the scratchpad story word `_DAT_1F800394`.
    Global,
}

/// What a conditional jump tests. The branch's `taken` arm runs when the
/// condition holds; `otherwise` is the fall-through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `0x7x` TEST: story flag `flag` (bank `0x80085758`) is set.
    StoryFlag { flag: u16 },
    /// `0x42` mode 0: bit `bit` of `_DAT_8007B8F4` is set.
    SceneBit { bit: u8 },
    /// `0x42` mode 1: the held pad matches selector `sel` (`0..8` compass,
    /// `8` Circle, `9` Cross, `10` Square, `11` Triangle).
    PadHeld { sel: u8 },
    /// `0x4D`: the context's tile is outside the tile box.
    OutsideTileBox { x0: u8, z0: u8, x1: u8, z1: u8 },
    /// `0x4E`: a loader value compared against an operand. `loader` is the
    /// sub-op (`0/1` HP/MP fraction, `2` level, `3` gold, `4` random byte,
    /// `5..=8` script counter slot, `9` coins, `10/11` 32-bit gold/coins);
    /// `below` is mode `0` (value < arg), else mode `1` (value > arg).
    Compare {
        loader: u8,
        page: u8,
        below: bool,
        arg: i64,
    },
    /// `0x4E` with a compare mode outside `0/1`: retail never takes it.
    CompareUnknownMode { loader: u8, mode: u8 },
    /// `4C A0..=A2`: bit `bit` of a one-bit bank is set.
    BitSet { bank: BitBank, bit: u8 },
    /// `4C 8B`: a present party member knows spell `spell`.
    KnowsSpell { spell: u8 },
    /// `4C 8C`: the context word `ctx[+0x68]` is zero.
    ContextWordZero,
    /// `4C 8D`: character `ch` has learned art `art`.
    LearnedArt { ch: u8, art: u8 },
    /// `4C D1`: a cutscene camera glide is live.
    CameraGlideLive,
    /// `4C DC`: the party holds item `item`.
    HoldsItem { item: u8 },
    /// `4C DE`: someone wears item `item`.
    WearsItem { item: u8 },
    /// `4C E4`: the actor stands outside a world-unit box (raw box bytes).
    OutsideWorldBox { bytes: [u8; 4] },
    /// `4C EB`: no field context answers to actor `actor`.
    ActorAbsent { actor: u8 },
}

/// A side effect worth showing in a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAction {
    /// `0x39`: give one of `item`.
    GiveItem { item: u8 },
    /// `4C 52`: take one of `item`.
    TakeItem { item: u8 },
    /// `0x3A`: add (or, negative, take) gold.
    Money { delta: i32 },
    /// `0x3C`.
    PartyJoin { ch: u8 },
    /// `0x3D`.
    PartyLeave { ch: u8 },
    /// `0x3B`.
    Equip { selector: u8, item: u8 },
    /// `0x5x`.
    SetFlag { flag: u16 },
    /// `0x6x`.
    ClearFlag { flag: u16 },
    /// `0x3F`: named scene change.
    SceneChange { name: Option<String> },
    /// `0x3E` with a door-warp `op0`: mode-24 minigame entry.
    Minigame { sub_id: u8 },
    /// `0x3E` otherwise: a scripted battle (`row` = MAN formation row).
    Battle { op0: u8, row: u8 },
    /// `0x49`: a submode park - `0` the inline shop, `1` the save menu, `7`
    /// the prize exchange, others by sub-op number.
    Menu { sub_op: u8 },
    /// `4C E2`: play movie `fmv_id`.
    Movie { fmv_id: i16 },
    /// `0x44` whose record has no text: spawn of flat MAN record `record`.
    Spawn { record: u8 },
}

/// Why a path ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    /// The raw `0x21` the dialog SM stops on: the conversation is over.
    Close,
    /// A finished page is followed by a byte the dialog SM does not continue
    /// on (inside a spawned event, where there is no next talk to show).
    Parked,
    /// A jump left the record's bytes.
    LeavesRecord { target: usize },
    /// The path ran to the record's end bound.
    RecordEnd,
    /// The decoder cannot size the instruction here: `opcode` (with `sub_op`
    /// for a sub-dispatched op). Nothing past it is guessed.
    Undecoded { opcode: u8, sub_op: Option<u8> },
    /// A picker whose region does not decode.
    BadPicker,
    /// [`MAX_NODES`] or [`MAX_DEPTH`] reached.
    Budget,
}

/// One node. `id` is unique within its tree and is what a [`DialogNode::Goto`]
/// points at; `pc` is the MAN offset of the instruction the node came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogNode {
    Page {
        id: usize,
        pc: usize,
        style: PageStyle,
        lines: Vec<DialogLine>,
    },
    Choice {
        id: usize,
        pc: usize,
        options: Vec<ChoiceOption>,
    },
    Branch {
        id: usize,
        pc: usize,
        cond: Condition,
        /// The actor a cross-context (`0x80`-prefixed) test reads instead of
        /// the record's own context.
        actor: Option<u8>,
        /// The jump-taken arm (the condition holds).
        taken: Vec<DialogNode>,
        /// The fall-through arm.
        otherwise: Vec<DialogNode>,
    },
    Action {
        id: usize,
        pc: usize,
        action: DialogAction,
    },
    /// An op-`0x44` spawn of a record that carries text: the record's own tree.
    Event {
        id: usize,
        pc: usize,
        record: u8,
        body: Vec<DialogNode>,
    },
    /// The talk ends after a page whose next byte the dialog SM does not
    /// continue on (`FUN_80038050`'s default arm): the record's cursor parks
    /// on that byte, and `then` is what the **next** talk to this actor runs
    /// from there.
    NextTalk {
        id: usize,
        pc: usize,
        then: Vec<DialogNode>,
    },
    /// The path joins one already shown: continue at node `to`. `to == None`
    /// when the joined instruction emitted no node of its own before the
    /// path looped back to it (a wait loop).
    Goto {
        id: usize,
        pc: usize,
        to: Option<usize>,
    },
    End {
        id: usize,
        pc: usize,
        reason: EndReason,
    },
}

/// One picker option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    pub label: DialogLine,
    /// What the option's jump runs.
    pub then: Vec<DialogNode>,
}

/// A placement's conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogTree {
    /// Partition-1 index of the record the tree was walked from - the
    /// placement's own, or the actor a talk proxy hands its touch to.
    pub record_placement: usize,
    /// `true` when the placement is a talk proxy for `record_placement`.
    pub via_proxy: bool,
    /// MAN offset of the interaction entry.
    pub entry: usize,
    pub nodes: Vec<DialogNode>,
    /// Number of nodes in the tree.
    pub node_count: usize,
}

impl DialogNode {
    pub fn id(&self) -> usize {
        match self {
            Self::Page { id, .. }
            | Self::Choice { id, .. }
            | Self::Branch { id, .. }
            | Self::Action { id, .. }
            | Self::Event { id, .. }
            | Self::NextTalk { id, .. }
            | Self::Goto { id, .. }
            | Self::End { id, .. } => *id,
        }
    }

    /// Visit this node and every node under it.
    pub fn visit<'a>(&'a self, f: &mut impl FnMut(&'a DialogNode)) {
        f(self);
        match self {
            Self::Choice { options, .. } => {
                for o in options {
                    for n in &o.then {
                        n.visit(f);
                    }
                }
            }
            Self::Branch {
                taken, otherwise, ..
            } => {
                for n in taken.iter().chain(otherwise) {
                    n.visit(f);
                }
            }
            Self::Event { body: v, .. } | Self::NextTalk { then: v, .. } => {
                for n in v {
                    n.visit(f);
                }
            }
            _ => {}
        }
    }
}

impl DialogTree {
    /// Visit every node in the tree.
    pub fn visit<'a>(&'a self, mut f: impl FnMut(&'a DialogNode)) {
        for n in &self.nodes {
            n.visit(&mut f);
        }
    }

    /// Number of text pages anywhere in the tree.
    pub fn page_count(&self) -> usize {
        let mut n = 0;
        self.visit(|x| {
            if matches!(x, DialogNode::Page { .. }) {
                n += 1;
            }
        });
        n
    }
}

impl Condition {
    /// A plain-English label for the condition, phrased as the test that
    /// selects the `taken` arm.
    pub fn describe(&self) -> String {
        match self {
            Self::StoryFlag { flag } => format!("story flag 0x{flag:03X} is set"),
            Self::SceneBit { bit } => format!("scene state bit {bit} (0x8007B8F4) is set"),
            Self::PadHeld { sel } => {
                const DIRS: [&str; 8] = [
                    "Down",
                    "Down+Left",
                    "Left",
                    "Left+Up",
                    "Up",
                    "Up+Right",
                    "Right",
                    "Right+Down",
                ];
                let what = match sel {
                    0..=7 => DIRS[*sel as usize],
                    8 => "Circle",
                    9 => "Cross",
                    10 => "Square",
                    11 => "Triangle",
                    _ => "any",
                };
                format!("the pad holds {what}")
            }
            Self::OutsideTileBox { x0, z0, x1, z1 } => {
                format!("the actor stands outside tiles ({x0},{z0})-({x1},{z1})")
            }
            Self::Compare {
                loader,
                page,
                below,
                arg,
            } => {
                let rel = if *below { "below" } else { "above" };
                match loader {
                    0 => format!("character {page}'s HP is {rel} {arg}/256 of max"),
                    1 => format!("character {page}'s MP is {rel} {arg}/256 of max"),
                    2 => format!("character {page}'s level is {rel} {arg}"),
                    3 | 10 => format!("gold is {rel} {arg}"),
                    4 => format!("a random byte (0-255) is {rel} {arg}"),
                    5..=8 => format!("script counter {} is {rel} {arg}", loader - 5),
                    9 | 11 => format!("coins are {rel} {arg}"),
                    _ => format!("compare loader {loader} is {rel} {arg}"),
                }
            }
            Self::CompareUnknownMode { loader, mode } => {
                format!("compare loader {loader}, mode {mode} (never taken)")
            }
            Self::BitSet { bank, bit } => {
                let b = match bank {
                    BitBank::Context => "context flag",
                    BitBank::Local => "local flag",
                    BitBank::Global => "scratch story flag",
                };
                format!("{b} bit {bit} is set")
            }
            Self::KnowsSpell { spell } => format!("a party member knows spell 0x{spell:02X}"),
            Self::ContextWordZero => "the actor's wait counter is zero".to_string(),
            Self::LearnedArt { ch, art } => format!("character {ch} has learned art {art}"),
            Self::CameraGlideLive => "a camera glide is running".to_string(),
            Self::HoldsItem { item } => format!("the party holds item 0x{item:02X}"),
            Self::WearsItem { item } => format!("someone wears item 0x{item:02X}"),
            Self::OutsideWorldBox { bytes } => format!(
                "the actor stands outside box ({},{})-({},{})",
                bytes[0], bytes[1], bytes[2], bytes[3]
            ),
            Self::ActorAbsent { actor } => format!("actor 0x{actor:02X} is not present"),
        }
    }
}

/// The conversation of partition-1 placement `p`, or `None` when it has no
/// interaction to walk (a prop, a door).
///
/// The record walked is the placement's own interaction record
/// ([`placement_interaction_record`]), else its scripted menu press
/// ([`placement_scripted_menu_record`]), else - for a talk proxy - the target
/// actor's interaction record ([`placement_talk_proxy_target`]).
pub fn placement_dialog_tree(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Option<DialogTree> {
    let own = placement_interaction_record(man_file, man, p)
        .or_else(|| placement_scripted_menu_record(man_file, man, p));
    let (rec_p, via_proxy, record) = match own {
        Some(r) => (p.clone(), false, r),
        None => {
            let target = placement_talk_proxy_target(man_file, man, p)?;
            let tp = man_file
                .actor_placements(man)
                .into_iter()
                .find(|x| x.index == target)?;
            let r = placement_interaction_record(man_file, man, &tp)
                .or_else(|| placement_scripted_menu_record(man_file, man, &tp))?;
            (tp, true, r)
        }
    };
    let start = rec_p.record_offset;
    let end = start + record.body.len();
    // The interaction cursor is one instruction past the spawn section's
    // `0x21`. `placement_interaction_record` bounds that scan at the first
    // text line and so misses a terminator sitting right in front of it
    // (`21 1F ..`, a record whose interaction is that line); the decoder
    // steps over text, so the scan here runs the whole record.
    let entry =
        start + placement_interaction_entry_pc(&record.body, rec_p.script_pc0, record.body.len());
    let mut w = Walker {
        man_file,
        man,
        next_id: 0,
        covered: HashMap::new(),
        pending: Vec::new(),
    };
    let mut nodes = w.walk(entry, start, end, 0, false);
    let covered = std::mem::take(&mut w.covered);
    // Resolve each goto through any chain of gotos to the node it finally
    // shows; one that lands on an end is that end.
    let mut ends = HashMap::new();
    let mut goto_pcs = HashMap::new();
    for n in &nodes {
        n.visit(&mut |x| match x {
            DialogNode::End { id, reason, .. } => {
                ends.insert(*id, reason.clone());
            }
            DialogNode::Goto { id, pc, .. } => {
                goto_pcs.insert(*id, *pc);
            }
            _ => {}
        });
    }
    let mut targets = HashMap::new();
    for (&g, &gpc) in &goto_pcs {
        let mut t = covered.get(&gpc).copied();
        let mut hops = 0;
        while let Some(cur) = t {
            if cur == g || hops > 64 {
                t = None;
                break;
            }
            match goto_pcs.get(&cur) {
                Some(next_pc) => t = covered.get(next_pc).copied(),
                None => break,
            }
            hops += 1;
        }
        targets.insert(g, t);
    }
    resolve_gotos(&mut nodes, &targets, &ends);
    let mut tree = DialogTree {
        record_placement: rec_p.index,
        via_proxy,
        entry,
        nodes,
        node_count: 0,
    };
    // Ids are allocated as the walk goes and a text-free event's expansion
    // is dropped again, so count what the tree holds.
    let mut count = 0;
    tree.visit(|_| count += 1);
    tree.node_count = count;
    Some(tree)
}

struct Walker<'a> {
    man_file: &'a ManFile,
    man: &'a [u8],
    next_id: usize,
    /// Instruction pc -> id of the node that covers it (the first node the
    /// walk emitted at or after it on the path that first reached it).
    covered: HashMap<usize, usize>,
    /// Pcs reached on the current straight-line run with no node yet.
    pending: Vec<usize>,
}

impl Walker<'_> {
    fn id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        for pc in self.pending.drain(..) {
            self.covered.insert(pc, id);
        }
        id
    }

    fn end(&mut self, pc: usize, reason: EndReason) -> DialogNode {
        DialogNode::End {
            id: self.id(),
            pc,
            reason,
        }
    }

    /// Walk one straight-line run from `pc`, recursing at branches.
    fn walk(
        &mut self,
        mut pc: usize,
        start: usize,
        end: usize,
        depth: usize,
        in_event: bool,
    ) -> Vec<DialogNode> {
        // A run starts with nothing pending from its caller's run.
        let saved = std::mem::take(&mut self.pending);
        let out = self.walk_run(&mut pc, start, end, depth, in_event);
        self.pending = saved;
        out
    }

    fn walk_run(
        &mut self,
        pc: &mut usize,
        start: usize,
        end: usize,
        depth: usize,
        in_event: bool,
    ) -> Vec<DialogNode> {
        let man = self.man;
        let mut out = Vec::new();
        let mut page: Option<(usize, Vec<DialogLine>)> = None;
        macro_rules! flush {
            () => {
                if let Some((ppc, lines)) = page.take() {
                    let id = self.id();
                    out.push(DialogNode::Page {
                        id,
                        pc: ppc,
                        style: PageStyle::Box,
                        lines,
                    });
                }
            };
        }
        loop {
            let here = *pc;
            if self.next_id >= MAX_NODES || depth >= MAX_DEPTH {
                flush!();
                out.push(self.end(here, EndReason::Budget));
                return out;
            }
            if here < start || here >= end {
                flush!();
                let reason = if here == end {
                    EndReason::RecordEnd
                } else {
                    EndReason::LeavesRecord { target: here }
                };
                out.push(self.end(here, reason));
                return out;
            }
            // A finished page: the dialog SM classifies the byte after it
            // (`FUN_80038050`, [`talk_dispatch`]) before running anything.
            if page.is_some() && man.get(here) != Some(&0x1F) {
                flush!();
                match talk_dispatch(&man[..end], here) {
                    TalkDispatch::EndAfter(_) => {
                        self.pending.push(here);
                        out.push(self.end(here, EndReason::Close));
                        return out;
                    }
                    // A picker opens from here: decode it below.
                    TalkDispatch::Continue(n) if n == here => {}
                    TalkDispatch::Continue(n) => {
                        self.pending.push(here);
                        *pc = n;
                        continue;
                    }
                    TalkDispatch::EndParked(p) => {
                        let id = self.id();
                        if in_event {
                            out.push(DialogNode::End {
                                id,
                                pc: p,
                                reason: EndReason::Parked,
                            });
                        } else {
                            let then = self.walk(p, start, end, depth + 1, in_event);
                            out.push(DialogNode::NextTalk { id, pc: p, then });
                        }
                        return out;
                    }
                }
            }
            if self.covered.contains_key(&here) || self.pending.contains(&here) {
                flush!();
                let id = self.id();
                out.push(DialogNode::Goto {
                    id,
                    pc: here,
                    to: None,
                });
                return out;
            }
            // Decode against the record's own bytes so nothing reads past it.
            let insn = match field_disasm::decode(&man[..end], here) {
                Ok(i) => i,
                Err(e) => {
                    flush!();
                    self.pending.push(here);
                    let (opcode, sub_op) = match e {
                        field_disasm::DisasmError::UnknownSubOp { opcode, sub_op, .. } => {
                            (opcode, Some(sub_op))
                        }
                        field_disasm::DisasmError::UnknownOpcode { opcode, .. }
                        | field_disasm::DisasmError::Truncated { opcode, .. } => (opcode, None),
                        field_disasm::DisasmError::EndOfStream { .. } => (man[here], None),
                    };
                    out.push(self.end(here, EndReason::Undecoded { opcode, sub_op }));
                    return out;
                }
            };
            let next = here + insn.size.max(1);
            self.pending.push(here);

            // Text: a `0x1F` line joins the open page.
            if let InsnInfo::TextSegment { len } = insn.info {
                let line = DialogLine {
                    lead: here,
                    bytes: man[here + 1..here + 1 + len].to_vec(),
                };
                match page.as_mut() {
                    Some((_, lines)) => lines.push(line),
                    None => page = Some((here, vec![line])),
                }
                *pc = next;
                continue;
            }
            // The dialog SM's stop.
            if !in_event && insn.extended.is_none() && man[here] == 0x21 {
                out.push(self.end(here, EndReason::Close));
                return out;
            }

            match insn.info {
                InsnInfo::Picker { .. } => {
                    let Some(pk) = legaia_mes::picker::parse_picker_at(&man[..end], here) else {
                        out.push(self.end(here, EndReason::BadPicker));
                        return out;
                    };
                    let id = self.id();
                    let mut options = Vec::new();
                    for (i, o) in pk.options.iter().enumerate() {
                        let lend = legaia_mes::dialog_box::line_end(&man[..end], o.lead + 1);
                        let label = DialogLine {
                            lead: o.lead,
                            bytes: man[o.lead + 1..lend.max(o.lead + 1)].to_vec(),
                        };
                        let then = match pk.jump_target(i) {
                            Some(t) => self.walk(t, start, end, depth + 1, in_event),
                            None => vec![self.end(here, EndReason::LeavesRecord { target: 0 })],
                        };
                        options.push(ChoiceOption { label, then });
                    }
                    out.push(DialogNode::Choice {
                        id,
                        pc: here,
                        options,
                    });
                    return out;
                }
                InsnInfo::JmpRel { target, .. } => {
                    *pc = target;
                    continue;
                }
                // `sel >= 0xC` takes the jump with no test.
                InsnInfo::CondJmp {
                    mode: 1,
                    op1,
                    target,
                    ..
                } if op1 >= 0xC => {
                    *pc = target;
                    continue;
                }
                // Mode >= 2 halts at the PC forever in retail.
                InsnInfo::CondJmp { mode, .. } if mode >= 2 => {
                    out.push(self.end(
                        here,
                        EndReason::Undecoded {
                            opcode: 0x42,
                            sub_op: Some(mode),
                        },
                    ));
                    return out;
                }
                _ => {}
            }

            if let Some((cond, target)) = branch_of(man, &insn) {
                let id = self.id();
                let taken = self.walk(target, start, end, depth + 1, in_event);
                let otherwise = self.walk(next, start, end, depth + 1, in_event);
                out.push(DialogNode::Branch {
                    id,
                    pc: here,
                    cond,
                    actor: insn.extended,
                    taken,
                    otherwise,
                });
                return out;
            }

            // A text balloon is a one-line page of its own.
            if let InsnInfo::MenuCtrl {
                kind:
                    field_disasm::MenuCtrlKind::HighNibble {
                        outer: 0xE, sub: 1, ..
                    },
                ..
            } = insn.info
                && let Some(lead) = field_disasm::text_lead(&man[..end], &insn)
            {
                let lend = legaia_mes::dialog_box::line_end(&man[..end], lead + 1);
                let id = self.id();
                out.push(DialogNode::Page {
                    id,
                    pc: here,
                    style: PageStyle::Balloon,
                    lines: vec![DialogLine {
                        lead,
                        bytes: man[lead + 1..lend.max(lead + 1)].to_vec(),
                    }],
                });
                *pc = next;
                continue;
            }

            if let InsnInfo::SpawnRecord { global_index } = insn.info {
                let id = self.id();
                let body = if !in_event {
                    flat_record_span(self.man_file, man, usize::from(global_index)).map(
                        |(s, pc0, len)| {
                            // An event's own expansion never shares nodes
                            // with the conversation's: its record is disjoint.
                            self.walk(s + pc0, s, s + len, depth + 1, true)
                        },
                    )
                } else {
                    None
                };
                let has_text = body.as_ref().is_some_and(|b| {
                    let mut t = false;
                    for n in b {
                        n.visit(&mut |x| t |= matches!(x, DialogNode::Page { .. }));
                    }
                    t
                });
                out.push(match body {
                    Some(body) if has_text => DialogNode::Event {
                        id,
                        pc: here,
                        record: global_index,
                        body,
                    },
                    _ => DialogNode::Action {
                        id,
                        pc: here,
                        action: DialogAction::Spawn {
                            record: global_index,
                        },
                    },
                });
                *pc = next;
                continue;
            }

            if let Some(action) = action_of(man, &insn) {
                let id = self.id();
                out.push(DialogNode::Action {
                    id,
                    pc: here,
                    action,
                });
            }
            *pc = next;
        }
    }
}

/// The condition and jump target of a conditional jump, per the
/// branch-encoding table of `docs/subsystems/script-vm.md`.
fn branch_of(man: &[u8], insn: &field_disasm::Insn) -> Option<(Condition, usize)> {
    let pc = insn.pc;
    // The `4C` skip exit: a signed halfword in the instruction's last two
    // bytes, added less two to the PC past the instruction.
    let skip = || {
        let e = pc + insn.size;
        let s = i16::from_le_bytes([*man.get(e - 2)?, *man.get(e - 1)?]);
        Some((e as isize + s as isize - 2) as usize)
    };
    let operand = pc + if insn.extended.is_some() { 2 } else { 1 };
    match &insn.info {
        InsnInfo::SystemFlag {
            kind: FlagKind::Test,
            idx,
            target: Some(t),
            ..
        } => Some((Condition::StoryFlag { flag: *idx }, *t)),
        InsnInfo::CondJmp {
            mode: 0,
            op1,
            target,
            ..
        } => Some((Condition::SceneBit { bit: op1 & 0x1F }, *target)),
        InsnInfo::CondJmp {
            mode: 1,
            op1,
            target,
            ..
        } => Some((Condition::PadHeld { sel: *op1 }, *target)),
        InsnInfo::BBoxTest {
            x_min,
            z_min,
            x_max,
            z_max,
            skip_target,
            ..
        } => Some((
            Condition::OutsideTileBox {
                x0: *x_min,
                z0: *z_min,
                x1: *x_max,
                z1: *z_max,
            },
            *skip_target,
        )),
        InsnInfo::InventoryCmp {
            page,
            mode_byte,
            kind,
        } => {
            let mode = mode_byte & 0x0F;
            let (loader, arg, t) = match kind {
                InventoryCmpKind::Compare {
                    sub_op,
                    arg,
                    skip_target,
                    ..
                } => (*sub_op, i64::from(*arg), *skip_target),
                InventoryCmpKind::PartyBank {
                    sub_op,
                    scaled,
                    skip_target,
                    ..
                } => (*sub_op, i64::from(*scaled), *skip_target),
                InventoryCmpKind::DefaultAdvance => return None,
            };
            let cond = if mode <= 1 {
                Condition::Compare {
                    loader,
                    page: *page,
                    below: mode == 0,
                    arg,
                }
            } else {
                Condition::CompareUnknownMode { loader, mode }
            };
            Some((cond, t))
        }
        InsnInfo::MenuCtrl { op0, .. } => {
            let b = |k: usize| man.get(operand + k).copied();
            let cond = match *op0 {
                0xA0 => Condition::BitSet {
                    bank: BitBank::Context,
                    bit: b(1)? & 0x1F,
                },
                0xA1 => Condition::BitSet {
                    bank: BitBank::Local,
                    bit: b(1)? & 0x1F,
                },
                0xA2 => Condition::BitSet {
                    bank: BitBank::Global,
                    bit: b(1)? & 0x1F,
                },
                0x8B => Condition::KnowsSpell { spell: b(1)? },
                0x8C => Condition::ContextWordZero,
                0x8D => Condition::LearnedArt {
                    ch: b(1)?,
                    art: b(2)?,
                },
                0xD1 => Condition::CameraGlideLive,
                0xDC => Condition::HoldsItem { item: b(1)? },
                0xDE => Condition::WearsItem { item: b(1)? },
                0xE4 => Condition::OutsideWorldBox {
                    bytes: [b(1)?, b(2)?, b(3)?, b(4)?],
                },
                0xEB => Condition::ActorAbsent { actor: b(1)? },
                _ => return None,
            };
            Some((cond, skip()?))
        }
        _ => None,
    }
}

/// The conversation-visible side effect of a straight-line instruction.
fn action_of(man: &[u8], insn: &field_disasm::Insn) -> Option<DialogAction> {
    Some(match &insn.info {
        InsnInfo::GiveItem { item_id } => DialogAction::GiveItem { item: *item_id },
        InsnInfo::MenuCtrl {
            kind: field_disasm::MenuCtrlKind::Nibble5TakeItem { item_id },
            ..
        } => DialogAction::TakeItem { item: *item_id },
        InsnInfo::AddMoney { signed_24 } => DialogAction::Money { delta: *signed_24 },
        InsnInfo::PartyAdd { char_id } => DialogAction::PartyJoin { ch: *char_id },
        InsnInfo::PartyRemove { char_id } => DialogAction::PartyLeave { ch: *char_id },
        InsnInfo::SetEquipSlot { selector, item } => DialogAction::Equip {
            selector: *selector,
            item: *item,
        },
        InsnInfo::SystemFlag {
            kind: FlagKind::Set,
            idx,
            ..
        } => DialogAction::SetFlag { flag: *idx },
        InsnInfo::SystemFlag {
            kind: FlagKind::Clear,
            idx,
            ..
        } => DialogAction::ClearFlag { flag: *idx },
        InsnInfo::SceneChange { .. } => DialogAction::SceneChange {
            name: field_disasm::scene_change_name(man, insn),
        },
        InsnInfo::WarpOrInteract {
            op0, op1, is_warp, ..
        } => {
            if *is_warp && is_genuine_warp(*op0, insn.extended) {
                DialogAction::Minigame { sub_id: op0 - 100 }
            } else if insn.extended.is_none() && !*is_warp {
                DialogAction::Battle {
                    op0: *op0,
                    row: *op1,
                }
            } else {
                return None;
            }
        }
        InsnInfo::StateResume { sub_op, kind } => match kind {
            StateResumeKind::DoneSub0Shop { .. } => DialogAction::Menu { sub_op: 0 },
            _ => DialogAction::Menu { sub_op: *sub_op },
        },
        InsnInfo::MenuCtrl {
            kind: field_disasm::MenuCtrlKind::FmvTrigger { fmv_id },
            ..
        } => DialogAction::Movie { fmv_id: *fmv_id },
        _ => return None,
    })
}

/// Point every [`DialogNode::Goto`] at its resolved target (`targets`, goto
/// id -> final node id). A goto that lands on an end is that end: the path is
/// over either way.
fn resolve_gotos(
    nodes: &mut [DialogNode],
    targets: &HashMap<usize, Option<usize>>,
    ends: &HashMap<usize, EndReason>,
) {
    for n in nodes {
        match n {
            DialogNode::Goto { id, pc, to } => {
                *to = targets.get(id).copied().flatten();
                if let Some(reason) = to.and_then(|t| ends.get(&t)) {
                    *n = DialogNode::End {
                        id: *id,
                        pc: *pc,
                        reason: reason.clone(),
                    };
                }
            }
            DialogNode::Choice { options, .. } => {
                for o in options {
                    resolve_gotos(&mut o.then, targets, ends);
                }
            }
            DialogNode::Branch {
                taken, otherwise, ..
            } => {
                resolve_gotos(taken, targets, ends);
                resolve_gotos(otherwise, targets, ends);
            }
            DialogNode::Event { body: v, .. } | DialogNode::NextTalk { then: v, .. } => {
                resolve_gotos(v, targets, ends)
            }
            _ => {}
        }
    }
}
