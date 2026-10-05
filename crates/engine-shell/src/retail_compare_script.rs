//! The field-script half of the retail comparison corpus: which field-VM
//! contexts a retail state holds, and where in their bytecode they stand.
//!
//! Every field context is an actor node on the five SCUS actor lists
//! (`_DAT_8007C34C..`, walked by `FUN_8002519C` through the node's `+0x00`
//! link). A context's script is `(base, pc)`: `+0x90` is the absolute RAM
//! address of the record's script start and `+0x9E` the signed-16 PC into it
//! (`FUN_80039B7C` reads both at `0x80039BD4` / `0x80039BD8`), `+0x50` is the
//! record's **flat** MAN index (`FUN_8003BDE0` stores `N0 + N1 + i` for a
//! spawned partition-2 record, `FUN_8003A1E4` `N0 + i` for a placement), and
//! `+0x10 & 0x100` is the engaged bit a running interaction holds
//! (`docs/subsystems/script-vm.md`, "Engagement and the system script").
//!
//! A state caught inside a running record names the record and the PC, and
//! the seed runs the engine's own copy of the record to that phase
//! ([`ScriptGate`]) instead of sampling a fixed window after the entry. The
//! scene system script (tick `FUN_801DA51C`, not the field actor tick) and
//! the player are not candidates: the first runs every frame whatever the
//! capture shows, the second is driven by the pad.

use legaia_mednafen::game_anchors;

/// The actor-list heads `FUN_8002519C` walks (`_DAT_8007C34C` onward).
const ACTOR_LIST_HEADS: u32 = 0x8007_C34C;
/// Heads scanned from [`ACTOR_LIST_HEADS`] (four-byte stride, the player
/// pointer `_DAT_8007C364` among them is not a list head but reads as one
/// node chain at worst, which the tick filter drops).
const ACTOR_LIST_SPAN: u32 = 9;
/// Per-list walk bound (the actor pool is a small fixed table).
const ACTOR_LIST_MAX: usize = 512;
/// `FUN_8003BC08`, the field actor tick that routes an engaged context to
/// the script runner.
const FIELD_ACTOR_TICK: u32 = 0x8003_BC08;
/// The engaged bit.
const ENGAGED: u32 = 0x100;
/// `FUN_801DC0BC`, the cutscene camera mover's tick: progress `+0x9C`
/// counts up to the duration `+0x9E`, and `+0x10 & 0x8` marks it dead the
/// frame it lands (`0x801DD238..0x801DD260`).
const CAMERA_MOVER_TICK: u32 = 0x801D_C0BC;
/// A mover's dead bit.
const DEAD: u32 = 0x8;
/// Bytes of a record read to identify it.
pub const RECORD_HEAD_LEN: usize = 48;

/// A running (engaged) field context in a retail state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetailScript {
    /// `ctx[+0x50]`, the record's flat MAN index.
    pub flat_index: u16,
    /// `ctx[+0x9E]`, the PC relative to the record's script start.
    pub pc: usize,
    /// `ctx[+0x54]`, the wait accumulator an op-`0x4A` wait counts up.
    pub wait: i16,
    /// The opcode byte at the PC.
    pub op: u8,
    /// The record's first [`RECORD_HEAD_LEN`] bytes from its script start.
    pub head: Vec<u8>,
}

/// The field-script observables of one retail state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetailScripts {
    /// Engaged contexts other than the system script and the player, in
    /// actor-list order.
    pub running: Vec<RetailScript>,
    /// Display frames left on a live camera-mover glide, when one is in
    /// flight (duration `+0x9E` less progress `+0x9C`).
    pub glide_left: Option<i32>,
}

fn in_ram(p: u32) -> bool {
    (p & 0xFFE0_0000) == 0x8000_0000
}

fn bytes_at(ram: &[u8], va: u32, len: usize) -> Option<Vec<u8>> {
    let lo = (va & 0x1F_FFFF) as usize;
    ram.get(lo..lo + len).map(<[u8]>::to_vec)
}

/// Every node on the actor lists, each once, in list order.
fn actor_nodes(ram: &[u8]) -> Vec<u32> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for k in 0..ACTOR_LIST_SPAN {
        let mut p = game_anchors::u32_at(ram, ACTOR_LIST_HEADS + 4 * k);
        let mut n = 0;
        while in_ram(p) && n < ACTOR_LIST_MAX && seen.insert(p) {
            out.push(p);
            p = game_anchors::u32_at(ram, p);
            n += 1;
        }
    }
    out
}

