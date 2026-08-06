//! Host for the field overlay's **op-`0x49` submode screens**: the missing
//! caller that walks the submode driver actor and invokes its `+0x50` handler.
//!
//! [`legaia_engine_vm::baka_hub_actors`] carries the dispatcher `FUN_801F159C`,
//! its `PTR_FUN_801F33B4` state machines and the `PANEL_WINDOW_TABLE` painters.
//! None of them can run without three things this module supplies: an actor
//! with a `+0x50` handler id, the submode cursor context `DAT_801C6EA4` the
//! dispatcher polls, and a per-frame caller. The engine already had the first
//! ingredient and did not use it - [`World::man_load_actor_reset`] spawns an
//! [`ActorHandler::SubmodeDriver`] actor on every MAN load (retail's
//! `FUN_801D9C3C` at `0x8003B444`) and
//! [`crate::actor_handler::HandlerKernel`] classed it `Unported`, so the actor
//! sat in the pool doing nothing.
//!
//! ## Why this is load-bearing rather than decoration
//!
//! Field-VM opcode `0x49` (`STATE_RESUME`) is a **tristate park**: the script
//! arms `_DAT_8007B450` with its operand pointer and then re-enters the same PC
//! every frame until something writes `1` there. `docs/subsystems/script-vm.md`
//! names the writer: "the Done writer is field-overlay `FUN_801F159C`-class" -
//! that is [`hub_dispatch`]'s retire arm. The engine recognised exactly three
//! sub-ops (`0` inline shop, `3` name entry, `5` tile board) and had no Done
//! writer for the rest, so a script reaching any other sub-op re-armed and
//! halted on the same PC forever. Running the dispatcher supplies the writer.
//!
//! Slot `0` of `PTR_FUN_801F33B4` is `FUN_801F2134` (the close tick), and a
//! freshly spawned driver carries `+0x50 = 0`, so a sub-screen the engine
//! cannot draw yet still closes itself the retail way instead of parking.
//!
//! Two properties of that park are load-bearing and easy to lose:
//!
//! - **A driver per arm.** The dispatcher retires the driver when a screen
//!   hands back, and retail's Idle arm allocates a fresh one every time it
//!   arms - so [`World::open_field_submode_screen`] spawns one when none is
//!   live. Relying on the single MAN-load driver leaves every screen after
//!   the first with no dispatcher, hence Armed for the rest of the scene.
//! - **A park per context.** Retail's `_DAT_8007B450` is one global; the port
//!   steps several field-VM contexts inside one `World::tick` and resolves
//!   three sub-ops through host paths that bypass the global. The park is
//!   therefore tagged with its [`Op49ParkOwner`] and read only by that owner.
//!
//! REF: FUN_8002519C (the `jalr node[+0x0C]` walk this hangs off),
//! FUN_801D9C3C (the spawn), FUN_801E9B3C (the panel install this records
//! rather than performs)

use legaia_engine_vm::baka_hub_actors::{
    self as hub, ACTOR_RETIRE, CoinCounter, HubAction, HubActor, HubDraw, HubEnv, HubEquipEnv,
    HubEquipRecord, HubFrame, HubGrid, HubPainter, slot,
};
use legaia_engine_vm::world_map_overlay::{EquipProps, ItemProps};

use crate::actor_handler::ActorHandler;
use crate::equipment::{DiscEquipEntry, DiscEquipInfo, EquipSlot};
use crate::world::World;

/// Which field-VM context armed the op-`0x49` park.
///
/// Retail's park is a single global (`_DAT_8007B450`) and every context shares
/// it. The port cannot: it runs the per-tick field script
/// ([`World::step_field`]), the per-actor channels, and the spawned
/// partition-2 record contexts (the modal cutscene timeline and its concurrent
/// helpers) inside the *same* `World::tick`, and it resolves three sub-ops
/// ([`OP49_DEDICATED_SUB_OPS`]) through dedicated host paths that bypass the
/// global entirely. A park armed by one context and read by another therefore
/// answers a question that was never asked - which is exactly how the town01
/// opening's name-entry hand-off gets swallowed: the field script arms a
/// screen, and the modal timeline's own op-`0x49` reads that screen's Armed
/// and halts before `op49_invoke_setup` can open name entry.
///
/// Tagging the park with its owner keeps every context's Done its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Op49ParkOwner {
    /// The per-tick field script / interaction + inline-dialogue runners -
    /// everything stepped outside a spawned record slice.
    #[default]
    FieldScript,
    /// The modal cutscene timeline (`in_cutscene_timeline`).
    CutsceneTimeline,
    /// A concurrent helper context (a spawned partition-2 record that is not
    /// the modal timeline).
    HelperContext,
}

