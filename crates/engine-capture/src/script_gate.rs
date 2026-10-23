//! [`ScriptGate`]: a capture's field-script phase - the running record, the
//! PC it is parked on, how far into a wait it is - as a predicate the engine
//! is run to, with the drive that resumes the record and pages its dialog
//! boxes, and its environment hand-off.

/// Bytes of a record read to identify it.
pub const RECORD_HEAD_LEN: usize = 48;

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
/// as `RetailScript::head` (bytes from the record's script start).
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

/// Engine ticks a script-gated seed runs before giving up on the phase.
pub const SCRIPT_GATE_DEADLINE: u64 = 9000;

/// A retail capture's script phase: the running record (by its head bytes),
/// the PC it is parked on and how far into an op-`0x4A` wait it is. The seed
/// runs the engine until its own context for that record holds the same
/// phase, rather than for a fixed window - the field twin of the battle
/// half's [`crate::phase_gate::PhaseGate`].
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
    /// Where retail's scene system script is parked
    /// (`RetailScripts::system_pc`). The system SM runs no pass while a
    /// record holds the player, so this is the last pass it ran before the
    /// gate's record took over - and the pass after which the seed starts a
    /// record the engine is not running ([`Self::drive_resume`]).
    pub system_pc: Option<usize>,
    /// Set once [`Self::drive_resume`] has started the record ahead of
    /// [`SCRIPT_RESUME_TICK`].
    pub resumed_early: std::cell::Cell<bool>,
    /// A text gate caught while the pager still types its page: the typing
    /// row's window index (`_DAT_801F3530`) and the reveal counter
    /// (`_DAT_801F2748`) of pager state `0x0B`. `None` for a page that is up
    /// and waits - the shape [`Self::met`] assumes otherwise.
    pub typing: Option<(u8, i32)>,
    /// A gate parked on the player's clip end-latch spin (`AD F8 08`): the
    /// player's clip cursor (`+0x68`) at the capture. The spin holds the
    /// record on its PC for the whole clip, so the PC alone meets the gate
    /// on the clip's first frame.
    pub latch: Option<i16>,
    /// A gate parked on a cross-context op whose target placement is still
    /// walking a compass leg the record armed (`B7` / `C1 <id> b0 b1`): the
    /// target byte and the leg's spent units (`+0x54`) at the capture. The
    /// arm runs the record on past the walk and the dispatcher prologue
    /// holds the next op on that actor at its PC until the leg lands
    /// (`0x801DE90C..0x801DE944`), so the PC alone meets the gate on the
    /// leg's first frame.
    pub walk: Option<(u8, i16)>,
}