impl RetailScripts {
    /// Read the field contexts of a retail state's RAM.
    pub fn from_ram(ram: &[u8]) -> Self {
        let player = game_anchors::player_ptr(ram);
        let mut out = Self::default();
        for node in actor_nodes(ram) {
            let tick = game_anchors::u32_at(ram, node + 0x0C);
            let flags = game_anchors::u32_at(ram, node + 0x10);
            let id = game_anchors::u16_at(ram, node + 0x50);
            if tick == CAMERA_MOVER_TICK && flags & DEAD == 0 && out.glide_left.is_none() {
                let left = i32::from(game_anchors::i16_at(ram, node + 0x9E))
                    - i32::from(game_anchors::i16_at(ram, node + 0x9C));
                out.glide_left = Some(left.max(0));
                continue;
            }
            let base = game_anchors::u32_at(ram, node + 0x90);
            if !in_ram(base) {
                continue;
            }
            let head = bytes_at(ram, base, RECORD_HEAD_LEN);
            if tick != FIELD_ACTOR_TICK || flags & ENGAGED == 0 || Some(node) == player {
                continue;
            }
            let pc = game_anchors::i16_at(ram, node + 0x9E);
            let (Ok(pc), Some(head)) = (usize::try_from(pc), head) else {
                continue;
            };
            out.running.push(RetailScript {
                flat_index: id,
                pc,
                wait: game_anchors::i16_at(ram, node + 0x54),
                op: game_anchors::u8_at(ram, base.wrapping_add(pc as u32)),
                head,
            });
        }
        out
    }
}

/// Where an engine field context lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineContextKind {
    /// The modal cutscene timeline (`World::cutscene.timeline`).
    Timeline,
    /// A concurrent spawned-record context (`FieldVmState::helper_contexts`).
    Helper(usize),
    /// A placement channel (`FieldVmState::channels`).
    Channel(usize),
    /// The talk runner (`DialogState::inline`): a placement's interaction
    /// body the player engaged, the engine's twin of retail's engaged
    /// placement context.
    Inline,
}

/// One engine field context: where it lives, its record head and its PC.
#[derive(Debug, Clone)]
pub struct EngineContext {
    pub kind: EngineContextKind,
    pub head: Vec<u8>,
    pub pc: usize,
    pub wait: i16,
    pub done: bool,
    /// The context is parked on an open dialog box.
    pub dialog_open: bool,
}

fn head_of(bytes: &[u8]) -> Vec<u8> {
    bytes[..bytes.len().min(RECORD_HEAD_LEN)].to_vec()
}

/// The engine's field contexts, each with its record head in the same frame
/// as [`RetailScript::head`] (bytes from the record's script start).
pub fn engine_contexts(world: &legaia_engine_core::world::World) -> Vec<EngineContext> {
    let mut out = Vec::new();
    if let Some(tl) = world.cutscene.timeline.as_ref() {
        out.push(EngineContext {
            kind: EngineContextKind::Timeline,
            head: head_of(&tl.bytecode),
            pc: tl.pc,
            wait: tl.ctx.wait_accum,
            done: tl.done,
            dialog_open: tl.dialog.is_some(),
        });
    }
    for (i, h) in world.field_vm.helper_contexts.iter().enumerate() {
        out.push(EngineContext {
            kind: EngineContextKind::Helper(i),
            head: head_of(&h.bytecode),
            pc: h.pc,
            wait: h.ctx.wait_accum,
            done: h.done,
            dialog_open: h.dialog.is_some(),
        });
    }
    if let Some(id) = world
        .dialog
        .inline
        .as_ref()
        .filter(|id| id.prop_anchor.is_none())
    {
        out.push(EngineContext {
            kind: EngineContextKind::Inline,
            head: head_of(&id.bytecode),
            pc: id.pc,
            wait: id.ctx.wait_accum,
            done: id.done,
            dialog_open: id.panel.is_some(),
        });
    }
    if let Some(man) = world.field_vm.channels_man.as_deref() {
        for (i, c) in world.field_vm.channels.iter().enumerate() {
            let Some(body) = man.get(c.record_offset..) else {
                continue;
            };
            out.push(EngineContext {
                kind: EngineContextKind::Channel(i),
                head: head_of(body),
                pc: c.pc,
                wait: c.ctx.wait_accum,
                done: c.done,
                dialog_open: false,
            });
        }
    }
    out
}

