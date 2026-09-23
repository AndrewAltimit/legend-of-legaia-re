//! Cutscene presentation state: narration, timeline, caption / card / balloon overlays, FMV handoff and the opening-chain latches.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Cutscene presentation state: narration, timeline, caption / card / balloon overlays, FMV handoff and the opening-chain latches.
pub struct CutsceneState {
    /// Pending FMV trigger (field-VM op `0x4C 0xE2`). When `Some(fmv_id)`,
    /// the field VM has signalled that the next-game-mode global should
    /// transition to game mode 26 (StrInit) with the given index. Engines
    /// drain this after [`crate::world::World::tick`] to actually open the corresponding
    /// `MV*.STR` (use [`crate::cutscene::fmv_index_to_str_filename`] for
    /// the retail mapping). `None` between triggers.
    pub pending_fmv_trigger: Option<i16>,
    /// The FMV currently playing in [`crate::world::SceneMode::Cutscene`]. Set when the
    /// world consumes a [`crate::world::CutsceneState::pending_fmv_trigger`] at the top of a
    /// [`crate::world::World::tick`] and flips into the cutscene mode (mirroring retail's
    /// next-game-mode dispatch to game mode 26 one frame after the field-VM
    /// op writes the global). While `Some`, the field VM is suspended (the
    /// STR overlay owns the frame in retail); the host plays the resolved
    /// `MV*.STR` and calls [`crate::world::World::finish_cutscene`] when playback ends.
    /// `None` outside an STR-FMV cutscene.
    pub active_fmv: Option<i16>,
    /// Scene mode to restore when the active STR-FMV cutscene finishes
    /// (set on entry, consumed by [`crate::world::World::finish_cutscene`]). Retail
    /// returns to the field after the cutscene overlay unloads; `None`
    /// outside a cutscene.
    pub return_mode: Option<SceneMode>,
    /// The `fmv_id` whose playback just ended, parked here by
    /// [`crate::world::World::finish_cutscene`] for exactly one drain by
    /// [`crate::scene::SceneHost::apply_pending_fmv_handoff`].
    ///
    /// Retail's post-play control transfer is not a world-only decision - the
    /// [`FmvHandoff::Field`](crate::cutscene::FmvHandoff::Field) arm loads a
    /// *different scene*, which needs the host's asset index. So the world
    /// records "an FMV finished, and which one" and the scene host performs
    /// the transfer. Draining it is a `take`: the transfer runs once however
    /// many hosts poll.
    pub finished_fmv: Option<i16>,
    /// Live **script-cutscene elements** - the pool the position tween
    /// (`FUN_801D5C08`), the teardown (`FUN_801D5D60`) and the ambient emitter
    /// (`FUN_801D6058`) run on, each carrying the linked object whose done bit
    /// gates it. See [`crate::world::cutscene_elements`].
    pub elements: Vec<crate::world::CutsceneElement>,
    /// What the element channel produced on the last tick - the writes, the
    /// teardown requests and the ambient particles a host reads back.
    pub element_frame: crate::world::ElementFrame,
    /// Active opening-cutscene narration presenter, or `None` when no cutscene
    /// narration is playing. Installed by [`crate::world::World::open_cutscene_narration`]
    /// (the `opdeene` opening prologue) with the inline subtitle pages decoded
    /// from the scene MAN's cutscene-timeline script; its per-page timer is
    /// advanced in [`crate::world::World::tick`], and the host renders [`Self::narration`]'s
    /// current page. It gates the prologue hand-off: while it is active the
    /// confirm press skips narration pages, and only once it completes does a
    /// confirm reach [`crate::world::World::take_prologue_handoff`].
    pub narration: Option<crate::cutscene_narration::CutsceneNarration>,
    /// Monotonic counter incremented each time [`crate::world::World::open_cutscene_narration`]
    /// installs a crawl block. Lets observers distinguish back-to-back crawl
    /// blocks (a non-blocking crawl opens the next block the same tick the prior
    /// scrolls out) that a rising-edge `active`-watch would merge into one.
    pub narration_seq: u32,
    /// The crawl config block (`*0x801C6EA4 +0x4C..+0x52`) the next roller
    /// reads: the scene reset's values until a timeline `CC F8 E8` seed op
    /// overwrites them. See [`crate::cutscene_narration::RollerSeed`].
    pub narration_seed: crate::cutscene_narration::RollerSeed,
    /// Active opening-cutscene timeline executor, or `None` when no cutscene
    /// timeline is running. Installed by
    /// [`crate::world::World::load_cutscene_timeline_from_man`] (the `opdeene` opening
    /// prologue) with the partition-2 record that issues `GFLAG_SET 26`;
    /// stepped each frame by [`crate::world::World::step_cutscene_timeline`] so the cutscene's
    /// camera path + actor moves play and the hand-off bit fires by execution.
    /// See [`crate::cutscene_timeline::CutsceneTimeline`].
    ///
    /// This is the single **modal** context slot: while it is active the
    /// cutscene camera owns the frame and pad locomotion is locked
    /// ([`crate::world::World::cutscene_timeline_active`] gates). Ordinary mid-play spawned
    /// records execute concurrently in [`crate::world::FieldVmState::helper_contexts`] instead and
    /// never seize either.
    pub timeline: Option<crate::cutscene_timeline::CutsceneTimeline>,
    /// `true` only while [`crate::world::World::step_cutscene_timeline`] is executing the
    /// spawned cutscene context. The field-VM host reads it to suppress the
    /// actor-allocator hook (op `0x4C` n8 sub-0), which in the cutscene context
    /// (target `0xF8`) is the inline-narration text-draw the separate
    /// [`crate::world::CutsceneState::narration`] presenter owns - not an actor spawn.
    pub in_timeline: bool,
    /// Set when the `town01` opening cutscene timeline is installed via the
    /// new-game prologue hand-off. While set, the timeline's first op-`0x49`
    /// STATE_RESUME (the pinned name-entry handoff at P2[3] body `0x02c6`) opens
    /// the name-entry overlay instead of parking generically. One-shot for the
    /// opening; a normal `town01` visit never sets it. See
    /// [`crate::world::World::install_town01_opening_timeline`].
    pub prologue_naming_pending: bool,
    /// Set once the timeline's op-`0x49` has opened the name-entry overlay, so
    /// the op suspends (Armed) until the player commits a name, then resumes
    /// (Done) - and never re-opens it on the record's later STATE_RESUMEs.
    pub prologue_naming_armed: bool,
    /// Set by [`crate::world::World::take_prologue_handoff`] when it hands off to `town01`, so
    /// the next `town01` field entry installs the opening cutscene timeline
    /// (establishing shot + Vahn walk-out + name-entry handoff). Cleared when
    /// the entry consumes it, so only the prologue path runs the opening.
    pub entering_town01_opening: bool,
    /// The active opening-cutscene static title card (narration `0x89`
    /// blocks): pages shown simultaneously, centered mid-screen, until a
    /// blank card block clears it (the `map01` fly-in's "twilight of
    /// humanity" card). Rendered by the host; independent of the crawl
    /// roller [`crate::world::CutsceneState::narration`].
    pub card: Option<Vec<String>>,
    /// The `opdeene` "It was the Seru." caption, decoded to RGBA at scene
    /// entry ([`crate::cutscene_caption::decode_opdeene_caption`]). `Some`
    /// only while `opdeene` is loaded; the host uploads it once as a sprite
    /// atlas and blits it, faded by [`crate::world::CutsceneState::caption_alpha`]. Unlike
    /// the crawl / card this is a pre-rendered image, not font text - retail
    /// draws it as a scene textured quad, so the engine blits the scene
    /// texture rather than rendering a string. See [`crate::cutscene_caption`].
    pub caption: Option<crate::cutscene_caption::CaptionImage>,
    /// The live `4C E1` single-line text balloon, spawned by the field-VM
    /// menu-ctrl sub-op (`FUN_8003C764`) and ticked per frame
    /// ([`crate::text_balloon::TextBalloon::tick`], the `FUN_801DA7F0`
    /// handler). Spawning replaces any live balloon - the retail
    /// predecessor-kill. Hosts render `text` at `(x, y)` while it runs.
    pub text_balloon: Option<crate::text_balloon::TextBalloon>,
    /// Fade level (0..=1) of [`crate::world::CutsceneState::caption`], ramped each
    /// [`crate::world::World::tick`]. Target-visible in the gap after the first narration
    /// crawl block scrolls out and before the second opens (retail shows the
    /// caption once, between `opdeene`'s two crawls).
    pub caption_alpha: f32,
    /// Frames [`crate::world::CutsceneState::caption`] has been fully faded in. Used to bound
    /// the caption to a retail-like ~2 s beat and fade it back out, since the
    /// engine's inter-crawl timeline gap currently runs much longer than
    /// retail's - so the caption reads as a deliberate pause, not a freeze,
    /// even when the second crawl block is still frames away. Reset on scene
    /// entry; never re-shows once the hold elapses (the gap continues hidden).
    pub caption_shown_frames: u32,
    /// `true` while the New-Game opening cutscene chain is playing (from the
    /// `opdeene` entry through its `opstati` / `opurud` / world-map fly-in
    /// legs, until `town01` is entered). While set, a confirm press with the
    /// hand-off bit armed skips the WHOLE remaining opening to `town01` -
    /// retail's `FUN_801D1344` packet is a skip available any time after
    /// `opdeene` arms `GFLAG 26`, not a post-narration gate. Set when the
    /// prologue cutscene scene is entered; cleared by the skip or by the
    /// `town01` opening entry.
    pub opening_chain_active: bool,
}

impl CutsceneState {
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
            element_frame: Default::default(),
            pending_fmv_trigger: None,
            active_fmv: None,
            return_mode: None,
            finished_fmv: None,
            narration: None,
            narration_seq: 0,
            narration_seed: crate::cutscene_narration::RollerSeed::SCENE_RESET,
            timeline: None,
            in_timeline: false,
            prologue_naming_pending: false,
            prologue_naming_armed: false,
            entering_town01_opening: false,
            card: None,
            caption: None,
            text_balloon: None,
            caption_alpha: 0.0,
            caption_shown_frames: 0,
            opening_chain_active: false,
        }
    }
}

impl Default for CutsceneState {
    fn default() -> Self {
        Self::new()
    }
}