/// The live state of the one submode screen a field frame can have up.
///
/// Mirrors the retail globals rather than inventing a shape: [`Self::actor`]
/// is the driver actor's `+0x0A..+0x54` view, [`Self::cursor`] is
/// `DAT_801C6EA4`, [`Self::counter`] the coin counter's own cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmodeScreen {
    /// The driver actor's fields the dispatcher reads and writes.
    pub actor: HubActor,
    /// `DAT_801C6EA4` - the submode cursor context.
    pub cursor: HubGrid,
    /// The coin counter's digit cells, cursors and hold timer.
    pub counter: CoinCounter,
    /// Panel-window record index a **host** pins for this screen, independent
    /// of what the state machine installs. `None` is the retail shape - the
    /// descriptor is what names the window (see [`Self::installed_windows`]).
    pub window: Option<usize>,
    /// The panel-window records the last installed descriptor names, which is
    /// how retail decides which painter draws: a state machine calls
    /// `FUN_801E9B3C` with a descriptor, and the descriptor's entries carry
    /// the window indices ([`legaia_engine_vm::baka_hub_actors::panel_windows`]).
    ///
    /// The engine previously took the window from the *caller* at open time
    /// and never read the installs at all, so the coin counter drew the
    /// two-option panel where retail draws the three-line one, and the
    /// sub-menu drew nothing.
    pub installed_windows: Vec<usize>,
    /// A screen is up: the op-`0x49` park is Armed.
    pub open: bool,
    /// The dispatcher retired the actor - retail's `_DAT_8007B450 = 1`, which
    /// is the op-`0x49` Done signal.
    pub done: bool,
    /// Whatever the last tick drew and did, for a renderer to consume.
    pub frame: HubFrame,
    /// `FUN_801E9DC8`'s return for the confirm panel, supplied by the host
    /// that owns the two-option picker. `0` while nothing is picking.
    pub picker_result: i32,
    /// Pad edges seen since the dispatcher last ran.
    ///
    /// The actor pool advances on the **game-tick** clock - every second vsync
    /// under the field cadence floor of `2` - while the engine's hosts publish
    /// a pad word every vsync, so an edge that lands on a skipped tick is
    /// gone by the time the dispatcher looks. The parity is stable, so this is
    /// not a flake: with the pad set once per `World::tick`, *every* press
    /// released on the next tick fell in the gap, and no directional input
    /// ever reached a submode screen. A digit could not be entered on the pad
    /// at all.
    ///
    /// Retail does not have the gap because it samples the pad **once per game
    /// tick** (the master driver `FUN_80016444` runs one pass per
    /// `DAT_1F800393` vsyncs and the pad pump is inside it), so its edge word
    /// `_DAT_8007B874` is produced at exactly the rate the pool consumes it.
    /// The latch restores that relationship from the other side: it unions the
    /// per-vsync edges and hands the union to the pass that runs.
    pub pad_edge_latch: u32,
    /// The three bytes at `_DAT_8007B450 + 1..=3` - the op-`0x49` operand's
    /// payload, which is what the start menu counts to size its panel.
    ///
    /// Retail parks the operand *pointer* and the start menu dereferences it
    /// (`lw a0,-0x4bb0(v0)` then `lbu v0,0x1(a0)` .. `0x3(a0)` at
    /// `0x801F1168..0x801F11A4`); the port has no pointer, so the arm edge
    /// copies the three bytes off the instruction itself.
    pub board_entries: [u8; 3],
    /// The field-VM context that armed this park - see [`Op49ParkOwner`].
    /// Only that context's op-`0x49` reads [`Self::open`] / [`Self::done`].
    pub owner: Op49ParkOwner,
    /// The armed park's **kind byte**: retail's `*_DAT_8007B450`.
    ///
    /// Retail stores the op-`0x49` *operand pointer* in the park
    /// (`sw s6,-0x4bb0(s0)` at `0x801e09a8`) and every consumer
    /// dereferences its first byte, which is the sub-op the arm read one
    /// instruction earlier (`lbu v0,0x0(s6)` at `0x801e0984`). The port has
    /// no RAM pointer, so it keeps the byte itself - the whole of the
    /// pointer any consumer actually uses.
    ///
    /// `None` is retail's null park (nothing armed). It is set for **every**
    /// sub-op, including the three the port resolves through dedicated host
    /// paths ([`OP49_DEDICATED_SUB_OPS`]), because retail's single global is
    /// written before the port's paths diverge.
    ///
    /// Cleared on resume, which retail also does: the Done arm reads the
    /// sentinel and zeroes the slot before advancing.
    ///
    /// ```text
    /// 801e08c8  lw   v0,-0x4bb0(s0)     ; the park
    /// 801e08d0  bne  v0,s1,0x801e097c   ; s1 = 1, the Done sentinel
    /// 801e08d8  sw   zero,-0x4bb0(s0)   ; resume clears it
    /// ```
    ///
    /// REF: FUN_801DE840 (op `0x49` arm + Done arms)
    pub park_sub_op: Option<u8>,
    /// `_DAT_8007BB9C` - the selected menu-list row's **class nibble**, which
    /// the entry list's equipment sub-panel reads to decide where its
    /// comparison candidate comes from (see [`World::set_hub_equip_mode`]).
    ///
    /// Retail's own value is a menu-list global, not hub state: the list
    /// machinery publishes the highlighted row's class there
    /// ([`crate::menu_list_rows`]) and the sub-panel reads it. `0` is the
    /// no-candidate arm, which is what a hub screen opened without a list up
    /// sees.
    pub equip_mode: u32,
    /// The `+6` character mask and `+7` slot byte of the equipment table
    /// `DAT_80074F68`, per item id - the two bytes of that row the engine's
    /// world-resident tables do not carry (see
    /// [`World::install_hub_equip_restrictions`]). Empty until a host installs
    /// them, which also holds [`Self::equip_mode`] at the no-candidate arm.
    pub equip_restrictions: std::collections::BTreeMap<u8, (u8, u8)>,
}

