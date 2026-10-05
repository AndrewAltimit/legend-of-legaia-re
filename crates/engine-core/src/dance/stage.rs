//! Count-in banner, clip / face-rig gates, scene entry and stage, and the sprite-part emitter.
//! Split out of `dance.rs`.

use super::*;

/// One past the last frame of the count-in banner's timeline: slide-in
/// (`0x1e`) + hold (to `0x5a`) + slide-out (`0x1e` more).
pub const COUNTIN_END_FRAME: i32 = 0x5a + 0x1e;

/// The banner counter at which `FUN_801cf470`'s state 3 leaves the READY
/// banner (`slti v0,v0,0x6f` at `0x801CFAB4`, read after the animator ran):
/// the slide-out is cut there, short of [`COUNTIN_END_FRAME`].
pub const COUNTIN_READY_EXIT: i32 = 0x6F;

/// `GO!` fade accumulator step per animator run: states 4 / 5 add / subtract
/// `dt * 2` (`sll v1,v1,0x1` at `0x801CFC08` / `0x801CFC5C`), and the hall runs
/// at `dt = 3`.
pub const COUNTIN_GO_STEP: i32 = 2 * COUNTIN_ANIM_STEP;

/// The `GO!` accumulator's ceiling: state 4 leaves once it reaches `0x3D`
/// (`slti v0,v0,0x3d` at `0x801CFC14`) and parks it at `0x3C`.
pub const COUNTIN_GO_FULL: i32 = 0x3C;

/// State 4 fires the run-start cue once the accumulator has reached `0x1F`
/// (`slti v0,v0,0x1f` at `0x801CFBDC`), and only after the READY hold's
/// intro cue raised the latch `DAT_801D5134` to `1`.
pub const COUNTIN_START_CUE_AT: i32 = 0x1F;

/// The run-start cue (`li v0,0x201` / `sh v0,-0x4928(v1)` at `0x801CFBF0`).
pub const COUNTIN_START_CUE: u16 = 0x201;

/// Where the count-in is: retail's states 3 (READY), 4 (`GO!` in) and 5
/// (`GO!` out) of `FUN_801cf470`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CountInStage {
    /// State 3: the READY banner animator `FUN_801d2d98`.
    #[default]
    Ready,
    /// State 4: `GO!` (widget `0x0C`) fades in.
    GoIn,
    /// State 5: `GO!` fades out.
    GoOut,
    /// State 6 reached: the song starts.
    Done,
}

/// The pre-song count-in as an advancing object - the READY banner's frame
/// counter, the `GO!` fade accumulator `DAT_801D515C`, and the cue latch
/// `DAT_801D5134` (`1` once the intro cue fired, `2` once the start cue did).
///
/// Retail runs the below-10 states of `FUN_801cf470` before the beat clock
/// starts; the port stages one of these on [`crate::world::World::enter_dance`]
/// and the world's dance tick plays it out, holding `DanceGame::advance` off
/// until it finishes. Owning the counter here rather than in a host is what
/// makes the native window, the browser play page and the minigames page
/// count in identically - and what gives the **door-warp** entry a count-in
/// at all.
///
/// The three states, read off the disassembly:
///
/// * **State 3** draws `FUN_801d2d98(counter)` every run and leaves once the
///   counter it drew is at least [`COUNTIN_READY_EXIT`]; the counter grows by
///   `dt` at the tail of every run (`0x801D015C..0x801D0184`).
/// * **State 4** draws `GO!` (widget `0x0C`, `(0xA0, 0x78)`) at brightness
///   `acc * 2` (`sll a3,a3,0x1` at `0x801CFCC4`) while `acc` climbs by
///   [`COUNTIN_GO_STEP`]; the run that reaches `0x3D` parks it at
///   [`COUNTIN_GO_FULL`]. On the way it fires [`COUNTIN_START_CUE`].
/// * **State 5** draws the same while `acc` falls back by the same step; the
///   run that takes it below zero clears it and hands over to state 6, which
///   starts the song.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CountIn {
    pub(super) frame: i32,
    /// Vsyncs since the animator last ran, `0..COUNTIN_ANIM_PERIOD_VSYNCS`.
    pub(super) phase: u8,
    pub(super) cue_fired: bool,
    pub(super) stage: CountInStage,
    /// `DAT_801D515C`, the `GO!` fade accumulator.
    pub(super) go_acc: i32,
    pub(super) start_cue_fired: bool,
    /// What the last run drew: the READY envelope, or the `GO!` brightness.
    pub(super) view: CountInView,
}