/// The engine context running the same record as `script`, if any.
pub fn find_engine_context<'a>(
    contexts: &'a [EngineContext],
    script: &RetailScript,
) -> Option<&'a EngineContext> {
    contexts.iter().find(|c| c.head == script.head)
}

/// Engine ticks a script-gated seed runs before giving up on the phase.
pub const SCRIPT_GATE_DEADLINE: u64 = 9000;

/// A retail capture's script phase: the running record (by its head bytes),
/// the PC it is parked on and how far into an op-`0x4A` wait it is. The seed
/// runs the engine until its own context for that record holds the same
/// phase, rather than for a fixed window - the field twin of the battle
/// half's [`crate::retail_compare_battle::PhaseGate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGate {
    /// The record's flat MAN index (`ctx[+0x50]`).
    pub flat_index: u16,
    pub head: Vec<u8>,
    pub pc: usize,
    pub wait: i16,
    /// The opcode byte at the PC. A text segment (`& 0x7F < 0x20`) is a
    /// capture on an open dialog box, met once the engine's box has typed
    /// its page and waits for the press - the frame every such capture in
    /// the library shows (the whole page up, the prompt glyph on).
    pub op: u8,
    /// Display frames left on retail's camera glide, when one was in flight:
    /// a capture parked on the PC while the shot moves (a `4C CD` wait, a
    /// beat's `0x4A`) is met once the engine's glide has as little left.
    pub glide_left: Option<i32>,
    /// Kept by [`Self::met`]: `.0` while the engine's context for the record
    /// still runs and reaches the gate PC in straight-line code, `.1` once
    /// that context retired. The engine retires a record the tick its tail
    /// runs, so a park retail holds on that tail is never a PC the engine
    /// holds.
    pub retire_watch: std::cell::Cell<(bool, bool)>,
}

impl ScriptGate {
    /// The gate for a retail state's first running context, if any.
    pub fn from_retail(scripts: &RetailScripts) -> Option<Self> {
        scripts.running.first().map(|s| Self {
            flat_index: s.flat_index,
            head: s.head.clone(),
            pc: s.pc,
            wait: s.wait.max(0),
            op: s.op,
            glide_left: scripts.glide_left,
            retire_watch: std::cell::Cell::new((false, false)),
        })
    }

    /// `<flat index>:<head hex>:<pc>:<wait>:<op>[:<glide left>]` -
    /// `LEGAIA_SCRIPT_GATE` for `play-window`.
    pub fn to_env(&self) -> String {
        let hex: String = self.head.iter().map(|b| format!("{b:02x}")).collect();
        let glide = self.glide_left.map(|g| format!(":{g}")).unwrap_or_default();
        format!(
            "{}:{hex}:{}:{}:{}{glide}",
            self.flat_index, self.pc, self.wait, self.op
        )
    }

    /// Inverse of [`Self::to_env`].
    pub fn from_env(s: &str) -> Option<Self> {
        let mut it = s.trim().split(':');
        let flat_index = it.next()?.parse().ok()?;
        let hex = it.next()?;
        let pc = it.next()?.parse().ok()?;
        let wait = it.next()?.parse().ok()?;
        let op = it.next()?.parse().ok()?;
        let glide_left = match it.next() {
            Some(g) => Some(g.parse().ok()?),
            None => None,
        };
        if hex.is_empty() || hex.len() % 2 != 0 {
            return None;
        }
        let head = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        Some(Self {
            flat_index,
            head,
            pc,
            wait,
            op,
            glide_left,
            retire_watch: std::cell::Cell::new((false, false)),
        })
    }

    /// A dialog box with its page typed and the prompt glyph on: a page
    /// waiting for the press that turns it, or the last page waiting for the
    /// press that closes it (`is_done` - the box stays up until that press,
    /// the same pair the window's prompt glyph keys on).
    fn page_up(d: &legaia_engine_core::dialog::OwnedDialogPanel) -> bool {
        d.is_waiting_for_input() || d.is_done()
    }