impl SubmodeScreen {
    /// Is a screen up (the op-`0x49` park should read Armed)?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Did the last tick hand the frame back (the park should read Done)?
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Is a screen up *and* armed by `owner` (the park this context reads)?
    pub fn is_open_for(&self, owner: Op49ParkOwner) -> bool {
        self.open && self.owner == owner
    }

    /// Did a screen armed by `owner` hand the frame back?
    pub fn is_done_for(&self, owner: Op49ParkOwner) -> bool {
        self.done && self.owner == owner
    }

    /// Every text/sprite draw the last tick produced.
    pub fn draws(&self) -> &[HubDraw] {
        &self.frame.draws
    }
}

impl World {
    /// Open one op-`0x49` sub-screen on the submode driver actor.
    ///
    /// `handler` is a `PTR_FUN_801F33B4` index (see
    /// [`legaia_engine_vm::baka_hub_actors::slot`]) and `window` the
    /// panel-window record whose painter draws it, if one is known.
    ///
    /// Retail's enter path also seeds the cursor context's completion gate to
    /// `1` (`FUN_801F1278` writes `_DAT_801C6EA4+0x3E = 1`); without it the
    /// dispatcher would retire the actor on its very first frame.
    ///
    /// It also **spawns the driver actor**, and that is not optional. The
    /// op-`0x49` Idle arm calls the allocator unconditionally on every arm,
    /// before it stores the operand pointer into the park:
    ///
    /// ```text
    /// 801e0984  lbu   v0,0x0(s6)        ; sub_op
    /// 801e098c  sltiu v0,v0,0xe         ; sub_op < 0xE ?
    /// 801e0990  beq   v0,zero,0x801e3624
    /// 801e09a0  jal   0x80020de0        ; spawn the driver - EVERY arm
    /// 801e09a4  _addiu a0,a0,0x65c      ;   from descriptor 0x8007065C
    /// 801e09a8  sw    s6,-0x4bb0(s0)    ; _DAT_8007B450 = operand ptr
    /// ```
    ///
    /// (`overlay_0897_801de840.txt`, `FUN_801DE840` case `0x49`.) The engine
    /// had only the one driver [`World::man_load_actor_reset`] spawns per MAN
    /// load, and [`World::tick_submode_screen`] *retires* that driver when a
    /// screen hands back - so the second screen of a scene had no dispatcher,
    /// never handed back, and left the park Armed for the rest of the scene.
    ///
    /// REF: FUN_80020DE0 (the allocator), FUN_801D9C3C (the MAN-load spawn)
    pub fn open_field_submode_screen(&mut self, handler: u16, window: Option<usize>) {
        let owner = self.op49_park_owner();
        if self
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_none()
            && let Some(slot) = self.spawn_handler_actor(ActorHandler::SubmodeDriver)
        {
            // Retail clears the fresh driver's `+0x50` / `+0x54`; the handler
            // slot this screen wants goes on the screen's own actor view.
            self.actors[slot].state_54 = 0;
        }
        let s = &mut self.submode_screen;
        s.owner = owner;
        s.actor = HubActor {
            width: SUBMODE_PANEL_WIDTH,
            state: handler,
            ..HubActor::default()
        };
        s.cursor = HubGrid {
            done_gate: 1,
            ..HubGrid::default()
        };
        s.counter = CoinCounter::default();
        s.window = window;
        s.installed_windows.clear();
        s.pad_edge_latch = 0;
        s.open = true;
        s.done = false;
        s.picker_result = 0;
        s.frame = HubFrame::default();
    }

    /// Open the casino **coin counter** - buy coins with party gold at
    /// [`legaia_engine_vm::baka_hub_actors::GOLD_PER_COIN`] each.
    ///
    /// Handler slot `0x25`. No window is pinned: the counter's own arms
    /// install `PANEL_COIN_IDLE` and `PANEL_COIN_CONFIRM`, and those
    /// descriptors name the records that draw it - record `10` for the idle
    /// screen and the **three-line** panel `FUN_801F1890` for the confirm.
    pub fn open_coin_counter(&mut self) {
        self.open_field_submode_screen(slot::COIN_COUNTER, None);
    }

    /// Which field-VM context is stepping right now - the owner an op-`0x49`
    /// park armed on this step belongs to, and the only one that may read it.
    ///
    /// [`World::in_spawned_record_slice`] is set by `run_spawned_record_slice`
    /// for both spawned-record shapes, and [`World::in_cutscene_timeline`]
    /// only for the modal one, so the pair separates all three contexts.
    pub fn op49_park_owner(&self) -> Op49ParkOwner {
        if !self.in_spawned_record_slice {
            Op49ParkOwner::FieldScript
        } else if self.in_cutscene_timeline {
            Op49ParkOwner::CutsceneTimeline
        } else {
            Op49ParkOwner::HelperContext
        }
    }