/// What the count-in draws this vsync.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CountInView {
    /// The READY banner's envelope while state 3 runs.
    pub banner: Option<CountInBanner>,
    /// `GO!`'s brightness (`acc * 2`) while states 4 / 5 run.
    pub go: Option<i32>,
}

/// Vsyncs between two runs of the count-in banner animator.
///
/// Retail does not step the banner every frame: it calls the animator once per
/// **three** vsyncs, and the counter it hands in advances by
/// [`COUNTIN_ANIM_STEP`] each time. Same wall-clock duration, coarser
/// sampling - the sliding halves jump 18 px per visible step where a
/// per-vsync port slides them 6.
pub const COUNTIN_ANIM_PERIOD_VSYNCS: u8 = 3;

/// How far the animator's own counter advances per run - the same 3 (`dt`).
pub const COUNTIN_ANIM_STEP: i32 = 3;

/// Animator runs the whole count-in takes: READY draws counters
/// `0, 3, ..` through the first at or past [`COUNTIN_READY_EXIT`], `GO!` climbs
/// to its ceiling and falls back below zero.
pub const COUNTIN_RUNS: i32 = (COUNTIN_READY_EXIT + COUNTIN_ANIM_STEP - 1) / COUNTIN_ANIM_STEP
    + 1
    + 2 * ((0x3D + COUNTIN_GO_STEP - 1) / COUNTIN_GO_STEP);

/// Vsyncs from the count-in's first frame to the one that reports `done`
/// (the first vsync of state 6's run).
pub const COUNTIN_TOTAL_VSYNCS: i32 = COUNTIN_RUNS * COUNTIN_ANIM_PERIOD_VSYNCS as i32 + 1;

/// What one [`CountIn::step`] produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountInStep {
    /// This frame's READY envelope, `None` once state 3 has left.
    pub banner: Option<CountInBanner>,
    /// This frame's `GO!` brightness, `None` outside states 4 / 5.
    pub go: Option<i32>,
    /// The intro cue ([`COUNTIN_INTRO_CUE`]) on the READY hold's first frame,
    /// or the start cue ([`COUNTIN_START_CUE`]) during the `GO!` fade-in.
    pub cue: Option<u16>,
    /// State 6 reached: the caller starts the song this frame.
    pub done: bool,
}

impl CountIn {
    pub fn new() -> Self {
        Self::default()
    }

    /// Frames elapsed (the banner animator's own counter).
    pub fn frame(&self) -> i32 {
        self.frame
    }

    /// Which state the count-in is in.
    pub fn stage(&self) -> CountInStage {
        self.stage
    }

    /// What the count-in draws now, without advancing.
    pub fn view(&self) -> CountInView {
        self.view
    }

    /// The READY envelope at the current counter - what a `&self` draw
    /// builder reads while state 3 runs.
    pub fn banner(&self) -> CountInBanner {
        dance_countin_banner_envelope(self.frame)
    }

    /// One animator run: retail's per-tick body of the current state.
    fn run(&mut self) -> Option<u16> {
        let mut cue = None;
        match self.stage {
            CountInStage::Ready => {
                let banner = dance_countin_banner_envelope(self.frame);
                if banner.hold && !self.cue_fired {
                    self.cue_fired = true;
                    cue = Some(COUNTIN_INTRO_CUE);
                }
                self.view = CountInView {
                    banner: Some(banner),
                    go: None,
                };
                if self.frame >= COUNTIN_READY_EXIT {
                    self.stage = CountInStage::GoIn;
                    self.go_acc = 0;
                }
                self.frame += COUNTIN_ANIM_STEP;
            }
            CountInStage::GoIn => {
                if self.cue_fired && !self.start_cue_fired && self.go_acc >= COUNTIN_START_CUE_AT {
                    self.start_cue_fired = true;
                    cue = Some(COUNTIN_START_CUE);
                }
                self.go_acc += COUNTIN_GO_STEP;
                if self.go_acc > COUNTIN_GO_FULL {
                    self.go_acc = COUNTIN_GO_FULL;
                    self.stage = CountInStage::GoOut;
                }
                self.view = CountInView {
                    banner: None,
                    go: Some(self.go_acc * 2),
                };
            }
            CountInStage::GoOut => {
                self.go_acc -= COUNTIN_GO_STEP;
                if self.go_acc < 0 {
                    self.go_acc = 0;
                    self.stage = CountInStage::Done;
                }
                self.view = CountInView {
                    banner: None,
                    go: Some(self.go_acc * 2),
                };
            }
            CountInStage::Done => {
                self.view = CountInView::default();
            }
        }
        cue
    }