impl ScriptGate {
    /// `<flat index>:<head hex>:<pc>:<wait>:<op>[:<glide left>][:s<system pc>][:t<row>.<counter>][:l<cursor>]`
    /// - `LEGAIA_SCRIPT_GATE` for `play-window`.
    pub fn to_env(&self) -> String {
        let hex: String = self.head.iter().map(|b| format!("{b:02x}")).collect();
        let glide = self.glide_left.map(|g| format!(":{g}")).unwrap_or_default();
        let system = self.system_pc.map(|p| format!(":s{p}")).unwrap_or_default();
        let typing = self
            .typing
            .map(|(r, c)| format!(":t{r}.{c}"))
            .unwrap_or_default();
        let latch = self.latch.map(|c| format!(":l{c}")).unwrap_or_default();
        let walk = self
            .walk
            .map(|(t, a)| format!(":w{t}.{a}"))
            .unwrap_or_default();
        format!(
            "{}:{hex}:{}:{}:{}{glide}{system}{typing}{latch}{walk}",
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
        let mut glide_left = None;
        let mut system_pc = None;
        let mut typing = None;
        let mut latch = None;
        let mut walk = None;
        for tail in it {
            if let Some(w) = tail.strip_prefix('w') {
                let (t, a) = w.split_once('.')?;
                walk = Some((t.parse().ok()?, a.parse().ok()?));
            } else if let Some(c) = tail.strip_prefix('l') {
                latch = Some(c.parse().ok()?);
            } else if let Some(p) = tail.strip_prefix('s') {
                system_pc = Some(p.parse().ok()?);
            } else if let Some(t) = tail.strip_prefix('t') {
                let (r, c) = t.split_once('.')?;
                typing = Some((r.parse().ok()?, c.parse().ok()?));
            } else {
                glide_left = Some(tail.parse().ok()?);
            }
        }
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
            system_pc,
            resumed_early: std::cell::Cell::new(false),
            typing,
            latch,
            walk,
        })
    }

    /// The per-tick resume drive both seeds run: start the gate's record
    /// when no engine context runs it ([`resume_record`]), at
    /// [`SCRIPT_RESUME_TICK`] - or earlier, on the first tick the engine's
    /// system script stands on the PC retail's is parked on.
    ///
    /// The early leg is what keeps the entry script's history retail's. The
    /// system SM `FUN_801DA51C` runs no pass while a record holds the
    /// player, so a record that took the player straight after the install
    /// pass leaves the system context parked there for the whole cutscene:
    /// `town01`'s opening record holds it on `+0x9F` and `map01`'s on
    /// `+0x13F`, the PC after each install's last `0x21`, and neither scene's
    /// per-frame body has run once. A seed that waited out the settle window
    /// first ran that body with a free player at the seat, and it raised the
    /// region selector there (`0x19D` / `0x19E` in `town01`, `0x528` in
    /// `map01`) and picked the clear colour - bits and a backdrop the retail
    /// state does not hold. A park inside the per-frame loop
    /// ([`loops_back_to`]) keeps the settle-tick resume: retail's body has
    /// run there, and the engine first stands on that PC the tick before
    /// its own first pass. Returns `true` on the tick a context was
    /// installed.
    pub fn drive_resume(
        &self,
        host: &mut legaia_engine_scene_host::scene_host::SceneHost,
        tick: u64,
    ) -> bool {
        if self.resumed_early.get() {
            return false;
        }
        if tick == SCRIPT_RESUME_TICK {
            return resume_record(host, self);
        }
        // A placement the engine holds only on its idle channel is as much
        // "not running" as a record with no context: the resume engages it.
        // Retail's post-fight return does that from the install slice itself
        // (`town01` `P1[0]` tests `0x23C` at `+0x91` and runs `B1 2E 08`), so
        // `v0_1_post_battle_tetsu_town` holds the system context on the
        // install park with the talk up, and an engagement left to the
        // settle tick let the loop select `0x19D` first.
        let early = tick < SCRIPT_RESUME_TICK
            && self.system_pc == Some(host.world.field_pc)
            && !loops_back_to(&host.world.field_bytecode, host.world.field_pc)
            && self
                .context(&host.world)
                .is_none_or(|c| matches!(c.kind, EngineContextKind::Channel(_)));
        if early && resume_record(host, self) {
            self.resumed_early.set(true);
            return true;
        }
        false
    }

    /// A dialog box with its page typed and the prompt glyph on: a page
    /// waiting for the press that turns it, or the last page waiting for the
    /// press that closes it (`is_done` - the box stays up until that press,
    /// the same pair the window's prompt glyph keys on).
    fn page_up(d: &legaia_engine_dialog::dialog::OwnedDialogPanel) -> bool {
        d.is_waiting_for_input() || d.is_done()
    }

    /// The player's clip has run as far as the capture's, for a gate parked
    /// on its end-latch spin ([`Self::latch`]); `true` for any other gate.
    fn latch_reached(&self, world: &legaia_engine_core::world::World) -> bool {
        self.latch.is_none_or(|cursor| {
            world
                .player_script_clip_cursor()
                .is_some_and(|c| c >= cursor)
        }) && self.walk_reached(world)
    }

    /// The target's compass leg has spent as many units as the capture's,
    /// for a gate held behind a walking placement ([`Self::walk`]); `true`
    /// for any other gate, and once the engine's leg has landed.
    fn walk_reached(&self, world: &legaia_engine_core::world::World) -> bool {
        self.walk.is_none_or(|(target, accum)| {
            world
                .npc_compass_walk_accum(target)
                .is_none_or(|a| a as i16 >= accum)
        })
    }

    /// The text the gate's box shows: the page up and waiting, or - for a
    /// capture caught mid-page ([`Self::typing`]) - the pager typing the same
    /// window row with at least as many units revealed.
    fn text_shown(&self, d: &legaia_engine_dialog::dialog::OwnedDialogPanel) -> bool {
        let Some((row, counter)) = self.typing else {
            return Self::page_up(d);
        };
        Self::page_up(d)
            || d.window().is_some_and(|w| {
                w.phase.retail_state() == 0x0B
                    && w.rows.len() == usize::from(row) + 1
                    && w.pacer.counter >= counter
            })
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
        } && self.latch_reached(world);
        let passed = |tl: &legaia_engine_field::cutscene_timeline::CutsceneTimeline| {
            head_of(&tl.bytecode) == self.head
                && ((tl.pc == self.pc
                    && tl.ctx.wait_accum >= self.wait
                    && glide_landed
                    && (!text || tl.dialog.as_ref().is_some_and(|d| self.text_shown(d))))
                    || (tl.pc != self.pc && tl.visited.get(self.pc).copied().unwrap_or(false)))
        };
        if world.cutscene.timeline.as_ref().is_some_and(passed)
            || world.field_vm.helper_contexts.iter().any(passed)
        {
            return true;
        }
        // An engaged placement: the engine runs the talk on its inline
        // runner, which holds the PC on a segment's start while its box is
        // up - the same PC retail's `+0x9E` holds. A multi-page box keeps
        // the PC on its first segment and turns its pages inside the panel,
        // marking each new page's row leads visited as the page opens; a
        // gate on a later page is met once that page is on screen and
        // typed, not on the turn (`town01_tetsu_topic_prompt` is parked on
        // `+0x155`, the third page of the box `P1[10]` opens at `+0x4E`).
        if world.dialog.inline.as_ref().is_some_and(|id| {
            let on_page = id
                .panel
                .as_ref()
                .is_some_and(|p| p.row_leads().contains(&self.pc));
            id.prop_anchor.is_none()
                && !id.done
                && head_of(&id.bytecode) == self.head
                && (((id.pc == self.pc || on_page)
                    && id.ctx.wait_accum >= self.wait
                    && self.latch_reached(world)
                    && (!text || id.panel.as_ref().is_some_and(|d| self.text_shown(d))))
                    || (id.pc != self.pc
                        && !on_page
                        && id.visited.get(self.pc).copied().unwrap_or(false)))
        }) {
            return true;
        }
        engine_contexts(world).iter().any(|c| {
            matches!(c.kind, EngineContextKind::Channel(_))
                && c.head == self.head
                && !c.done
                && c.pc == self.pc
                && c.wait >= self.wait
                && self.latch_reached(world)
        })
    }

    /// The engine runs the gate's record on the timeline or a concurrent
    /// context whose PC reaches the gate PC in straight-line code: every
    /// instruction from it decodes, none is a jump or a text segment, and
    /// one ends exactly on the gate PC.
    fn straight_to_gate(&self, world: &legaia_engine_core::world::World) -> bool {
        use legaia_scene_formats::field_disasm::{InsnInfo, decode};
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
        // A topic menu on the way: the player picked the option whose arm
        // leads to the capture, so steer the cursor onto the arm with the
        // last branch target at or before the gate PC and confirm it there.
        // `Cross` alone took option 0 every time (`v0_1_tetsu_dialogue_accept`
        // looped on Tetsu's first topic, short of the spar arm it is captured
        // in).
        let inline = world
            .dialog
            .inline
            .as_ref()
            .filter(|id| !id.done && head_of(&id.bytecode) == self.head);
        // A press while the menu still slides in commits it at its opening
        // cursor once the pager reads the choice: hold off until it takes
        // input.
        if inline.is_some_and(|id| id.menu_active() && !id.picker_takes_input()) {
            return 0;
        }
        if let Some(id) = inline
            && id.picker_takes_input()
            && let Some(p) = id.picker()
            && let Some(want) = (0..p.n)
                .filter_map(|i| p.jump_target(i).map(|t| (i, t)))
                .filter(|&(_, t)| t <= self.pc)
                .max_by_key(|&(_, t)| t)
                .map(|(i, _)| i)
        {
            let cur = id.picker_cursor();
            let b = match cur.cmp(&want) {
                std::cmp::Ordering::Less => legaia_engine_system::input::PadButton::Down,
                std::cmp::Ordering::Greater => legaia_engine_system::input::PadButton::Up,
                std::cmp::Ordering::Equal => legaia_engine_system::input::PadButton::Cross,
            };
            return b.mask();
        }
        match self.context(world) {
            Some(c) if c.dialog_open && c.pc != self.pc => {
                legaia_engine_system::input::PadButton::Cross.mask()
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

/// Whether a script parked on `pc` can come back to it: some jump decoded in
/// straight-line order after `pc` targets `pc` or an earlier byte. A system
/// script's per-frame loop closes that way (`map01`'s `26 84 FF` at `+0x1BD`
/// back to the `0x21` at `+0x142`), while the park after its install pass
/// (`+0x13F`) has only forward jumps ahead of it.
pub fn loops_back_to(bytecode: &[u8], pc: usize) -> bool {
    use legaia_scene_formats::field_disasm::{InsnInfo, decode};
    let mut at = pc;
    while let Ok(i) = decode(bytecode, at) {
        if i.size == 0 {
            break;
        }
        if let InsnInfo::JmpRel { target, .. } = i.info
            && target <= pc
        {
            return true;
        }
        at += i.size;
    }
    false
}

/// Ticks after the seed at which a record the engine is not running is
/// resumed from its start ([`resume_record`]): the settle window, so a
/// record the scene's own entry starts (the credits' vignette driver) is
/// found running instead, and the settle-window sample stays the ungated one.
pub const SCRIPT_RESUME_TICK: u64 = crate::SETTLE_TICKS;

/// Start the gate's record from its first opcode when no engine context runs
/// it. A capture inside a partition-2 record the card-load resume does not
/// restart - its one-shot gate flag is already set in the save, or its
/// trigger is a walk-on tile the seat does not cross - otherwise never
/// reaches the phase. The record is resolved by the flat index and checked
/// against the capture's own head bytes, installed ungated (its gate flags
/// are the ones it has already latched) as the modal cutscene timeline, or
/// as a concurrent context when another timeline holds that slot. The
/// op-`0x35` words of the arm that spawns the record, when that arm scored it
/// ([`legaia_engine_field::man_field_scripts::walk_spawn_scores`]), are
/// replayed first. Returns `true` when a context was installed.
pub fn resume_record(
    host: &mut legaia_engine_scene_host::scene_host::SceneHost,
    gate: &ScriptGate,
) -> bool {
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
    let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
        return false;
    };
    let n0 = mf.header.partition_counts[0].max(0) as usize;
    let n1 = mf.header.partition_counts[1].max(0) as usize;
    let Some(idx) = usize::from(gate.flat_index).checked_sub(n0 + n1) else {
        return false;
    };
    let Some((start, pc0, len)) =
        legaia_engine_field::man_field_scripts::partition_record_span(&mf, &man, 2, idx)
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
    if let Some(score) = legaia_engine_field::man_field_scripts::walk_spawn_scores(&mf, &man)
        .into_iter()
        .find(|s| u16::from(s.global_index) == gate.flat_index)
    {
        host.world.replay_field_bgm_words(&score.words);
    }
    let world = &mut host.world;
    if world.cutscene_timeline_active() {
        world.field_vm.helper_contexts.push(
            legaia_engine_field::cutscene_timeline::CutsceneTimeline::new(
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
        // A talk the capture holds past a fight it staged resumes where that
        // fight's talk ended, not at the record's entry.
        if let Some(pc) = post_battle_resume(&body, gate.pc)
            && let Some(rec) = world.npcs.dialog_prologue.get_mut(&slot)
        {
            rec.entry_pc = pc;
        }
    }
    world.trigger_field_interact(0, slot);
    true
}

/// The PC a placement's talk resumes at when the capture is past a fight the
/// talk staged: the byte after the `0x21` that ends the talk on a scripted
/// battle (`3E FF <row>`, `21`), when the linear walk from it reaches the gate
/// PC with no other talk end on the way. Retail's talk and the placement are
/// one context (`actor[+0x9E]`): the talk ends on that `0x21` with the PC
/// past it, the fight runs, and the engagement after it resumes there.
/// `town01` `P1[10]` stages the sparring fight at `+0x7F7` and ends on the
/// `21` at `+0x7FA`; `v0_1_post_battle_tetsu_town` is captured on "You did
/// well." at `+0x804`, which a talk opened from the entry only reaches
/// through the fight. `None` for every other gate.
pub fn post_battle_resume(body: &[u8], gate_pc: usize) -> Option<usize> {
    use legaia_scene_formats::field_disasm::decode;
    for start in 0..gate_pc.min(64) {
        let mut pc = start;
        let mut prev_op = None;
        let mut resume = None;
        while pc < gate_pc {
            let Ok(i) = decode(body, pc) else { break };
            if i.size == 0 {
                break;
            }
            if i.opcode == 0x21 && i.extended.is_none() {
                resume = (prev_op == Some(0x3E)).then_some(pc + i.size);
            }
            prev_op = Some(i.opcode);
            pc += i.size;
        }
        if pc == gate_pc {
            return resume;
        }
    }
    None
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
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, decode};
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

/// The items a record gives (op `0x39`) on its way to `gate_pc`, in the same
/// straight-line run [`run_latches`] reads: retail executed those grants to
/// stand where it was captured, so the state's bag already holds them, and a
/// replay grants them a second time. `town01` `P1[10]`'s spar arm gives item
/// `119` at `+0x7A6`, ahead of the line `v0_1_tetsu_dialogue_accept` is
/// captured on.
pub fn run_grants(body: &[u8], gate_pc: usize) -> Vec<u8> {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, decode};
    for start in 0..gate_pc.min(64) {
        let mut pc = start;
        let mut run: Vec<u8> = Vec::new();
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
                InsnInfo::GiveItem { item_id } => run.push(item_id),
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

/// Clear [`run_latches`] and take back [`run_grants`] before a record is
/// replayed toward the gate.
fn roll_back_run_latches(
    world: &mut legaia_engine_core::world::World,
    body: &[u8],
    gate_pc: usize,
) {
    for idx in run_latches(body, gate_pc) {
        world.system_flag_clear(idx);
    }
    for id in run_grants(body, gate_pc) {
        let bag = &mut world.party.inventory;
        match bag.get(&id).copied() {
            Some(n) if n > 1 => {
                bag.insert(id, n - 1);
            }
            Some(_) => {
                bag.remove(&id);
            }
            None => {}
        }
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
    fn run_grants_are_the_gives_after_the_last_branch() {
        // 39 05 (give 5, behind the jump), 26 02 00 (jump), 39 77 (give
        // 119), 4A 10 00 (wait), gate.
        let body = [0x39, 0x05, 0x26, 0x02, 0x00, 0x39, 0x77, 0x4A, 0x10, 0x00];
        assert_eq!(run_grants(&body, 10), vec![0x77]);
        assert!(run_grants(&body, 5).is_empty());
    }

    /// A gate past a talk's `3E` / `21` fight boundary resumes after the
    /// `21`; a later `21` with no fight before it cancels that.
    #[test]
    fn a_post_battle_gate_resumes_past_the_fight_end() {
        // 4A 10 00 | 3E FF 04 | 21 | 52 0C | <gate>
        let body = [0x4A, 0x10, 0x00, 0x3E, 0xFF, 0x04, 0x21, 0x52, 0x0C, 0x24];
        assert_eq!(post_battle_resume(&body, 9), Some(7));
        // 4A 10 00 | 3E FF 04 | 21 | 21 | <gate>
        let body = [0x4A, 0x10, 0x00, 0x3E, 0xFF, 0x04, 0x21, 0x21, 0x24];
        assert_eq!(post_battle_resume(&body, 8), None);
        // No fight at all.
        assert_eq!(post_battle_resume(&[0x4A, 0x10, 0x00, 0x24], 3), None);
    }

    #[test]
    fn a_loop_park_is_told_from_an_install_park() {
        // 21 | 26 03 00 (-> +5) | 21 | 25 | 26 FD FF (-> +4)
        let body = [0x21, 0x26, 0x03, 0x00, 0x21, 0x25, 0x26, 0xFD, 0xFF];
        // Parked after the install `21`: every jump ahead lands past it.
        assert!(!loops_back_to(&body, 1));
        // Parked after the loop's `21`: the tail jump comes back.
        assert!(loops_back_to(&body, 5));
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
            system_pc: Some(159),
            resumed_early: std::cell::Cell::new(false),
            typing: None,
            latch: None,
            walk: None,
        };
        assert_eq!(ScriptGate::from_env(&g.to_env()), Some(g.clone()));
        let g = ScriptGate {
            glide_left: Some(277),
            typing: Some((1, 15)),
            latch: Some(-3),
            walk: Some((59, 8)),
            ..g
        };
        assert_eq!(ScriptGate::from_env(&g.to_env()), Some(g));
        assert_eq!(ScriptGate::from_env("13:0c8:1:2:74"), None);
        assert_eq!(ScriptGate::from_env("13:0c83:1:2"), None);
    }
}
