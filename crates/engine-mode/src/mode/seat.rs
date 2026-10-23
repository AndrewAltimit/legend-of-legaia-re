//! `ModeSeat`: the mode-word stores each init hands off with, the boot chain, and the seat that sequences a mode change.

use super::*;

/// One static store into the master mode word `_DAT_8007B83C`.
///
/// Every mode transition retail performs is one of these: an `sh` of a
/// literal into `0x8007B83C`, reached as `lui 0x8007 + 0xB83C`,
/// `lui 0x8008 - 0x47C4`, or `0x524(gp)`. The three encodings are the same
/// address, which is why a scan that only knows one of them under-reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeWordStore {
    /// VA of the `sh`.
    pub store_pc: u32,
    /// The mode the store writes.
    pub mode: GameMode,
}

/// The mode an INIT handler hands its RUN sibling, and the store that does it.
///
/// This is the INIT column's own exit, not a table field: `ModeEntry::next`
/// is the mode table's `+0x0A` word, which the *debug* advance chord reads,
/// while a live INIT handler ends by storing its successor itself. Every row
/// below is one such store, located by scanning each image for writes to
/// `_DAT_8007B83C` in all three addressing forms and reading back the literal
/// last loaded into the stored register.
///
/// `MonsterTest` is the row that shows why this cannot be `index + 1`: mode 4
/// stores `0`, so it bounces to the debug menu without ever reaching mode 5
/// ([`ModeInitBare`]).
///
/// `ReadInit`'s store is the only one outside `SCUS_942.54` - mode 16 jumps
/// into whatever sits in overlay slot A, and on the boot path that is
/// `init.pak` (PROT 0895), whose logo pass ends `li v0,0x11` /
/// `sh v0,-0x47c4(v1)` at `0x801CEC94`, in the delay slot of a `jal`.
///
/// `GameOverInit`'s store is the second one outside `SCUS_942.54`: it lives
/// in PROT 0902, at the head of the staged entry `0x801CE844`, as
/// `addiu v0,zero,0x13` (`0x801CE8D4`) / `sh v0,-0x47c4(at)` (`0x801CE8DC`).
/// An earlier reading left mode 18 without a successor because the scan
/// behind this table did not reach PROT 0902; a sweep of all three
/// addressing forms over `SCUS_942.54` plus the 83 mapped overlay images
/// finds 53 stores into the word, and that one among them.
pub const INIT_HANDOFFS: &[(GameMode, ModeWordStore)] = &[
    (
        GameMode::ConfigInit,
        ModeWordStore {
            store_pc: 0x8002_5D20,
            mode: GameMode::ConfigMode,
        },
    ),
    (
        GameMode::MainInit,
        ModeWordStore {
            store_pc: 0x8002_5E50,
            mode: GameMode::MainMode,
        },
    ),
    (
        GameMode::MonsterTest,
        ModeWordStore {
            store_pc: 0x8002_6120,
            mode: GameMode::ConfigInit,
        },
    ),
    (
        GameMode::MapdispInit,
        ModeWordStore {
            store_pc: 0x8002_5DF8,
            mode: GameMode::MapdispMode,
        },
    ),
    (
        GameMode::ReadInit,
        ModeWordStore {
            store_pc: 0x801C_EC94,
            mode: GameMode::ReadMode,
        },
    ),
    (
        GameMode::BattleInit,
        ModeWordStore {
            store_pc: 0x8005_5E4C,
            mode: GameMode::BattleMode,
        },
    ),
    (
        GameMode::CardInit,
        ModeWordStore {
            store_pc: 0x8002_5974,
            mode: GameMode::CardMode,
        },
    ),
    (
        GameMode::OtherInit,
        ModeWordStore {
            store_pc: 0x8002_5B04,
            mode: GameMode::OtherMode,
        },
    ),
    (
        GameMode::GameOverInit,
        ModeWordStore {
            store_pc: 0x801C_E8DC,
            mode: GameMode::GameOverMode,
        },
    ),
];