    /// Advance one **vsync**.
    ///
    /// The animator runs on the first vsync of every
    /// [`COUNTIN_ANIM_PERIOD_VSYNCS`] and its picture holds for the period -
    /// so this returns the same view three vsyncs running and then jumps.
    pub fn step(&mut self) -> CountInStep {
        let (cue, done) = if self.phase == 0 {
            let done = self.stage == CountInStage::Done;
            (self.run(), done)
        } else {
            (None, false)
        };
        self.phase += 1;
        if self.phase >= COUNTIN_ANIM_PERIOD_VSYNCS {
            self.phase = 0;
        }
        CountInStep {
            banner: self.view.banner,
            go: self.view.go,
            cue,
            done,
        }
    }
}

/// The intro-cue id the count-in banner fires once, when it crosses into its
/// hold segment (`FUN_801d2d98`, into the runtime SFX bank; see `sfx-table.md`).
pub const COUNTIN_INTRO_CUE: u16 = 0x200;

/// The count-in banner's slide / hold / fade envelope for one frame
/// (`FUN_801d2d98`). The two banner halves emit through the hub sprite emitter
/// [`FUN_801d2f38`]; this is the arithmetic that emit is fed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountInBanner {
    /// Horizontal offset of the sliding halves from screen centre (`0xa0`); `0`
    /// during the hold. Retail places the right half at `0xa0 + x_offset`, the
    /// left at `0xa0 - x_offset`.
    pub x_offset: i32,
    /// Brightness passed to the emit, clamped `0..=0xff`. Halved for the two
    /// sliding halves; full for the single held banner.
    pub brightness: i32,
    /// `true` = the single centred banner (widget `0x78`, opaque); `false` = the
    /// two half-brightness sliding halves (widget `0x77`).
    pub hold: bool,
}

// Wired: the play window runs a pre-song count-in phase (the shell holds the
// parsed [`DanceGame`] pending while the banner's own frame counter runs,
// entering the dance only when the envelope finishes - the `FUN_801cf470`
// below-10 states as a host phase). The host owns the counter and the
// once-only [`COUNTIN_INTRO_CUE`] latch; this returns the envelope.
/// PORT: FUN_801d2d98 - the count-in banner animator (`1 2 3 READY... GO!`).
///
/// Three segments keyed on the banner's own frame counter `frame`:
/// **slide-in** (`frame < 0x1e`): the two halves fly in from `0xb4 - 6*frame`
/// to centre at flat brightness `0x80`; **hold** (`0x1e..0x5a`): a single
/// centred banner whose brightness ramps `((frame-0x1e)*0x7f)/0x1e + 0x80`
/// (clamped `0xff`) - and the once-only intro cue [`COUNTIN_INTRO_CUE`] fires
/// on entry; **slide-out** (`>= 0x5a`): the halves fly back out `6*(frame-0x5a)`
/// as brightness fades `200 - ((frame-0x5a)*0x7f)/0x1e`. The emit itself and the
/// cue-fire latch (`DAT_801d5134`) are the host's; this returns the envelope.
pub fn dance_countin_banner_envelope(frame: i32) -> CountInBanner {
    let (mut x_offset, mut brightness, hold);
    if frame < 0x1e {
        x_offset = 0xb4 - 6 * frame;
        brightness = 0x80;
        hold = false;
    } else {
        x_offset = 0;
        brightness = ((frame - 0x1e) * 0x7f) / 0x1e + 0x80;
        hold = true;
    }
    if frame > 0x59 {
        x_offset = 6 * (frame - 0x5a);
        brightness = 200 - ((frame - 0x5a) * 0x7f) / 0x1e;
        // The slide-out overrides the hold path back to two sliding halves.
        return CountInBanner {
            x_offset,
            brightness: brightness.clamp(0, 0xff) / 2,
            hold: false,
        };
    }
    brightness = brightness.clamp(0, 0xff);
    if !hold {
        brightness /= 2;
    }
    CountInBanner {
        x_offset,
        brightness,
        hold,
    }
}