    /// Whether the engine's context for the record is on the gate's PC, at
    /// least as far into its wait - or has already executed the op there.
    /// Retail's capture is parked on the op (a halt, a wait, a walk); the
    /// engine can clear the same op inside one slice where retail held it
    /// across frames (a channel flag already up, a walk already at its
    /// tile), so the first tick past it is the nearest the engine comes.
    ///
    /// A capture parked on the record's tail - the PC right after a `0x3F`
    /// scene change (the departing scene held behind the streaming actor
    /// `FUN_8001FD44`, the record spinning on its `26 FF FF`), or a fade op
    /// before the record's terminal park - is a PC the engine runs through
    /// in the slice that retires the record, dropping its context. Such a
    /// gate is met once a context that reached the gate PC in straight-line
    /// code (no jump between its PC and the gate's) has retired, and then
    /// held while the camera glide still has more left than retail's.
    /// `map03 P2[13]` (the `son` portal cutscene, `son_arrival_from_doman`)
    /// and `kor5 P2[5]` (`kor5_post_436_organic`, on its `36` fades) end
    /// this way.
    pub fn met(&self, world: &legaia_engine_core::world::World) -> bool {
        let retired = self.context(world).is_none_or(|c| c.done);
        let (armed, passed) = self.retire_watch.get();
        let passed = passed || (armed && retired);
        self.retire_watch
            .set((self.straight_to_gate(world), passed));
        if passed
            && self
                .glide_left
                .is_none_or(|g| world.camera.state.glide_frames <= g)
        {
            return true;
        }
        let text = self.op & 0x7F < 0x20;
        // A capture with no camera mover on its lists holds a landed shot:
        // retail frees the mover (`FUN_801DC0BC`) when its glide ends. So a
        // record parked on the gate PC is met once the engine's glide has
        // landed too - `name_input_ui` is parked on the opening's `49 03`
        // one op after a 16-frame glide that lands under the prompt, and the
        // first tick on that PC is that glide's first frame.
        let glide_landed = match self.glide_left {
            Some(g) => world.camera.state.glide_frames <= g,
            None => world.camera.state.glide_frames <= 0,
        };
        let passed = |tl: &legaia_engine_core::cutscene_timeline::CutsceneTimeline| {
            head_of(&tl.bytecode) == self.head
                && ((tl.pc == self.pc
                    && tl.ctx.wait_accum >= self.wait
                    && glide_landed
                    && (!text || tl.dialog.as_ref().is_some_and(Self::page_up)))
                    || (tl.pc != self.pc && tl.visited.get(self.pc).copied().unwrap_or(false)))
        };
        if world.cutscene.timeline.as_ref().is_some_and(passed)
            || world.field_vm.helper_contexts.iter().any(passed)
        {
            return true;
        }
        // An engaged placement: the engine runs the talk on its inline
        // runner, which holds the PC on a segment's start while its box is
        // up - the same PC retail's `+0x9E` holds.
        if world.dialog.inline.as_ref().is_some_and(|id| {
            id.prop_anchor.is_none()
                && !id.done
                && head_of(&id.bytecode) == self.head
                && ((id.pc == self.pc
                    && id.ctx.wait_accum >= self.wait
                    && (!text || id.panel.as_ref().is_some_and(Self::page_up)))
                    || (id.pc != self.pc && id.visited.get(self.pc).copied().unwrap_or(false)))
        }) {
            return true;
        }
        engine_contexts(world).iter().any(|c| {
            matches!(c.kind, EngineContextKind::Channel(_))
                && c.head == self.head
                && !c.done
                && c.pc == self.pc
                && c.wait >= self.wait
        })
    }

    /// The engine runs the gate's record on the timeline or a concurrent
    /// context whose PC reaches the gate PC in straight-line code: every
    /// instruction from it decodes, none is a jump or a text segment, and
    /// one ends exactly on the gate PC.
    fn straight_to_gate(&self, world: &legaia_engine_core::world::World) -> bool {
        use legaia_asset::field_disasm::{InsnInfo, decode};
        let reaches = |bc: &[u8], mut pc: usize| {
            if head_of(bc) != self.head {
                return false;
            }
            for _ in 0..128 {
                if pc == self.pc {
                    return true;
                }
                if pc > self.pc {
                    return false;
                }
                let Ok(i) = decode(bc, pc) else {
                    return false;
                };
                if i.size == 0
                    || matches!(
                        i.info,
                        InsnInfo::JmpRel { .. }
                            | InsnInfo::CondJmp { .. }
                            | InsnInfo::TextSegment { .. }
                            | InsnInfo::Picker { .. }
                    )
                {
                    return false;
                }
                pc += i.size;
            }
            false
        };
        world
            .cutscene
            .timeline
            .iter()
            .filter(|tl| !tl.done)
            .map(|tl| (tl.bytecode.as_slice(), tl.pc))
            .chain(
                world
                    .field_vm
                    .helper_contexts
                    .iter()
                    .filter(|h| !h.done)
                    .map(|h| (h.bytecode.as_slice(), h.pc)),
            )
            .any(|(bc, pc)| reaches(bc, pc))
    }