    /// Record an op-`0x49` arm's **kind byte** - retail's
    /// `_DAT_8007B450 = operand`, reduced to the one byte every consumer
    /// dereferences.
    ///
    /// Called from the field VM's arm edge for every sub-op, so the three
    /// the port resolves through dedicated host paths are recorded too;
    /// retail's store happens before any of those paths would diverge.
    ///
    /// The owner tag is the same one [`World::open_field_submode_screen`]
    /// applies, for the same reason: retail has one global and the port
    /// steps several field-VM contexts inside one `World::tick`, so a park
    /// armed by one context must not answer another context's question.
    ///
    /// PORT: FUN_801DE840 (`0x801e0984` / `0x801e09a8`)
    pub fn record_op49_park(&mut self, sub_op: u8) {
        self.submode_screen.owner = self.op49_park_owner();
        self.submode_screen.park_sub_op = Some(sub_op);
    }

    /// Record the op-`0x49` operand's payload bytes for the screen this arm
    /// opens - retail's `_DAT_8007B450 + 1..=3`, read through the parked
    /// pointer by [`legaia_engine_vm::baka_hub_actors::start_menu`].
    ///
    /// `instr` is the instruction from its opcode byte, so `instr[1]` is the
    /// sub-op (the byte the park points at) and `instr[2..=4]` are the three
    /// the menu counts.
    pub fn set_submode_board_entries(&mut self, instr: &[u8]) {
        let mut out = [0u8; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = instr.get(2 + i).copied().unwrap_or(0);
        }
        self.submode_screen.board_entries = out;
    }

    /// Clear the park on resume - retail's `sw zero,-0x4bb0(s0)` at
    /// `0x801e08d8`, which runs on the Done edge before the PC advances.
    ///
    /// Only the context that armed the park may clear it, for the same
    /// reason it is the only one that may read it.
    pub fn clear_op49_park(&mut self) {
        if self.submode_screen.owner == self.op49_park_owner() {
            self.submode_screen.park_sub_op = None;
        }
    }

    /// Close whatever screen is up without running its hand-back.
    pub fn close_field_submode_screen(&mut self) {
        self.submode_screen.open = false;
        self.submode_screen.window = None;
        self.submode_screen.frame = HubFrame::default();
    }

    /// Run one dispatcher frame over the submode driver actor.
    ///
    /// This is the `jalr node[+0x0C]` arm of `FUN_8002519C` for
    /// [`ActorHandler::SubmodeDriver`], so it is called from
    /// [`World::tick_handler_actors`] on the same cadence as the colour tween.
    ///
    /// Returns `true` when the dispatcher retired the actor this frame, which
    /// is retail's `_DAT_8007B450 = 1` and unparks the field VM's op `0x49`.
    pub fn tick_submode_screen(&mut self, frame_delta: u8) -> bool {
        if !self.submode_screen.open {
            return false;
        }
        // Retail runs this off an actor on the pool; with no live driver there
        // is nothing to dispatch, exactly as before the spawn.
        if self
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_none()
        {
            return false;
        }
        let env = self.submode_env(frame_delta);
        // The latch is one-shot: a pass that ran has consumed every edge that
        // reached it, exactly as retail's per-game-tick pad sample is.
        self.submode_screen.pad_edge_latch = 0;
        let mut screen = std::mem::take(&mut self.submode_screen);
        let window = screen.window;
        let mut installed = std::mem::take(&mut screen.installed_windows);
        let SubmodeScreen {
            actor,
            cursor,
            counter,
            ..
        } = &mut screen;

        let frame = hub::hub_dispatch(actor, &env, cursor, |a, g| {
            let mut f = run_slot(a, &env, g, counter);
            // An install replaces whatever panel was up, and takes effect on
            // the frame that issued it: retail's window walk runs after the
            // handler in the same frame.
            if let Some(desc) = f.actions.iter().rev().find_map(|act| match act {
                HubAction::InstallPanel(va) => Some(*va),
                _ => None,
            }) {
                installed = hub::panel_windows(desc).to_vec();
            }
            // The host-pinned window (if any) draws alongside the installed
            // one; retail has no such override, so it is additive rather than
            // a replacement.
            // A descriptor may name the same record twice (the coin idle
            // program does); the window system installs it once.
            let mut to_paint: Vec<usize> = Vec::new();
            for idx in installed.iter().copied().chain(window) {
                if !to_paint.contains(&idx) {
                    to_paint.push(idx);
                }
            }
            for idx in to_paint {
                if let Some(p) = HubPainter::for_window(idx) {
                    let painted = p.paint(a, &env, g);
                    f.draws.extend(painted.draws);
                    f.actions.extend(painted.actions);
                }
            }
            f
        });
        screen.installed_windows = installed;

        let retired = screen.actor.flags & ACTOR_RETIRE != 0;
        screen.frame = frame;
        self.submode_screen = screen;
        self.apply_submode_actions();
        if retired {
            self.submode_screen.open = false;
            self.submode_screen.done = true;
            // Retail retires the node; the engine drops the pool slot the same
            // way every other kill-bit actor goes.
            if let Some(idx) = self.find_actor_by_handler(ActorHandler::SubmodeDriver) {
                self.actors[idx].physics.status_flags |=
                    crate::field_actor_kernels::ACTOR_FLAG_YIELD;
            }
        }
        retired
    }