/// The mode an INIT handler leaves the word at, or `None` when `mode` is not
/// an INIT mode whose hand-off is pinned ([`INIT_HANDOFFS`]).
pub fn init_successor(mode: GameMode) -> Option<GameMode> {
    INIT_HANDOFFS
        .iter()
        .find(|(m, _)| *m == mode)
        .map(|(_, s)| s.mode)
}

/// The retail **boot mode chain**, in order, each step with the store that
/// takes it.
///
/// Read end to end off the disassembly, and it corrects two readings that
/// have been carried in prose:
///
/// * The title screen does **not** run under mode `0x10`. Mode 16 `READ INIT`
///   is one frame: `FUN_8002612C` jumps into slot A, which on the boot path
///   is `init.pak`'s logo pass, and that pass ends by storing `0x11`
///   (`0x801CEC94`). The publisher logos animate under `READ MODE` as
///   ordinary actors.
/// * The title runs under `CARD MODE` (`0x17`) - the same mode word as the
///   in-field pause menu. `init.pak`'s phase-3 arm runs
///   [`CORE_STATE_RESET`] and stores `0x16` `CARD INIT` at `0x801CF4D4`
///   whenever the entry word `0x8007BB00` is non-zero (`init.pak` raises it
///   itself), and mode 22's handler `FUN_8002574C` hands the word to `0x17`
///   at `0x80025974`. That is why the title dispatcher `FUN_801DD35C` is
///   resident in the *menu* overlay and is spawned by mode 22: the front-end
///   and the pause menu are one mode.
///
/// The `0` arm beside that store (`0x801CF4E4`, entry word zero) is the dev
/// route: it writes `CONFIG INIT`, the debug menu, instead.
///
/// The chain's last two steps are the title dispatcher's own NEW GAME store
/// (`0x801DFC00`, `legaia_engine_title::title_overlay`) and mode 2's hand-off.
pub const BOOT_MODE_CHAIN: &[ModeWordStore] = &[
    ModeWordStore {
        store_pc: 0x8001_D5B8,
        mode: GameMode::ReadInit,
    },
    ModeWordStore {
        store_pc: 0x801C_EC94,
        mode: GameMode::ReadMode,
    },
    ModeWordStore {
        store_pc: 0x801C_F4D4,
        mode: GameMode::CardInit,
    },
    ModeWordStore {
        store_pc: 0x8002_5974,
        mode: GameMode::CardMode,
    },
    ModeWordStore {
        store_pc: 0x801D_FC00,
        mode: GameMode::MainInit,
    },
    ModeWordStore {
        store_pc: 0x8002_5E50,
        mode: GameMode::MainMode,
    },
];

/// What a mode change costs, beside the new word.
///
/// Retail's dispatcher runs a fixed sequence whenever the handler it just
/// called left `gp[0x524] != gp[0x494]` (`0x800161B8..0x80016200`):
///
/// ```text
///   FUN_8003DE7C(0)          ; CD read-wait poll
///   FUN_8003ED04(0)          ; overlay-load wait
///   FUN_80016230()           ; mode-transition routine
///   FUN_80058104(0)          ; sound teardown
///   FUN_8001822C(gp+0x4F8)   ; re-publish the pad reports
///   gp+0x3D8 = 0             ; frame-begin-skip flag
///   gp+0x538 = 0             ; 0x8007B850, the live pad word
///   0x8007B938 = 0
///   gp+0x55C = 0             ; 0x8007B874, the pad *edge* word
///   gp+0x564 = gp+0x494 = new mode
/// ```
///
/// The two pad clears are the half with observable behaviour: the button that
/// caused a transition is not also delivered as the new mode's first input.
/// The engine's equivalent is [`legaia_engine_system::input::InputState::clear_edges`], which
/// drops the edges and leaves what is held alone.
///
/// The `0x8007B938` clear is a fourth store the earlier three-clear reading of
/// this block missed, and `gp+0x564` / `gp+0x494` are *copies of the new mode*,
/// not clears.
// REF: FUN_8003DE7C, FUN_8003ED04, FUN_80016230, FUN_80058104, FUN_8001822C
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeChangeEdge {
    /// The mode the word held before the change (`gp+0x494`).
    pub from: GameMode,
    /// The mode it holds now (`gp+0x524`).
    pub to: GameMode,
    /// The engine performed the pad-edge swallow (`gp+0x538` / `gp+0x55C`).
    pub swallowed_pad_edges: bool,
    /// The engine cleared the frame-begin-skip flag (`gp+0x3D8`).
    pub cleared_frame_begin_skip: bool,
}