// REF: FUN_800204f8 (the shared clip driver this gate decides to call; the
// driver itself is the move-VM consumer ported in `legaia-engine-vm`)
// Wired: [`DanceGame::dancer_clip_frames`] runs this per floor slot every frame
// off the dancer actor pool, and the pool is populated by
// [`DanceGame::from_overlay_for_mode`] from the disc's own spawn + kind tables.
//
// CORRECTION to the reason this row previously carried. The old tag read
// `+0x5C` as "the groovy-move turns left" and concluded the spin arm was
// already satisfied. It is not that slot. `FUN_801d0190` stores
// `kind_desc[0x10] & 0x1FF` - the **idle clip's anim id** - into `+0x5C` at
// spawn, and `FUN_801d1358` rewrites it with each judge-returned move clip
// (`sh v0,0x5c(s0)` at `801d1544` / `801d1584` / `801d16d4`, each preceded by
// `andi ...,0x1ff`); the groovy-move turn counter is the overlay global
// `DAT_801d564c[i]`, decremented at `801d1454` and not an actor field at all.
// So the first arm is "this actor has a clip bound", and satisfying it meant
// binding clips - which is what the dancer record now does.
/// PORT: FUN_801d4098 - the per-dancer actor clip-driver gate. Retail hands the
/// dancer to the shared clip driver `FUN_800204f8` only when its bound clip id
/// (`+0x5C`) is positive **or** its flag word (`+0x10`) carries bit `0x1000`.
/// That predicate is the whole function; the driver call is the animation
/// host's. `clip_id` is the signed `+0x5C` halfword, `flags` the actor flag
/// word.
pub fn dance_clip_driver_gate(clip_id: i16, flags: u32) -> bool {
    clip_id > 0 || (flags & crate::minigame_actor::FLAG_DRIVE_CLIP) != 0
}

// Wired: the dance entry's five face-stamp calls resolve their dancer slots
// through it (`crate::dance_venue::entry_face_stamps`, applied to the venue
// VRAM by `DanceVenue::build` on the native window and both browser pages).
// The per-frame face draw on the browser page still resolves its rig from the
// disc cast table's per-dancer kind (`castRigs()` in
// `site/js/minigame-dance.js`); on the qualifier floor those kinds are already
// `0/2/3`, the exact output of this remap, so the two agree.
/// PORT: FUN_801d03c4 - the dancer face-stamp's rig selector. The face blit picks
/// a per-dancer VRAM strip + eye/mouth frame table by rig index; in the qualifier
/// (mode 0) the overlay remaps dancer `2 -> 3` and `1 -> 2`, so the rig id equals
/// the dancer's kind (the qualifier cast is kinds `0/2/3`). Dancers past `3` are
/// not stamped (retail early-returns); a rig `>= 5` has no jump-table case.
/// Returns the rig index; the pose-unchanged latch (`DAT_801d56cc`) and the two
/// `MoveImage` blits are the render host's.
pub fn dance_face_rig(mode: DanceMode, dancer: usize) -> Option<usize> {
    if dancer >= 4 {
        return None;
    }
    let rig = if mode == DanceMode::Qualifier {
        match dancer {
            2 => 3,
            1 => 2,
            d => d,
        }
    } else {
        dancer
    };
    (rig < 5).then_some(rig)
}

/// The dance venue's own PROT block base, the raw-TOC index the overlay's
/// init writes into `_DAT_80084540` (`0x801CF100`). It is the block the dance
/// scene lives in - see [`legaia_asset::dance_cast`], whose
/// `DANCE_SCENE_NAME` (`other7`) is the venue's real scene name.
///
/// There is no dance-side scene-name literal. `0x801D518C` - once read here
/// as holding `other1` - is **BSS** in the static PROT 0980 image (all zeros
/// at file `0x6974`); it is where the overlay's init `FUN_801CEF54` *saves*
/// the caller's scene name (`0x801CF0B0`) so the teardown can put it back.
/// The only image in the corpus carrying the literal `other1` is the
/// **fishing** overlay, whose venue that scene is.
pub const DANCE_SCENE_BLOCK_BASE: u16 = 0x4CC;