    /// Install the two equipment-table bytes the sub-panel's **candidate** arm
    /// needs and the world's own tables do not carry: the `+6` character mask
    /// and the `+7` slot byte of `DAT_80074F68`.
    ///
    /// Everything else the panel reads is already world state - the character
    /// records ([`World::roster`]), the item property table's `+0` / `+1` bytes
    /// ([`World::item_effects`]) and the five stat bonuses
    /// ([`World::equipment_table`]). These two bytes are dropped by the
    /// modifier-only view a boot installs on the world, and survive only on the
    /// [`DiscEquipInfo`] a host builds for its menu runtime - hence the
    /// hand-in. Until one arrives the panel stays on retail's no-candidate arm
    /// rather than answering "can this character equip that" from a zero mask,
    /// which would paint the reject line over every entry.
    pub fn install_hub_equip_restrictions(&mut self, info: &DiscEquipInfo) {
        self.submode_screen.equip_restrictions = (0..=u8::MAX)
            .filter_map(|id| info.entry(id).map(|e| (id, (e.mask, disc_slot_bits(e)))))
            .collect();
    }

    /// Set the menu class word `_DAT_8007BB9C` the sub-panel's candidate ladder
    /// reads (`0x1000` / `0x6000` / `0x9000` bag slot, `0x3000` the cursor as
    /// an item id, `0x4000` compare against an empty loadout, anything else no
    /// candidate at all).
    ///
    /// Honored only once [`World::install_hub_equip_restrictions`] has run -
    /// see that method for why.
    pub fn set_hub_equip_mode(&mut self, mode: u32) {
        self.submode_screen.equip_mode = mode;
    }

    /// Project the world's equipment state onto the globals `FUN_801E5B4C`
    /// reads: the per-entry character records plus the two static resolver
    /// tables it walks them through.
    ///
    /// Retail addresses all of this directly - the save block at `0x80084140`,
    /// the item property table `DAT_80074368` and the equipment table
    /// `DAT_80074F68`. Each has a world-resident counterpart:
    ///
    /// | Retail read | World source |
    /// |---|---|
    /// | `char[+0x75E..]`, the five equip slots | [`World::roster`] |
    /// | `char[+0x6DA/+0x6DC/+0x6DE]`, ATK / UDF / LDF | the same record's `+0x112` / `+0x114` / `+0x116` |
    /// | `DAT_80074368[id*0xC + 0/+1]`, kind + stat index | [`World::item_effects`] |
    /// | `DAT_80074F68[row][+0..+4]`, the five bonuses | [`World::equipment_table`], re-keyed row-wise |
    /// | `0x80084140 + 0x1818`, the bag id list | [`World::inventory`] |
    /// | `0x8007B42C`, the weapon-slot table | [`RETAIL_WEAPON_SLOTS`] |
    ///
    /// The bonus rows arrive **keyed by item id** (that is the shape a battle
    /// aggregator wants) and the panel indexes them by the stat-table row an
    /// item's `+1` byte names, so they are re-keyed here through the same `+1`
    /// byte. Rows no equippable id reaches stay zero, which is what the disc
    /// holds for the one such row the panel can actually reach: item id `0`
    /// (the empty-slot sentinel) names row `0x6A`, and that row is eight zero
    /// bytes - so an empty slot contributes nothing without the port
    /// special-casing id `0`, exactly as retail does not.
    fn submode_equip_env(&self) -> HubEquipEnv {
        let mut env = HubEquipEnv {
            weapon_slots: RETAIL_WEAPON_SLOTS.to_vec(),
            ..HubEquipEnv::default()
        };
        // One record per distinct entry code. The code is a character index
        // (retail multiplies it by `0x414` against the save block), and the
        // engine's roster is that same index space.
        for &code in &self.active_party {
            if env.records.iter().any(|r| r.code == code) {
                continue;
            }
            let Some(rec) = self.roster.members.get(usize::from(code)) else {
                continue;
            };
            let live = rec.live_stats();
            env.records.push(HubEquipRecord {
                code,
                slots: hub_panel_slots(&rec.equipment().slots, usize::from(code)),
                base_stats: [live.atk as i32, live.udf as i32, live.ldf as i32],
            });
        }

        // The bag list the three inventory modes index with the shared cursor.
        // Retail's is the item window's own slot order; the engine's bag is a
        // map, so this is the id order `World::save_party` also writes.
        let mut bag: Vec<u8> = self
            .inventory
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(&id, _)| id)
            .collect();
        bag.sort_unstable();
        env.inventory = bag;