/// The staging an INIT mode asks its host for, in the one shape a host can
/// match on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInitPlan {
    /// The [`ModeInitStage`] wrapper shape - an overlay-A request plus the
    /// entry the handler calls once the load lands.
    Stage(ModeInitStage),
    /// The mode-24 warp dispatcher's per-sub-id staging
    /// ([`other_warp_init_stage`]).
    Warp(ModeInitStage),
    /// An INIT handler that stages nothing ([`ModeInitBare`]).
    Bare(ModeInitBare),
}

/// One frame's worth of mode-table dispatch, as a [`ModeSeat`] resolved it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeFrame {
    /// The mode word this frame ran under.
    pub game_mode: GameMode,
    /// The [`SceneMode`] the `(mode, warp sub-id)` pair resolves to.
    pub scene_mode: SceneMode,
    /// The transition this frame opened with, if the word changed.
    pub edge: Option<ModeChangeEdge>,
    /// The INIT column's staging plan, for an INIT (even-indexed) mode. The
    /// seat resolves it and then performs that mode's hand-off store, so a
    /// host sees each plan on exactly the frame retail's handler ran.
    pub init: Option<ModeInitPlan>,
    /// The per-frame staging plan, for a per-frame (odd-indexed) mode.
    pub stage: Option<PerFrameStage>,
    /// Whether this mode's handler calls the master frame driver - `false`
    /// only for `CARD MODE` ([`runs_master_frame_driver`]).
    pub runs_master_driver: bool,
}

/// A host's **seat at the mode table**: the port's copy of `_DAT_8007B83C`,
/// plus the dispatch retail's `main` loop performs around it.
///
/// PORT: FUN_80015E90 (the mode-table loop, `0x8001615C..0x8001620C`)
///
/// Retail's outermost level is a three-line loop over one halfword: index the
/// 28-entry table at `0x8007078C`, call `+0x10`, and if the handler changed
/// the word, run the transition edge. The port has the table
/// ([`TABLE`]), the per-frame level ([`per_frame_stage`]) and the inner frame
/// driver (`World::tick`); this is the outer level, and the thing that makes
/// it a seat rather than a mirror is that its **writes are the port's own
/// transitions**. A host calls [`Self::enter`] where retail's code stores the
/// word, and gets back the INIT column's staging plan for that mode.
///
/// ## What owns what
///
/// The word is the seat's. [`SceneMode`] stays the scene sessions' - they own
/// the loaded assets, and the port's minigames are resident rules engines
/// rather than paged overlays, so a session outlives the mode word that
/// staged it. The two are reconciled once per frame by
/// [`Self::adopt_scene_mode`], which writes the word when a session moved the
/// world somewhere the word does not name. That direction is lossy exactly
/// where [`GameMode::for_scene_mode`] says it is (the five warp minigames all
/// answer `OTHER MODE`), so the seat stages the sub-id alongside, and
/// [`Self::scene_mode`] round-trips.
///
/// An INIT frame is never adopted away: the word an [`Self::enter`] left is
/// what the frame runs under, and the hand-off to the RUN sibling
/// ([`INIT_HANDOFFS`]) happens after the host has staged what the plan named.
#[derive(Debug)]
pub struct ModeSeat {
    driver: ModeDriver,
    /// `gp+0x494` - the word as of the last edge, which is what the loop's
    /// `bne` compares against.
    previous: GameMode,
    /// Mode changes taken since the seat opened.
    edges: u64,
    /// `_DAT_8007BB00` - the **front-end entry word**, the companion store of
    /// [`legaia_engine_mode::card_request::request_card_mode`].
    ///
    /// It is the flag `init.pak`'s hand-off arm reads to choose between the
    /// front end and the debug menu ([`Self::boot_handoff`]), and the flag the
    /// title dispatcher's `Init` reads to route to `0x11` instead of the
    /// retail-unreachable `0x02` (`0x801DD97C`,
    /// [`legaia_engine_system::mode_entry_init::ENTRY_WORD_ADDR`]). `init.pak`
    /// raises it itself on a cold boot, which is why the seat opens with it
    /// set.
    pub(super) entry_word: u32,
    /// `_DAT_8007B820` - the title menu's **row counter**. A power-on global:
    /// only the title's own Up / Down arm and init.pak's card scan write it
    /// (the scan never matches a US save, `docs/subsystems/boot.md`), so
    /// every later title opens on whichever row it last held. The seat lives
    /// across every title a session raises, which is why the word is here.
    title_row: u8,
}