/// What the dance overlay's **entry** stages, in the order retail stores it.
///
/// The counterpart of [`DanceSceneStage`]: `FUN_801CEF54` is the mode-24
/// sub-id-6 initialiser (arm 6 of the SCUS switch `FUN_80025980`, `jal
/// 0x801cef54` at `0x80025AE0`), and every field below is one of its stores or
/// one of its fixed-argument calls. Straight-line apart from one branch: the
/// by-name asset load is taken only while the dev/retail loader flag
/// `_DAT_8007B8C2` is clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceSceneEntry {
    /// Display width handed to `FUN_8001DAF8` (`0x140` = 320), then the
    /// ordering-table depth handed to the boot mode-init `FUN_8001DCF8`.
    pub screen_width: u16,
    /// `FUN_8001DCF8`'s argument. `0x0C` here against the duel's `0x0C` and
    /// the fishing bring-up's `0x0B` - it is a per-mode depth, not a constant.
    pub ot_depth: u8,
    /// Scene block base written to `_DAT_80084540`, which the SCUS BGM
    /// resolver indexes as `*(0x80084540) + 6 + bgm_id`. Same value the
    /// teardown restores - see [`DANCE_SCENE_BLOCK_BASE`].
    pub scene_block_base: u16,
    /// Bytes the game-mode work buffer is allocated at through the malloc
    /// wrapper `FUN_80017888`, then parked at the scratchpad scene pointer
    /// `0x1F8003EC`.
    pub work_buffer_bytes: u32,
    /// Bytes the GPU primitive-packet buffer is allocated at
    /// (`FUN_8001E3B8`).
    pub prim_buffer_bytes: u32,
    /// The spawned actor's position, `(+0x14, +0x16, +0x18)` of the actor
    /// `FUN_80020DE0` materialises from the template at `0x801D42E4`
    /// (`0x801CF23C..0x801CF250`). That actor is the **beat clock**, not a
    /// drawn dancer - its tick word is `FUN_801cf470` - and the position is
    /// the dance camera's anchor: the tick writes `-(+0x14)` and `-(+0x18)`
    /// into the focus trio `0x80089118` / `0x80089120` every frame
    /// (`0x801CFF84..0x801CFFA4`). The middle component is negative: the
    /// floor is above the origin.
    pub dancer_spawn: (i16, i16, i16),
    /// `+0x4` / `+0x8` of the camera-target block at `0x800840B8`
    /// (`0x801CF2B4..0x801CF2C8`, `+0x0` cleared) - the eye-space
    /// translation's `Y` and eye-back depth, the same pair the field-camera
    /// reset writes on field entry.
    pub camera_pair: (u32, u32),
    /// The camera angle triple at `0x8007B790` (pitch / yaw / roll, 12-bit),
    /// written `0x3C, 0, 0` at `0x801CF29C..0x801CF2AC`.
    pub camera_angles: (u16, u16, u16),
    /// GTE `H` (`_DAT_8007B6F4`), written `0x200` at `0x801CF294`.
    pub gte_h: u16,
    /// Scratchpad bytes `0x1F8003E8..EB` - the camera's visible tile window,
    /// as `(min_x, min_z, max_x, max_z)` signed tiles. **Symmetric** about the
    /// camera on both axes, where the field's default
    /// ([`crate::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW`]) is offset
    /// forward and to the left; the dance floor is also two tiles wider and
    /// four deeper.
    pub view_window: (i8, i8, i8, i8),
    /// How many per-dancer slots the entry clears, one word each in the four
    /// parallel arrays at `0x801D544C` / `0x801D53CC` / `0x801D578C` /
    /// `0x801D57CC`. Three - the qualifier floor's size, which is the floor
    /// the overlay's own init stages regardless of which mode runs later.
    pub cleared_dancer_slots: usize,
    /// Arguments of the five `FUN_801D03C4` face-stamp calls, in order, as
    /// `(dancer, pose)`: `a0` is the dancer slot and `a1` the pose, the
    /// value the selector compares against its per-dancer latch at
    /// `0x801D56CC` and then indexes the rig's frame table with.
    pub face_stamps: [(u8, u8); 5],
    /// The mode global `DAT_801D514C` in force at each face-stamp call. The
    /// entry raises it to `1` (finals: no slot remap) at `0x801CF35C` for the
    /// first three calls and drops it to `0` (qualifier: `1 -> 2`, `2 -> 3`)
    /// at `0x801CF398` for the last two, clearing the three-word pose latch
    /// before each batch (`0x801CF360` / `0x801CF3B0` loops) - so the five
    /// calls preload rigs `0`, `1`, `2`, `2`, `3`.
    pub face_stamp_mode: [u32; 5],
    /// Streaming asset ids the entry loads, both **raw TOC** indices: the
    /// venue's field file (`FUN_8001F7C0`, also [`Self::scene_block_base`])
    /// and the audio bank (`FUN_8001FC00` then `FUN_8001E54C`) - raw `0x4D1`
    /// is extraction `1231`, the dance SFX VAB, and the first entry past the
    /// venue's scene data.
    pub stream_ids: (u32, u32),
}