    /// The pad word a gated seed holds on `tick`: from
    /// [`SCRIPT_RESUME_TICK`] on, `Cross` on every other tick while the
    /// gate's record sits in a dialog box short of the gate PC - the presses
    /// the player made to page the retail conversation to where it was
    /// captured. Neutral otherwise, and neutral once the context is on the
    /// gate PC (a capture on a box shows that box).
    pub fn advance_pad(&self, world: &legaia_engine_core::world::World, tick: u64) -> u16 {
        if tick < SCRIPT_RESUME_TICK || !tick.is_multiple_of(2) {
            return 0;
        }
        match self.context(world) {
            Some(c) if c.dialog_open && c.pc != self.pc => {
                legaia_engine_core::input::PadButton::Cross.mask()
            }
            _ => 0,
        }
    }

    /// One diagnostic line: the engine context running the gate's record and
    /// the modal timeline's parks (`LEGAIA_RC_SCRIPT_TRACE`).
    pub fn trace(&self, world: &legaia_engine_core::world::World) -> String {
        let c = self.context(world).map(|c| (c.kind, c.pc, c.wait, c.done));
        let parks = world.cutscene.timeline.as_ref().map(|tl| {
            format!(
                "walk={:?} chan={:?} player={:?} facing={} glide={} clip={:?} dialog={:?} frames={}",
                tl.walk_wait,
                tl.channel_wait,
                tl.player_wait,
                tl.facing_wait.is_some(),
                tl.player_glide.is_some(),
                tl.player_clip_wait,
                tl.dialog.as_ref().map(|d| d.state()),
                tl.frames
            )
        });
        let player = world
            .player_actor_slot
            .and_then(|p| world.actors.get(usize::from(p)))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let inline = world.dialog.inline.as_ref().map(|id| {
            (
                id.pc,
                id.done,
                id.panel.is_some(),
                id.npc_slot,
                head_of(&id.bytecode) == self.head,
            )
        });
        format!(
            "{c:?} mode={:?} glide={} player={player:?} {parks:?} inline={inline:?} cur={}",
            world.mode,
            world.camera.state.glide_frames,
            world.dialog.current.is_some()
        )
    }

    /// The engine context running the gate's record, if one is live. An
    /// engaged talk (the inline runner) is preferred over the placement's
    /// idle channel on the same record.
    pub fn context(&self, world: &legaia_engine_core::world::World) -> Option<EngineContext> {
        let all = engine_contexts(world);
        let inline = all
            .iter()
            .position(|c| c.kind == EngineContextKind::Inline && c.head == self.head);
        match inline {
            Some(i) => all.into_iter().nth(i),
            None => all.into_iter().find(|c| c.head == self.head),
        }
    }
}

/// Ticks after the seed at which a record the engine is not running is
/// resumed from its start ([`resume_record`]): the settle window, so a
/// record the scene's own entry starts (the credits' vignette driver) is
/// found running instead, and the settle-window sample stays the ungated one.
pub const SCRIPT_RESUME_TICK: u64 = crate::retail_compare::SETTLE_TICKS;