        let Some(items) = self.item_effects.as_ref() else {
            // No item property table: the aggregation has nothing to resolve
            // an equipped id through, so every row prints its base stat. The
            // candidate ladder stays off with it.
            return env;
        };
        env.item_props = (0..=u8::MAX)
            .map(|id| ItemProps {
                kind: items.kind(id),
                stat_index: items.subtype(id),
            })
            .collect();
        for id in 0..=u8::MAX {
            if items.kind(id) != legaia_asset::equip_stats::KIND_EQUIPMENT {
                continue;
            }
            let Some(m) = self.equipment_table.get(id) else {
                continue;
            };
            let row = usize::from(items.subtype(id));
            if env.equip_props.len() <= row {
                env.equip_props.resize(row + 1, EquipProps::default());
            }
            let props = &mut env.equip_props[row];
            // Record order is `[INT, ATK, UDF, LDF, SPD]` - the `+0` byte is
            // the INT bonus and lands last in retail's accumulator order, which
            // is why the modifier view names its fields rather than indexing.
            props.bonuses = [
                m.int as u8,
                m.atk as u8,
                m.udf as u8,
                m.ldf as u8,
                m.spd as u8,
            ];
            // Several ids can name the same row; both halves of what lands here
            // are that row's own bytes, so which id wrote it does not matter.
            if let Some(&(mask, slot_bits)) = self.submode_screen.equip_restrictions.get(&id) {
                props.char_mask = mask;
                props.slot_bits = slot_bits;
            }
        }
        if !self.submode_screen.equip_restrictions.is_empty() {
            env.mode = self.submode_screen.equip_mode;
        }
        env
    }

    /// Project the world's own state onto the globals the family reads.
    /// Union this vsync's pad edges into the submode latch.
    ///
    /// Called once per `World::tick`, which is once per vsync; the dispatcher
    /// that reads it runs once per game tick. See
    /// [`SubmodeScreen::pad_edge_latch`] for why the two rates have to be
    /// bridged rather than sampled independently.
    pub fn latch_submode_pad_edge(&mut self) {
        // Only while a screen is up: a latch that accumulated across a whole
        // scene would hand the next screen a press from minutes ago.
        if !self.submode_screen.open {
            self.submode_screen.pad_edge_latch = 0;
            return;
        }
        let pad = self.input.pad() as u32;
        let prev = self.input.pad_prev() as u32;
        self.submode_screen.pad_edge_latch |= pad & !prev;
    }

    fn submode_env(&self, frame_delta: u8) -> HubEnv {
        let pad = self.input.pad() as u32;
        let prev = self.input.pad_prev() as u32;
        let edge = (pad & !prev) | self.submode_screen.pad_edge_latch;
        HubEnv {
            // `DAT_801F2734` is the submode context's state word, which
            // `open_submode` seeds and `World::submode_context` mirrors.
            submode: self.submode_context.first().copied().unwrap_or(0) as i32,
            pad_edge: edge,
            pad_held: pad,
            pad_repeat: edge,
            confirm_mask: SUBMODE_ACCEPT_MASK | SUBMODE_BACK_MASK,
            cancel_mask: SUBMODE_BACK_MASK,
            accept_mask: SUBMODE_ACCEPT_MASK,
            back_mask: SUBMODE_BACK_MASK,
            frame_delta: frame_delta.max(1) as i32,
            picker_result: self.submode_screen.picker_result,
            cursor_row: self.submode_screen.counter.cursor,
            cursor_row_alt: self.submode_screen.counter.yes_no,
            // Retail reads the op-0x49 operand pointer here; the engine has no
            // RAM pointer, so it carries the "a screen is armed" truth value
            // the dispatcher's release arm actually branches on.
            board_flag: i32::from(self.submode_screen.open),
            board_entries: self.submode_screen.board_entries,
            // `DAT_80084594` / `DAT_80084598..` are the present-party roster;
            // the engine's mirror of retail's `0x8007BD10` list is
            // `World::active_party`.
            entry_count: self.active_party.len().min(u8::MAX as usize) as u8,
            entry_codes: self.active_party.clone(),
            gold: self.money,
            coin_bank: self.casino_coins.min(i32::MAX as u32) as i32,
            // Everything the entry list's per-entry sub-draw `FUN_801E5B4C`
            // reads out of RAM - see [`World::submode_equip_env`].
            equip: self.submode_equip_env(),
            ..HubEnv::default()
        }
    }

    /// Apply the side effects the last tick reported.
    fn apply_submode_actions(&mut self) {
        let actions = self.submode_screen.frame.actions.clone();
        for a in actions {
            match a {
                // The one action that moves persistent state: coins into the
                // casino bank `DAT_800845A4`, gold out of `DAT_8008459C`.
                HubAction::BuyCoins { coins, gold_cost } => {
                    if coins <= 0 {
                        continue;
                    }
                    self.casino_coins = self
                        .casino_coins
                        .saturating_add(coins as u32)
                        .min(hub::COIN_BANK_MAX as u32);
                    self.money = self.money.saturating_sub(gold_cost).max(0);
                }
                HubAction::ClearCursorRow => self.submode_screen.counter.cursor = 0,
                _ => {}
            }
        }
    }
}

/// Dispatch one `PTR_FUN_801F33B4` slot.
///
/// The slots without a ported body fall through to nothing, which is retail's
/// own behaviour for an index whose handler is a stub - the dispatcher still
/// runs its release arm afterwards, so the screen cannot wedge.
fn run_slot(
    actor: &mut HubActor,
    env: &HubEnv,
    cursor: &mut HubGrid,
    counter: &mut CoinCounter,
) -> HubFrame {
    match actor.state {
        slot::COIN_COUNTER => hub::coin_exchange(actor, env, counter, cursor),
        slot::START_MENU => hub::start_menu(actor, env, cursor),
        slot::PROMPT => hub::hub_prompt(actor, env, cursor),
        slot::SUBMENU => hub::submenu(actor, env, cursor),
        slot::DEACTIVATE => hub::deactivate(actor, env, cursor),
        slot::DRAW_TICK => hub::draw_tick(actor, env, cursor),
        slot::CLOSE_TICK | 0x14..=0x18 => hub::close_tick(actor, env, cursor),
        _ => HubFrame::default(),
    }
}