impl ModeSeat {
    /// Open a seat at the retail boot mode: `0x10` `READ INIT`, the word
    /// `0x8001D5B8` writes before the first dispatch
    /// ([`BOOT_MODE_CHAIN`]).
    pub fn new_at_boot() -> Self {
        Self::new(GameMode::ReadInit)
    }

    /// Open a seat at an arbitrary mode (a host resuming mid-game, a test).
    pub fn new(start: GameMode) -> Self {
        Self {
            driver: ModeDriver::new(start),
            previous: start,
            edges: 0,
            entry_word: legaia_engine_system::mode_entry_init::ENTRY_WORD_COLD_BOOT,
            title_row: 0,
        }
    }

    /// The current mode word.
    pub fn game_mode(&self) -> GameMode {
        self.driver.current()
    }

    /// The word as of the last edge (`gp+0x494`).
    pub fn previous_mode(&self) -> GameMode {
        self.previous
    }

    /// Mode changes taken since the seat opened.
    pub fn edges(&self) -> u64 {
        self.edges
    }

    /// The staged warp sub-id ([`WARP_SUB_ID_ADDR`]).
    pub fn warp_sub_id(&self) -> Option<i16> {
        self.driver.warp_sub_id()
    }

    /// The `(mode, sub-id)` pair's [`SceneMode`].
    pub fn scene_mode(&self) -> SceneMode {
        self.driver.scene_mode()
    }