/// Start the gate's record from its first opcode when no engine context runs
/// it. A capture inside a partition-2 record the card-load resume does not
/// restart - its one-shot gate flag is already set in the save, or its
/// trigger is a walk-on tile the seat does not cross - otherwise never
/// reaches the phase. The record is resolved by the flat index and checked
/// against the capture's own head bytes, installed ungated (its gate flags
/// are the ones it has already latched) as the modal cutscene timeline, or
/// as a concurrent context when another timeline holds that slot. The
/// op-`0x35` words of the arm that spawns the record, when that arm scored it
/// ([`legaia_engine_core::man_field_scripts::walk_spawn_scores`]), are
/// replayed first. Returns `true` when a context was installed.
pub fn resume_record(host: &mut legaia_engine_core::scene::SceneHost, gate: &ScriptGate) -> bool {
    if let Some(c) = gate.context(&host.world) {
        return match c.kind {
            EngineContextKind::Channel(_) => engage_placement(&mut host.world, gate),
            _ => false,
        };
    }
    let Some(Ok(Some(man))) = host
        .scene
        .as_ref()
        .map(|s| s.field_man_payload(&host.index))
    else {
        return false;
    };
    let Ok(mf) = legaia_asset::man_section::parse(&man) else {
        return false;
    };
    let n0 = mf.header.partition_counts[0].max(0) as usize;
    let n1 = mf.header.partition_counts[1].max(0) as usize;
    let Some(idx) = usize::from(gate.flat_index).checked_sub(n0 + n1) else {
        return false;
    };
    let Some((start, pc0, len)) =
        legaia_engine_core::man_field_scripts::partition_record_span(&mf, &man, 2, idx)
    else {
        return false;
    };
    if !man
        .get(start..start + len)
        .is_some_and(|body| body.starts_with(&gate.head))
    {
        return false;
    }
    // The arm that spawns the record may have scored it: replay that arm's
    // track words first, since the resume starts the record without it.
    roll_back_run_latches(&mut host.world, &man[start..start + len], gate.pc);
    if let Some(score) = legaia_engine_core::man_field_scripts::walk_spawn_scores(&mf, &man)
        .into_iter()
        .find(|s| u16::from(s.global_index) == gate.flat_index)
    {
        host.world.replay_field_bgm_words(&score.words);
    }
    let world = &mut host.world;
    if world.cutscene_timeline_active() {
        world.field_vm.helper_contexts.push(
            legaia_engine_core::cutscene_timeline::CutsceneTimeline::new(
                man[start..start + len].to_vec(),
                pc0,
            ),
        );
        true
    } else {
        world.install_cutscene_timeline_record(&mf, &man, 2, idx, false)
    }
}

/// Engage the placement whose interaction record is the gate's: the talk
/// the retail player opened. Retail's capture holds a placement context with
/// the engaged bit up (`+0x10 & 0x100`, raised only by a touch); the engine
/// keeps the placement's idle body on a channel and runs a talk on its
/// inline runner, opened by the interaction probe's own dispatch
/// ([`legaia_engine_core::world::World::trigger_field_interact`]). Without
/// this the gate's record sits on its idle channel for the whole deadline
/// and the frame shows the room with no conversation in it.
fn engage_placement(world: &mut legaia_engine_core::world::World, gate: &ScriptGate) -> bool {
    if world.dialog.inline.is_some() || world.dialog.current.is_some() {
        return false;
    }
    let mut slots: Vec<u8> = world
        .npcs
        .dialog_prologue
        .iter()
        .filter(|(_, rec)| head_of(&rec.body) == gate.head)
        .map(|(slot, _)| *slot)
        .collect();
    slots.sort_unstable();
    let Some(&slot) = slots.first() else {
        return false;
    };
    if let Some(body) = world
        .npcs
        .dialog_prologue
        .iter()
        .find(|(s, _)| **s == slot)
        .map(|(_, rec)| rec.body.to_vec())
    {
        roll_back_run_latches(world, &body, gate.pc);
    }
    world.trigger_field_interact(0, slot);
    true
}

/// The system flags a record set on its way to `gate_pc`: every `0x5x` SET
/// in the straight-line run that ends on the gate PC, from the last
/// branching instruction before it (a jump, a picker, a system-flag test).
/// Retail executed that run to stand where it was captured, so those
/// latches are the record's own and are already in the state's flags. A
/// replay from the first opcode tests them and takes the other arm:
/// `town0c` P1[21] (Nene's "Bees!" beat before the Queen Bee ambush) sets
/// `0x5C1` at `+0x7A`, and with it already up the replay branched at
/// `+0x76` straight to the battle, past the shot `rim_elm_queen_bee_battle`
/// is captured on. `body` is in the gate's frame (the record's script
/// start at offset 0).
pub fn run_latches(body: &[u8], gate_pc: usize) -> Vec<u16> {
    use legaia_asset::field_disasm::{FlagKind, InsnInfo, decode};
    for start in 0..gate_pc.min(64) {
        let mut pc = start;
        let mut run: Vec<u16> = Vec::new();
        let mut ok = false;
        while pc < gate_pc {
            let Ok(i) = decode(body, pc) else { break };
            if i.size == 0 {
                break;
            }
            match i.info {
                InsnInfo::JmpRel { .. }
                | InsnInfo::CondJmp { .. }
                | InsnInfo::Picker { .. }
                | InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    ..
                } => run.clear(),
                InsnInfo::SystemFlag {
                    kind: FlagKind::Set,
                    idx,
                    ..
                } => run.push(idx),
                _ => {}
            }
            pc += i.size;
            ok = pc == gate_pc;
        }
        if ok {
            return run;
        }
    }
    Vec::new()
}