/// Retail's per-character **weapon slot** table `DAT_8007B42C` (`SCUS_942.54`
/// file `0x6BC2C`, halfword per character), which `resolve_equip_slot` indexes
/// for a candidate whose `+7` byte says weapon.
///
/// The disc holds `2, 3, 2`: Vahn and Gala carry their weapon in equip byte
/// `2`, Noa in byte `3`. The halfword after them is `0`, and an index past the
/// pinned three resolves to `0` as well, so the two agree for every character.
/// Only entry codes below `3` draw a panel at all.
pub const RETAIL_WEAPON_SLOTS: [i16; 3] = [2, 3, 2];

/// The three fixed slots of retail's `+0x196` array, as
/// `(retail index, the engine slot holding that item)`: byte `0` is **body
/// armour**, `1` the **head** slot, `4` **footwear**. Bytes `2` / `3` are the
/// weapon, per [`RETAIL_WEAPON_SLOTS`].
///
/// That is retail's own order, and it is not the engine's. Two disc tables pin
/// it independently: the panel's own slot resolution
/// (`(+7 & 0x60) >> 5` -> `0`, `1`, the weapon table, `4`) and the equip
/// screen's row map `DAT_801E43E8` = `00 01 00 04 05 06 07`, whose seven rows
/// are weapon (overridden by the per-character halfword), helmet `1`, body
/// armour `0`, footwear `4` and three Goods slots `5..7`. The engine's own
/// array is weapon-first with a hand-guard slot retail has no row for
/// ([`crate::equip_session::ARMAMENT_ENGINE_SLOTS`]), so a record has to be
/// re-ordered before the ported kernel walks it - [`hub_panel_slots`].
pub const HUB_PANEL_FIXED_SLOTS: [(usize, EquipSlot); 3] = [
    (0, EquipSlot::BodyArmor),
    (1, EquipSlot::Helmet),
    (4, EquipSlot::Boot),
];

/// Re-order a character's engine equip array into the five slots the sub-panel
/// walks, in retail's own `+0x196` order.
///
/// The aggregation itself is order-blind (it sums all five), so this matters
/// for one thing: the **trial-equip destination**. `resolve_equip_slot` answers
/// in retail's index space, so handing it an engine-ordered array would displace
/// the wrong item - a body-armour candidate would replace the weapon.
///
/// The engine's hand-guard slot has no retail counterpart and is dropped, which
/// is retail's own five-slot sum; retail's three Goods slots (`5..7`) are
/// outside the walk for both.
pub fn hub_panel_slots(equip: &[u8; 8], char_index: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    for (retail, engine) in HUB_PANEL_FIXED_SLOTS {
        out[retail] = equip[engine.as_index() as usize];
    }
    let weapon = RETAIL_WEAPON_SLOTS
        .get(char_index)
        .copied()
        .unwrap_or(0)
        .clamp(0, 4) as usize;
    out[weapon] = equip[EquipSlot::Weapon.as_index() as usize];
    out
}

/// Rebuild an equipment record's `+7` **slot byte** from the disc restriction
/// view: the four `& 0x60` categories plus the `0x01` Ra-Seru bit.
///
/// The panel reads `+7 & 0x60` only; the Ra-Seru bit rides along because it is
/// the same byte and the view carries it.
pub fn disc_slot_bits(entry: DiscEquipEntry) -> u8 {
    use legaia_asset::equip_stats::EquipSlot as Disc;
    let category = match entry.category {
        Disc::Body => 0x00,
        Disc::Head => 0x20,
        Disc::Weapon => 0x40,
        Disc::Footwear => 0x60,
    };
    category | u8::from(entry.is_ra_seru)
}

/// Panel width the driver actor carries (`+0x0E`), the anchor the right-edge
/// cursor of the single-label painters is measured from.
pub const SUBMODE_PANEL_WIDTH: i16 = 0x60;

/// Accept edge in the packed Legaia pad layout (Cross), standing in for
/// `_DAT_800846D0`.
pub const SUBMODE_ACCEPT_MASK: u32 = crate::dev_menu::PACK_CROSS as u32;
/// Back-out edge (Circle), standing in for `_DAT_800846D4`.
pub const SUBMODE_BACK_MASK: u32 = crate::dev_menu::PACK_CIRCLE as u32;

/// Panel-window record whose painter draws the Yes/No panel
/// (`FUN_801F1950`).
///
/// Not the coin counter's: that screen installs
/// [`legaia_engine_vm::baka_hub_actors::PANEL_COIN_CONFIRM`], which names the
/// three-line record. Kept because a host that wants the Yes/No panel by
/// itself has no descriptor to install.
pub const COIN_PANEL_WINDOW: usize = hub::window::TWO_OPTION;

/// Op-`0x49` sub-ops the world handles through a dedicated path rather than
/// through a submode screen: `0` inline gold shop, `3` name entry, `5` tile
/// board.
///
/// The retail table agrees about all three: sub-`0` selects no handler at all
/// (`-1`), sub-`3` selects `FUN_801F03F0` (name entry) and sub-`5` selects
/// `FUN_801EF2B0` (the tile-board walk). The engine reaches the latter two
/// through its own host paths, so it keeps them out of the dispatcher.
pub const OP49_DEDICATED_SUB_OPS: [u8; 3] = [0, 3, 5];