    /// The debug label of the current mode's table row.
    pub fn mode_name(&self) -> &'static str {
        self.driver.entry().name
    }

    /// The front-end entry word `_DAT_8007BB00`.
    pub fn entry_word(&self) -> u32 {
        self.entry_word
    }

    /// The title row counter `_DAT_8007B820` (`0` NEW GAME, `1` CONTINUE).
    pub fn title_row(&self) -> u8 {
        self.title_row
    }

    /// Record the row the running title holds
    /// (`legaia_engine_title::title::TitleSession::row_counter`); the next title opens
    /// there (`legaia_engine_title::title::TitleSession::for_front_end_at`).
    pub fn set_title_row(&mut self, row: u8) {
        self.title_row = row;
    }

    /// Request the front end, both stores: the mode word to `CARD INIT` and
    /// the entry word raised.
    ///
    /// This is [`legaia_engine_mode::card_request::request_card_mode`]'s pair applied to
    /// the seat - the leaf is seven instructions and two stores, and this is
    /// where they land. Retail's field image calls it to hand the frame to
    /// the card / title screen; the port's hosts call it to open the pause
    /// menu and to re-enter the front end.
    pub fn request_card_mode(&mut self) {
        let req = crate::card_request::request_card_mode();
        if let Some(mode) = GameMode::from_index(req.game_mode as usize) {
            self.driver.jump_to(mode);
        }
        self.entry_word = req.flag;
    }

    /// The mode the boot pass hands off to once the publisher logos finish -
    /// retail's `init.pak` phase-3 arm.
    ///
    /// `0x801CF490..0x801CF4E8`: on `state[0x801F3EB0] == 3` it runs
    /// [`CORE_STATE_RESET`] and then branches on the entry word - non-zero
    /// stores `0x16` `CARD INIT` (the front end) at `0x801CF4D4`, zero stores
    /// `0` `CONFIG INIT` (the debug menu) at `0x801CF4E4` and clears the word.
    /// The seat performs the branch and the store, and returns the mode so a
    /// host can raise the screen the new mode owns.
    pub fn boot_handoff(&mut self) -> GameMode {
        let to = if self.entry_word == 0 {
            GameMode::ConfigInit
        } else {
            GameMode::CardInit
        };
        if to == GameMode::ConfigInit {
            self.entry_word = 0;
        }
        self.driver.jump_to(to);
        to
    }

    /// Stage the warp sub-id, as the field VM's `0x3E` door-warp arm does
    /// before handing the word to `OTHER INIT`.
    pub fn stage_warp(&mut self, sub_id: Option<i16>) {
        self.driver.set_warp_sub_id(sub_id);
    }

    /// Write the mode word - the port's counterpart of one of retail's `sh`
    /// stores. The edge is taken on the next [`Self::frame`], the way the
    /// dispatcher takes it after the handler returns.
    pub fn write_mode(&mut self, mode: GameMode) {
        self.driver.jump_to(mode);
    }

    /// Enter an INIT mode **now**: take the edge, resolve the INIT column's
    /// staging plan, and leave the word at the mode's RUN sibling.
    ///
    /// This is one retail INIT handler's whole body minus the overlay load
    /// the port replaces with native scene entry: the caller performs what
    /// the returned plan names, and the seat performs the hand-off store
    /// ([`INIT_HANDOFFS`]). It is [`Self::frame`] with the word written
    /// first, so a host that enters a mode mid-frame and a host that lets the
    /// seat reach it on its own run the same code. `None` comes back for a
    /// mode that stages nothing.
    ///
    /// `world` is needed for the edge itself ([`ModeChangeEdge`]).
    pub fn enter<W: ModeWorld + ?Sized>(
        &mut self,
        mode: GameMode,
        world: &mut W,
    ) -> Option<ModeInitPlan> {
        self.write_mode(mode);
        self.frame_inner(world, true).init
    }

    /// The INIT column's plan for `mode`, without entering it.
    pub fn init_plan(&self, mode: GameMode) -> Option<ModeInitPlan> {
        if matches!(mode, GameMode::OtherInit) {
            return self
                .driver
                .warp_sub_id()
                .and_then(other_warp_init_stage)
                .map(ModeInitPlan::Warp);
        }
        if let Some(stage) = mode_init_stage(mode) {
            return Some(ModeInitPlan::Stage(stage));
        }
        mode_init_bare(mode).map(ModeInitPlan::Bare)
    }

    /// Reconcile the word with a [`SceneMode`] a scene session moved the
    /// world to. Returns the write it made, if any.
    ///
    /// Never overwrites an INIT frame, and never writes for
    /// [`SceneMode::Title`] - the port uses that variant for "no scene
    /// loaded", which is a state the retail word has no single answer for.
    pub fn adopt_scene_mode(&mut self, scene: SceneMode) -> Option<GameMode> {
        if init_successor(self.driver.current()).is_some() {
            return None;
        }
        if self.driver.scene_mode() == scene {
            return None;
        }
        let target = GameMode::for_scene_mode(scene)?;
        if matches!(target, GameMode::OtherMode) {
            // The lossy arm: five scene modes share `OTHER MODE`, so the word
            // alone would not round-trip. Stage the sub-id retail's warp arm
            // would have left standing.
            let sub = crate::minigame_entry::MinigameSubId::ALL
                .iter()
                .find(|s| s.scene_mode() == Some(scene))
                .map(|s| i16::from(s.sub_id()));
            self.driver.set_warp_sub_id(sub);
        }
        self.driver.jump_to(target);
        Some(target)
    }

    /// Reconcile the word with a world's scene mode, honouring the
    /// battle-intro hold.
    ///
    /// The host-facing form of [`Self::adopt_scene_mode`]. The port seats the
    /// battle scene at the encounter trigger where retail seats it at the end
    /// of the intro spin, so `world.mode` reaches `Battle` a whole transition
    /// early; retail's own write of `_DAT_8007B83C = 0x14` waits for the
    /// clock and `ready == 3`
    /// (`legaia_engine_core::world::World::battle_mode_word_held`).
    /// Consulting the world here rather than at each host is what keeps the
    /// native window and the browser page on the same edge.
    ///
    /// REF: FUN_801CF5BC - the transition kernel whose `0x801CF8F8` store this
    /// gates the port's own word on. Deliberately a `REF:` and not a `PORT:`:
    /// the routine is already ported as
    /// `legaia_render_kernels::battle_intro_transition::tick_transition`, and a
    /// second anchor here would put an address on code that implements none
    /// of it.
    pub fn adopt_world_mode<W: ModeWorld + ?Sized>(&mut self, world: &W) -> Option<GameMode> {
        if world.battle_mode_word_held() {
            return None;
        }
        self.adopt_scene_mode(world.scene_mode())
    }

    /// Take the mode-change edge if the word moved since the last one.
    ///
    /// `swallow` decides whether the edge performs retail's pad-edge clears,
    /// and the answer is not "always" - see [`Self::frame`] for why an
    /// adopted change must not.
    fn take_edge<W: ModeWorld + ?Sized>(
        &mut self,
        world: &mut W,
        swallow: bool,
    ) -> Option<ModeChangeEdge> {
        let to = self.driver.current();
        if to == self.previous {
            return None;
        }
        let from = self.previous;
        if swallow {
            // The two pad clears (`gp+0x538` / `gp+0x55C`): the button that
            // caused the transition is not delivered again as the new mode's
            // first input.
            world.clear_pad_edges();
        }
        // `gp+0x3D8`.
        world.clear_frame_begin_skip();
        self.previous = to;
        self.edges += 1;
        Some(ModeChangeEdge {
            from,
            to,
            swallowed_pad_edges: swallow,
            cleared_frame_begin_skip: true,
        })
    }

    /// Drive one frame of the mode table: take any pending edge, resolve the
    /// frame's staging plan - the INIT column's for an INIT mode, the
    /// per-frame column's otherwise - and hand the word on where the INIT
    /// handler would have.
    ///
    /// A host calls this once per frame before its own frame body; the
    /// [`ModeFrame::runs_master_driver`] field is the same rule
    /// `World::tick` applies internally, surfaced so a host can skip its own
    /// render / actor work under `CARD MODE` too.
    ///
    /// **This edge does not swallow the pad, and the reason is an ordering
    /// mismatch rather than a fidelity choice.** Retail's transition block
    /// clears the pad words and the *next* loop pass polls the pad fresh, so
    /// the clear only ever discards the mode it left. The port's hosts publish
    /// a pad word immediately *before* each tick, so clearing here would
    /// discard this frame's own input - the button the player is pressing at
    /// the new mode, not the one that left the old one. The swallow therefore
    /// belongs to the transitions a host performs synchronously mid-frame,
    /// which is what [`Self::enter`] does.
    pub fn frame<W: ModeWorld + ?Sized>(&mut self, world: &mut W) -> ModeFrame {
        self.frame_inner(world, false)
    }

    fn frame_inner<W: ModeWorld + ?Sized>(&mut self, world: &mut W, swallow: bool) -> ModeFrame {
        let edge = self.take_edge(world, swallow);
        let mode = self.driver.current();
        let out = ModeFrame {
            game_mode: mode,
            scene_mode: self.driver.scene_mode(),
            edge,
            init: self.init_plan(mode),
            stage: per_frame_stage(mode),
            runs_master_driver: runs_master_frame_driver(mode),
        };
        // An INIT mode is one frame. Retail's handler ends by storing its own
        // successor ([`INIT_HANDOFFS`]), so the word has moved on by the time
        // the loop comes round again - which is also what keeps a seat from
        // parking on an INIT mode nothing dispatched.
        if let Some(next) = init_successor(mode) {
            self.driver.jump_to(next);
        }
        out
    }
}

impl Default for ModeSeat {
    fn default() -> Self {
        Self::new_at_boot()
    }
}