/// Clear [`run_latches`] before a record is replayed toward the gate.
fn roll_back_run_latches(
    world: &mut legaia_engine_core::world::World,
    body: &[u8],
    gate_pc: usize,
) {
    for idx in run_latches(body, gate_pc) {
        world.system_flag_clear(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_latches_are_the_sets_after_the_last_branch() {
        // 75 C1 05 00 (test 0x5C1 -> +5), 55 C2 (set 0x5C2, behind the
        // test), 26 02 00 (jump), 55 C1 (set 0x5C1), 4A 10 00 (wait), gate.
        let body = [
            0x75, 0xC1, 0x05, 0x00, 0x55, 0xC2, 0x26, 0x02, 0x00, 0x55, 0xC1, 0x4A, 0x10, 0x00,
        ];
        assert_eq!(run_latches(&body, 14), vec![0x5C1]);
        assert_eq!(run_latches(&body, 11), vec![0x5C1]);
        assert!(run_latches(&body, 9).is_empty());
    }

    #[test]
    fn a_script_gate_round_trips_through_its_env_form() {
        let g = ScriptGate {
            flat_index: 13,
            head: vec![0x0C, 0x83, 0x47, 0x00, 0xFF],
            pc: 1063,
            wait: 48,
            op: 0x4A,
            glide_left: None,
            retire_watch: std::cell::Cell::new((false, false)),
        };
        assert_eq!(ScriptGate::from_env(&g.to_env()), Some(g.clone()));
        let g = ScriptGate {
            glide_left: Some(277),
            ..g
        };
        assert_eq!(ScriptGate::from_env(&g.to_env()), Some(g));
        assert_eq!(ScriptGate::from_env("13:0c8:1:2:74"), None);
        assert_eq!(ScriptGate::from_env("13:0c83:1:2"), None);
    }

    /// A RAM image with one actor list: the scene system context (tick
    /// `FUN_801DA51C`) and one engaged spawned record, linked from the first
    /// list head. Only the record runs.
    #[test]
    fn engaged_contexts_are_read_off_the_actor_lists() {
        fn w32(ram: &mut [u8], va: u32, v: u32) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn w16(ram: &mut [u8], va: u32, v: u16) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
        let mut ram = vec![0u8; 0x20_0000];
        let (sys, rec) = (0x8010_0000u32, 0x8010_1000u32);
        let (sys_base, rec_base) = (0x8012_0000u32, 0x8012_1000u32);
        w32(&mut ram, ACTOR_LIST_HEADS, sys);
        w32(&mut ram, sys, rec);
        w32(&mut ram, sys + 0x0C, 0x801D_A51C);
        w32(&mut ram, sys + 0x10, ENGAGED);
        w16(&mut ram, sys + 0x50, 0xFB);
        w32(&mut ram, sys + 0x90, sys_base);
        w32(&mut ram, rec + 0x0C, FIELD_ACTOR_TICK);
        w32(&mut ram, rec + 0x10, ENGAGED);
        w16(&mut ram, rec + 0x50, 13);
        w16(&mut ram, rec + 0x54, 48);
        w32(&mut ram, rec + 0x90, rec_base);
        w16(&mut ram, rec + 0x9E, 7);
        ram[(rec_base & 0x1F_FFFF) as usize + 7] = 0x4A;
        ram[(sys_base & 0x1F_FFFF) as usize] = 0x24;
        let s = RetailScripts::from_ram(&ram);
        assert_eq!(s.running.len(), 1);
        let r = &s.running[0];
        assert_eq!((r.flat_index, r.pc, r.wait, r.op), (13, 7, 48, 0x4A));
        // A disengaged context is not running.
        w32(&mut ram, rec + 0x10, 0);
        assert!(RetailScripts::from_ram(&ram).running.is_empty());
    }
}