/// The handler slot an op-`0x49` sub-op opens.
///
/// This is retail's own selection: the 14-byte table
/// [`legaia_engine_vm::baka_hub_actors::OP49_SUBOP_SLOTS`] at `0x801F33A4`,
/// which the submode enter half indexes with the parked operand's first byte.
/// The engine previously had no mapping and opened the close tick for every
/// sub-op, so every screen but the three dedicated ones closed itself
/// immediately instead of running - the script unparked, but nothing drew.
pub fn slot_for_op49_sub_op(sub_op: u8) -> Option<u16> {
    if OP49_DEDICATED_SUB_OPS.contains(&sub_op) {
        return None;
    }
    // A sub-op the table gives no handler for still needs the park cleared,
    // and slot `0` is what a freshly spawned driver carries - retail's own
    // fallback for a `-1` row, which leaves `+0x50` at the spawn value.
    Some(hub::slot_for_sub_op(sub_op).unwrap_or(slot::CLOSE_TICK))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_driver() -> World {
        let mut w = World::new();
        w.man_load_actor_reset();
        assert!(
            w.find_actor_by_handler(ActorHandler::SubmodeDriver)
                .is_some(),
            "the MAN-load reset spawns the driver actor"
        );
        w
    }

    #[test]
    fn a_screen_only_ticks_while_the_driver_actor_is_alive() {
        let mut w = world_with_driver();
        w.open_coin_counter();
        assert!(!w.tick_submode_screen(1));
        assert!(!w.submode_screen.frame.actions.is_empty());

        // Retire the driver: the dispatcher has nothing to run.
        w.retire_actors_by_handler(ActorHandler::SubmodeDriver);
        w.retire_yielded_actors();
        w.submode_screen.frame = HubFrame::default();
        assert!(!w.tick_submode_screen(1));
        assert!(w.submode_screen.frame.actions.is_empty());
    }

    #[test]
    fn the_coin_counter_moves_coins_and_gold_through_a_world_tick() {
        let mut w = world_with_driver();
        w.money = 5_000;
        w.casino_coins = 7;
        w.open_coin_counter();

        // Frame 1: state 0 seeds the screen.
        w.tick_submode_screen(1);
        // Type 12 coins.
        w.submode_screen.counter.set_entered(12);
        // Frame 2: accept.
        w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
        w.tick_submode_screen(1);
        assert_eq!(w.submode_screen.actor.sub, 2, "the confirm panel is up");
        // Frame 3: pick Yes on the confirm panel.
        w.input.set_pad(0);
        w.submode_screen.counter.yes_no = 0;
        w.submode_screen.picker_result = hub::PICK_ACCEPT;
        w.tick_submode_screen(1);
        // Frame 4: the commit.
        w.submode_screen.picker_result = 0;
        w.tick_submode_screen(1);

        assert_eq!(w.casino_coins, 7 + 12, "coins land in the casino bank");
        assert_eq!(w.money, 5_000 - 1_200, "gold pays 100 per coin");
    }

    #[test]
    fn the_close_tick_default_unparks_instead_of_hanging() {
        let mut w = world_with_driver();
        // Slot 0 is what a fresh driver carries.
        w.open_field_submode_screen(slot::CLOSE_TICK, None);
        assert!(w.submode_screen.is_open());
        let mut retired = false;
        for _ in 0..8 {
            if w.tick_submode_screen(1) {
                retired = true;
                break;
            }
        }
        assert!(retired, "the close tick clears the gate and retires");
        assert!(w.submode_screen.is_done());
        assert!(!w.submode_screen.is_open());
    }

    #[test]
    fn a_painted_screen_emits_draws() {
        let mut w = world_with_driver();
        w.money = 100_000;
        w.open_coin_counter();
        // The entry arm installs the counter's idle panel, whose record has
        // no painter here; the confirm installs the three-line one, which
        // does. So the draw appears when the descriptor names it, not when
        // the screen opens.
        w.tick_submode_screen(1);
        assert_eq!(w.submode_screen.installed_windows, vec![0, 10, 10]);
        w.submode_screen.counter.set_entered(2);
        w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
        w.tick_submode_screen(1);
        assert_eq!(
            w.submode_screen.installed_windows,
            vec![hub::window::THREE_LINE]
        );
        assert!(
            !w.submode_screen.draws().is_empty(),
            "the installed panel's painter runs alongside the state machine"
        );
    }

    #[test]
    fn dedicated_sub_ops_keep_their_own_paths() {
        for s in OP49_DEDICATED_SUB_OPS {
            assert_eq!(slot_for_op49_sub_op(s), None);
        }
        // Everything else takes the handler retail's `0x801F33A4` table names.
        assert_eq!(slot_for_op49_sub_op(9), Some(slot::PROMPT));
        assert_eq!(slot_for_op49_sub_op(6), Some(slot::COIN_COUNTER));
        // A `-1` row leaves `+0x50` at the spawn value, which is slot `0`.
        assert_eq!(slot_for_op49_sub_op(7), Some(slot::CLOSE_TICK));
    }
}