/// The dance overlay's entry constants.
///
/// PORT: FUN_801CEF54 (`0x801cef54..0x801cf46c`)
///
/// WIRED on all three hosts, in two halves. The **mode** half is
/// [`crate::world::World::enter_dance`]: the actor this entry spawns from the
/// template at `0x801D42E4` is the beat clock (`FUN_801cf470` is that
/// template's tick word), which is [`DanceGame`] + [`CountIn`] there. The
/// **venue** half is [`crate::dance_venue`]: `sync_dance_venue` stages the
/// record's globals over the walked-in scene on the first dance frame -
/// [`DanceSceneEntry::scene_block_base`] into the `_DAT_80084540` mirror,
/// [`DanceSceneEntry::view_window`] into the camera's visible-tile window, and
/// the camera [`crate::dance_venue::venue_camera`] builds from
/// [`DanceSceneEntry::dancer_spawn`] / [`DanceSceneEntry::camera_pair`] /
/// [`DanceSceneEntry::camera_angles`] / [`DanceSceneEntry::gte_h`], which the
/// frame resolver's `FieldCameraFrame::Venue` arm frames through - and
/// restores them on the first frame after it. `DanceVenue::build` loads the
/// block [`DanceSceneEntry::stream_ids`] names and applies the five
/// [`DanceSceneEntry::face_stamps`] to its VRAM. The native play-window runs
/// both once a frame and draws the venue in place of the walked-in scene; the
/// browser play page runs the same sync, and both browser pages build their
/// hall through the same `DanceVenue::build` and frame it through the same
/// camera. The walked-in scene is never unloaded, so the teardown's scene-name
/// restore is structural. [`DanceSceneEntry::screen_width`],
/// [`DanceSceneEntry::ot_depth`] and the two buffer sizes size libgpu display
/// and heap state the renderer replaces.
pub const fn dance_scene_entry() -> DanceSceneEntry {
    DanceSceneEntry {
        screen_width: 0x140,
        ot_depth: 0x0c,
        scene_block_base: DANCE_SCENE_BLOCK_BASE,
        work_buffer_bytes: 0x1_4000,
        prim_buffer_bytes: 0x1_9000,
        dancer_spawn: (0x1800, -0x64, 0x3300),
        camera_pair: (0x62c, 0xff0),
        camera_angles: (0x3c, 0, 0),
        gte_h: 0x200,
        view_window: (-8, -0x0a, 8, 0x0a),
        cleared_dancer_slots: 3,
        face_stamps: [(0, 1), (1, 0), (2, 0), (1, 0), (2, 0)],
        face_stamp_mode: [1, 1, 1, 0, 0],
        stream_ids: (0x4cc, 0x4d1),
    }
}

/// What the dance scene teardown writes, in the order retail stores it. It is
/// a **restore**, not a stage: the overlay's init `FUN_801CEF54` saved each of
/// these on the way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceSceneStage {
    /// The scene-name buffer at `0x80084548` is refilled from the overlay's
    /// save slot `0x801D518C` (`0x801D416C`) - the field scene the player
    /// walked in from, whatever it was. Not a literal.
    pub restores_caller_scene: bool,
    /// `_DAT_8007B880` is zeroed - the pad latch the field subsystem reads,
    /// so the frame the dance enters or leaves on cannot carry a stale press
    /// into the next mode.
    pub clear_pad_latch: bool,
    /// `_DAT_80084540` is restored from the overlay's `DAT_801D5180`
    /// (`0x801D4184`). That word is the scene's **PROT block base index**
    /// (the entry wrote [`DANCE_SCENE_BLOCK_BASE`] into it), which is what
    /// the SCUS BGM resolver indexes as `*(0x80084540) + 6 + bgm_id` at
    /// `0x8002443C` - not a "scene kind" the loader dispatches on.
    pub restores_scene_block_base: bool,
    /// `_DAT_8007BA9C = -1` after the scene-setup helper returns
    /// (`0x801D4198`). This is the **BGM swap's force-reload latch**, not a
    /// dance-local arm: `FUN_800243F0` loads it at `0x8002457C`, compares it
    /// against `_DAT_8007BAB8` and skips the whole seven-stage swap machine
    /// when the two are equal, so writing an impossible value is the standard
    /// "reload the track" idiom. The consumer runs on the *next* image in
    /// slot A.
    pub bgm_force_reload: i32,
}

// PARTIALLY WIRED: `World::enter_dance` / `World::exit_dance` apply the record's
// `clear_pad_latch` through `InputState::clear_edges`, and
// `crate::dance_venue::sync_dance_venue` runs the block-base restore
// (`restores_scene_block_base`) on the first frame after the dance, together
// with the view-window restore. `restores_caller_scene` needs no write: the
// port never unloads the walked-in scene, so there is no name to copy back.
// `bgm_force_reload` has no consumer - `World::restore_minigame_bgm` re-queues
// the hall track directly instead of arming the swap machine's reload latch.
/// PORT: FUN_801d414c - the dance teardown, the inverse of the overlay's own
/// init `FUN_801CEF54`.
///
/// Copies the **saved caller scene name** back out of the overlay's slot
/// `0x801D518C` into the scene-name buffer at `0x80084548` through the string
/// copy `FUN_80056758` (`0x801D416C`), clears the pad latch `_DAT_8007B880`
/// (`0x801D417C`), restores `_DAT_80084540` from `DAT_801D5180`
/// (`0x801D4184`), calls the scene-setup helper `FUN_80026018`, and only
/// **then** writes `_DAT_8007BA9C = -1` (`0x801D4198`). The ordering matters:
/// the write is after the setup call, so a setup that re-enters cannot see it.
///
/// Called once from the dance tick `FUN_801CF470`.
pub const fn dance_scene_stage() -> DanceSceneStage {
    DanceSceneStage {
        restores_caller_scene: true,
        clear_pad_latch: true,
        restores_scene_block_base: true,
        bgm_force_reload: -1,
    }
}

/// Fade weight the sprite emit derives from the part's `+0x78` halfword,
/// clamped to `0 ..= 0xFF`.
///
/// Retail reads the field as a **halfword** and compares it against `0x4000`
/// as a *signed 32-bit* value, so the "above the window" test can never see a
/// negative: a value past `0x4000` collapses the weight to zero outright
/// rather than saturating it.
// PORT: FUN_801d387c (the fade-weight prologue)
// Wired: [`DanceGame::sprite_part_emits`] applies it to every live sprite
// part every frame, over the pool [`DanceGame::advance`] ages.
//
// What is still not retail-pinned is the *producer* of `+0x78`: no caller of
// `FUN_801d387c` exists in the dump corpus (its address sits as a callback
// word, the same shape as the duel's afterimage pass), so nothing shows
// which quantity the dance overlay parks there. The port drives it as the
// part's age on the prologue's own
// [`crate::minigame_actor::BEAT_FADE_CEILING`] ramp - a port decision, stated
// as one, not a reading of a store.
pub fn sprite_part_fade_weight(beat: u16) -> u8 {
    if beat as i32 > 0x4000 {
        return 0;
    }
    ((beat as i32) >> 4).min(0xFF) as u8
}

/// The emit dispatch's draw mode for a spawned sprite part.
///
/// **Not pinned to retail.** `FUN_801d387c` takes its mode from its caller and
/// no caller of that address is in the dump corpus - the address sits as an
/// actor-prototype callback word, the same shape as the duel's afterimage
/// pass. Mode `2` is the shadowed arm, the two-emit draw that applies the
/// `>> 3` inverse of the spawn's `<< 3`; the port uses it and says so rather
/// than implying a disassembly reading.
pub const PART_DRAW_MODE: u32 = 2;

/// `+0x78` units a sprite part sheds per frame.
///
/// A **port decision**, not a retail constant: nothing in the dump corpus
/// writes `+0x78` for this actor family, so the engine spawns a part at
/// [`crate::minigame_actor::BEAT_FADE_CEILING`] and decays it down the fade
/// prologue's own ramp. At this step a part reaches zero after 64 frames, the
/// same order as the play window's placeholder part lifetime.
pub const PART_AGE_STEP: u32 = 0x100;

/// One dancer's resolved per-frame clip work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DancerClipFrame {
    /// Floor slot (`0` = the human).
    pub slot: usize,
    /// The actor's bound clip id (`+0x5C`), for a host that wants to play it.
    pub clip_id: i16,
    /// The bound clip's cursor step (`+0x6A`).
    pub clip_rate: u16,
    /// What [`dance_clip_driver_gate`] resolved from `+0x5C` / `+0x10`: the
    /// shared clip driver runs for this actor this frame.
    pub clip_driver: bool,
    /// The bound clip indexes the party clip bank (anim word bit `0x200`,
    /// actor flag `0x01000000` - see
    /// [`crate::minigame_actor::FLAG_PARTY_CLIP_BANK`]).
    pub party_bank: bool,
}

/// One sprite part's resolved per-frame draw work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpritePartFrame {
    /// Index into [`DanceGame::sprite_parts`].
    pub index: usize,
    /// What [`sprite_part_emit`] resolved for the part's draw mode.
    pub emit: SpritePartEmit,
    /// What [`sprite_part_fade_weight`] resolved from the part's `+0x78`.
    pub fade: u8,
    /// The part's `+0x50` sprite id (the spawn's third argument).
    pub sprite: u16,
}

/// Which emit the dancer sprite dispatch performs for a draw mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpritePartEmit {
    /// Mode `0` - no emit at all: copy the transform template's `+0x90` trio
    /// into the dancer and zero its `+0x96` / `+0x98` / `+0x9A`.
    CopyTemplate,
    /// Mode `1` - store the caller's third argument into the dancer's
    /// `+0x94`; still no emit.
    SetTemplateZ,
    /// Mode `2` - the shadowed draw: **two** emits at the dancer's screen
    /// position (its `+0x14` / `+0x16` pair rounded toward zero and shifted
    /// `>> 3`), the first with semi-transparency flag `0x400` and the second
    /// with `0x800`.
    Shadowed { x: i16, y: i16, flags: [u16; 2] },
    /// Mode `3` - one plain emit at the **unrounded** `+0x14` / `+0x16` pair.
    /// Note this mode does not divide by eight at all.
    Plain { x: i16, y: i16, flags: u16 },
    /// Mode `4` - the marker draw: one emit with flags forced to `1` and the
    /// scale word forced to `0x1000`, after stamping `sprite << 4` into the
    /// overlay byte `DAT_801D46E8`.
    Marker { x: i16, y: i16, clut_byte: u8 },
    /// A mode past the five-entry jump table - nothing is drawn.
    None,
}

// Wired: [`DanceGame::sprite_part_emits`] calls this once per live sprite part
// every frame, over the pool [`DanceGame::spawn_sprite_part`] fills and
// [`DanceGame::advance`] ages. The prerequisite the old tag named - "a
// minigame actor record" carrying the `+0x14/+0x16` pair, the `+0x50` sprite
// word and the `+0x78` field - is
// [`crate::minigame_actor::MinigameActor`].
//
// CORRECTION, and it is what decides where the wire belongs. The old tag (and
// this function's old name, `dancer_emit`) read this as the **dancer's** draw.
// It is not: the actor family it reads is the one `FUN_801d3fd0` spawns.
// That spawner stamps `+0x50 = sprite_id` and stores `x << 3` / `y << 3` into
// `+0x14` / `+0x16` (`801d401c`..`801d4024`), and this dispatch reads exactly
// those three slots and shifts the pair back down by three - an exact inverse,
// on a spawner whose only two callers in the port are
// [`step_mark_effect_spawn`] and [`good_banner_spawn`]. The dancer bodies
// `FUN_801d0190` spawns carry a *world* triple in `+0x14`..`+0x18` and no
// `+0x50` at all, so a `>> 3` of a dancer's position lands hundreds of pixels
// off-screen.
//
// The old tag's second prerequisite, "a quad sink", was **wrong about the
// engine**: `legaia_engine_ui::screen_prim` has carried a PSX screen-space
// quad (`ScreenQuad` / `FlatQuad`, per-vertex gouraud, CLUT + texpage, ABR
// mode, ordering-table bucket) with both hosts consuming its `build_geometry`
// output since the battle-intro work. What genuinely has no sink is the
// *texel source*: no dance sprite page is resident in engine VRAM, which is
// why both hosts still degrade the emitted quads to a placeholder rather than
// sampling the overlay's page.
/// PORT: FUN_801d387c - the sprite-part / shadow emit dispatch.
///
/// `mode` selects one of five arms through a jump table; `x` / `y` are the
/// part's `+0x14` / `+0x16` pair and `sprite` its `+0x50` word. The two
/// emitting arms round toward zero **before** the `>> 3` (retail's
/// `bgez / addiu 7 / sra 3`), so a part at `-1` maps to screen `0` and not
/// `-1`.
pub fn sprite_part_emit(mode: u32, x: i16, y: i16, sprite: u16) -> SpritePartEmit {
    let scaled = |v: i16| -> i16 {
        let v = v as i32;
        ((if v < 0 { v + 7 } else { v }) >> 3) as i16
    };
    match mode {
        0 => SpritePartEmit::CopyTemplate,
        1 => SpritePartEmit::SetTemplateZ,
        2 => SpritePartEmit::Shadowed {
            x: scaled(x),
            y: scaled(y),
            flags: [sprite | 0x400, sprite | 0x800],
        },
        3 => SpritePartEmit::Plain {
            x,
            y,
            flags: sprite,
        },
        4 => SpritePartEmit::Marker {
            x,
            y,
            clut_byte: (sprite << 4) as u8,
        },
        _ => SpritePartEmit::None,
    }
}
