//! The two **boss-stage modules** the battle side-band pages in for the Cort
//! fight (formation monster `0xB5`): PROT 0968, the **arrival**, and PROT 0969,
//! the **form transition**. Both load at slot-B base `0x801F69D8` and both are
//! one phase machine on the battle context's `ctx[+0x289]` byte driving the
//! first monster seat (actor-table slot `3`).
//!
//! PORT: FUN_801F69F4, FUN_801F69D8
//!
//! Both addresses are shared with the cast-module band that pages into the
//! same slot-B window (`docs/subsystems/cast-module.md`); the two ported here
//! are the entry-968 and entry-969 bodies, which no PROT 0898 dispatch table
//! names - the side-band tick `FUN_80056208` is their only caller
//! ([`crate::battle_sideband`]).
//!
//! These are pure transition functions over a [`ArrivalView`] /
//! [`FormTransitionView`] of the retail state they touch, returning the calls
//! they make into the rest of the game as [`StageEffect`]s. The host is
//! `World::tick_battle_sideband` (`world/battle/sideband.rs`).
//!
//! The two writers that select these stages live here too:
//! [`battle_init_stage_override`] (stage `2`, battle init) and
//! [`boss_transition_stage_id`] (stage `3`, the Final Heal sweep's tail).
//!
//! # What is ported and what is not
//!
//! Ported: every phase transition and countdown, the camera register walks
//! and the `FUN_801D829C` framings (all of them pass a duration of `1`, so
//! each is a snap), the seat staging (position, facing, tint blend, render
//! flag and colour, anim rate, HP), the party re-seating, the HUD latches
//! `ctx[+0x243]` / `ctx[+0x278]`, the cue raises, the fade templates, the boss
//! name banner, the hand-back to flow state `0x0B` and the battle exit.
//!
//! Not staged: the in-image spawn records `FUN_80050ED4` is handed (every one
//! of them opens `model_sel = -1`, a meshless part), the two SCUS move-VM
//! effect trees `FUN_80021B04` runs at the hand-back, the render-node words
//! (`record[+0x42]`, `+0x72`, `+0x78`) and the CD-XA stop `FUN_8003ED04(0)`.
//! The hand-back's backdrop rebind (`ctx[+0x106C]` / `ctx[+0x1070]` slot 0
//! := slot 1) and its `FUN_80058490` rect push are reported as
//! [`StageEffect::RebindBackdrop`] / [`StageEffect::MoveImage`], which the
//! world turns into render state both hosts read. The spawns are still reported as [`StageEffect::Spawn`]
//! and the form transition still draws its battle-RNG values, so the RNG
//! stream matches retail's.
//!
//! Sources: `ghidra/scripts/funcs/overlay_battle_slot_b_0968_0968_801f69f4.txt`
//! and `ghidra/scripts/funcs/overlay_battle_slot_b_0969_0969_801f69d8.txt`
//! (disassembly); the arrival's seven-word phase table is the head of its own
//! image at `0x801F69D8`.
//!
//! REF: FUN_801D829C (the camera framing, duration `1`), FUN_8004FCC8 (cue),
//! FUN_80024E80 (fade spawn), FUN_80050ED4 (spawn), FUN_80035F04 /
//! FUN_8003541C (the banner), FUN_80056798 (battle RNG)

use crate::fade::FadeTemplate;

/// Cue the arrival raises when the camera reaches its pull-back
/// (`li a0,0x20a` at `0x801F6AC8`).
pub const ARRIVAL_CUE: u16 = 0x20A;
/// Cue the form transition raises on entry (`li a0,0x20b` at `0x801F6A74`).
pub const FORM_TRANSITION_CUE: u16 = 0x20B;

/// Camera `TR.y` (`0x800840BC`) the arrival's phase 0 walks up to.
pub const ARRIVAL_PULLBACK_TR_Y: i32 = 0xC00;
/// The value the arrival re-seeds `ctx[+0x6D6]` to on every tick.
pub const ARRIVAL_RAMP_DELAY: i16 = 0x100;
/// Flow state the arrival hands back to (`li v0,0xb` at `0x801F7138`): the
/// round-opening timer state, which then opens round one.
pub const ARRIVAL_HANDBACK_FLOW: u8 = 0x0B;
/// Pen `y` of the boss-name banner (`li v1,0x96` at `0x801F7074`).
pub const ARRIVAL_BANNER_Y: i32 = 0x96;
/// Screen centre the banner is measured about (`li a3,0xa0`).
pub const ARRIVAL_BANNER_CENTRE_X: i32 = 0xA0;
/// Seat height the arrival drops the boss in from (`li v0,0x600`).
pub const ARRIVAL_DROP_HEIGHT: i16 = 0x600;

/// Action state the form transition parks the SM in (`li v0,0xfc`).
pub const FORM_TRANSITION_ACTION_STATE: u8 = 0xFC;
/// Colour word the form transition stamps on the boss seat.
pub const FORM_TRANSITION_SEAT_COLOUR: u32 = 0x2008_0200;
/// Party seat `z` the form transition re-seats the two onlookers on.
pub const FORM_TRANSITION_PARTY_Z: i16 = -0x307;
/// Seat `z` the acting party member is moved to.
pub const FORM_TRANSITION_ACTOR_Z: i16 = -0x339;
/// The two alternating `TR.y` values of the transition shake.
pub const FORM_TRANSITION_SHAKE: (i32, i32) = (0x780, 0x800);

/// The arrival hand-back's `MoveImage` (`0x801F719C..0x801F71CC`): the
/// empty `16 x 64` strip at `(0x340, 0xC0)` copied onto `(0x370, 0xC0)` -
/// the ground grid's `(192..=255)^2` tile window on texture page 13
/// ([`legaia_asset::battle_backdrop::GROUND_TSB`]). The strip is all index
/// `0`, a transparent texel, so from the hand-back on the procedural floor
/// draws nothing and the flesh shell is the only ground.
pub const ARRIVAL_GROUND_BLANK: StageEffect = StageEffect::MoveImage {
    x: 0x340,
    y: 0xC0,
    w: 0x10,
    h: 0x40,
    dst_x: 0x370,
    dst_y: 0xC0,
};

/// The in-image spawn records each module hands `FUN_80050ED4`, by VA.
pub mod records {
    /// Arrival phase 0 (`0x801F6AE8`, `0x801F6B0C`, `0x801F6B24`).
    pub const ARRIVAL_ENTRY: [u32; 3] = [0x801F_71F0, 0x801F_7240, 0x801F_7290];
    /// Arrival phase 1 (`0x801F6BD0`, `0x801F6BE8`, `0x801F6C00`).
    pub const ARRIVAL_DROP: [u32; 3] = [0x801F_72D0, 0x801F_7320, 0x801F_7388];
    /// Arrival phases 2 / 3, every eighth frame.
    pub const ARRIVAL_TRAIL: u32 = 0x801F_73A4;
    /// Form transition phase 0.
    pub const TRANSITION_ENTRY: [u32; 2] = [0x801F_705C, 0x801F_709C];
    /// Form transition phase 1, every eighth frame.
    pub const TRANSITION_BURST: u32 = 0x801F_7024;
}

/// Stage id the **battle-init** override selects for a formation, or `None`
/// when the initializer leaves the byte alone.
///
/// `FUN_80055B6C` compares the formation cell's first monster id
/// (`_DAT_8007BD0C`) against
/// [`crate::encounter_record::BOSS_TRANSITION_MONSTER_ID`] and writes stage
/// id `2` (extraction entry 968) on a match - `0x80055D2C..0x80055D44`:
/// `lbu v1,-0x42f4(v1); li v0,0xb5; bne v1,v0; li v0,0x2;
/// sb v0,-0x49b6(at)`. No other condition: the override is a property of the
/// formation alone, applied while the phase-1 monster is still alive.
///
/// Tagged `REF`, not `PORT`: this mirrors one arm of the battle-scene
/// initializer, whose body (pool clears, party-slot composition, arena
/// allocation, disp/draw setup) is ported piecemeal elsewhere - a `PORT` tag
/// here would mark the whole routine ported on the strength of five
/// instructions.
///
/// REF: FUN_80055B6C (the `0xB5 -> 2` stage-override arm at `0x80055D2C`)
pub fn battle_init_stage_override(formation_slot0_monster_id: u8) -> Option<u8> {
    (formation_slot0_monster_id == crate::encounter_record::BOSS_TRANSITION_MONSTER_ID).then_some(2)
}

/// Stage id the **mid-battle** boss-transition writer selects, or `None`
/// while its guard holds off. This is the second `_DAT_8007B64A` writer for
/// the same monster id, resident in the battle overlay (PROT 0898), not in
/// `SCUS_942.54` - which is why the SCUS-only census sees three sites and
/// misses it.
///
/// The writer is the **tail arm of the Lost Grail "Final Heal" sweep**
/// `FUN_801E6968` (whose revive body is ported as
/// `World::apply_final_heal_revives`), run at the head of cleanup state `0x50` of
/// the battle SM `FUN_801E295C` (`jal 0x801E6968` at `0x801E5C6C`). The arm (`0x801E6CE4..0x801E6D64`,
/// `overlay_battle_action_801e6968.txt`) fires when **both** hold:
///
/// * `actor_table[3]` - the first monster seat - has HP `+0x14C == 0`
///   (`lw v0,0xc(s0); lhu v0,0x14c(v0); bne v0,zero,skip` at
///   `0x801E6CEC..0x801E6D00`), i.e. the phase-1 form is dead;
/// * the formation cell `_DAT_8007BD0C` still reads `0xB5`
///   (`lbu v1,-0x42f4(v0); li v0,0xb5; bne v1,v0,skip` at `0x801E6D04..`).
///
/// It then issues the loader-B call itself (`jal 0x8003EC70` at
/// `0x801E6D14` with `a0 = 0x4A` `= 3 + 0x47`, paging extraction entry 969
/// immediately rather than waiting for the dispatch reader), writes stage id
/// `3` (`sb v0,-0x49b6(a0)` at `0x801E6D2C`), **increments** `ctx[+0x26]`,
/// forces the flow-state byte `ctx[+0x7] = 0xFD`, and zeroes the dead seat's
/// `+0x21C` / `+0x225` bytes.
///
/// `ctx[+0x26]` is **not** a phase counter, which is how this arm's increment
/// used to read it. Its three readers all sit in the action machine's Done
/// band and the last of them, `0x801E61B4`, passes the byte as the element-id
/// argument of `FUN_801D8DE8(id, 1)` alongside sibling unloads that pass
/// literal ids - so it is a UI element id, and `0x65` (the "magic level
/// increased" banner, stored at `0x801E723C`) is the only value anything
/// assigns. See `docs/subsystems/battle-action.md`. What this increment is
/// *for* is unsettled; to every reader it only reads as non-zero.
///
/// Monster id `0xB5` is **Cort** (archive id 181; the spell-id collision
/// with Lapis Wave is settled in `docs/reference/re-settled-threads.md`), so
/// the evolved-Cort fight walks two stage modules: 968 from setup (the init
/// override above, phase 1 alive), then 969 - Cort's form-transition module,
/// an entry that doubles as the STR-path table - once the form dies. The
/// guard separating the two arms is the seat's liveness, nothing else.
///
/// A print-integrity note: this arm was first sighted at `0x801FD514` in a
/// base-tag-less `overlay_0897`-program dump. That coordinate is a phantom
/// printing (`+0x167E8` high); the store's byte pattern
/// (`24020003 a082b64a`) occurs in **no** PROT entry but 0898, at file
/// `0x18510` = VA `0x801E6D28` under the tagged base `0x801CE818` - and only
/// at the real base do the arm's `j 0x801E6***` exits land inside their own
/// function.
///
/// PORT: FUN_801E6968 (the boss-transition tail arm `0x801E6CE4..0x801E6D64`
/// only; the revive body is `World::apply_final_heal_revives`)
pub fn boss_transition_stage_id(
    formation_slot0_monster_id: u8,
    first_monster_seat_liveness: u16,
) -> Option<u8> {
    (formation_slot0_monster_id == crate::encounter_record::BOSS_TRANSITION_MONSTER_ID
        && first_monster_seat_liveness == 0)
        .then_some(3)
}

/// The camera globals the modules own while they run: the angle pair
/// `0x8007B790` / `0x8007B792`, the translation trio `0x800840B8..C0` and the
/// focus trio `0x80089118..20` (held un-negated, the way
/// `legaia_engine_vm::battle_cam_script::BattleCamPose` holds it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageCamera {
    pub pitch: i32,
    pub yaw: i32,
    pub tr: [i32; 3],
    pub focus: [i32; 3],
}

impl StageCamera {
    /// `FUN_801D829C(angles, tr, focus, 1)` - a one-frame tween, i.e. a snap.
    /// `TR.z` is handed over in world units and prescaled the way the builder
    /// prescales it (`(z << 8) / 0xA0`); the focus argument is the negated
    /// point, so the un-negated point is what lands here.
    pub fn frame(&mut self, pitch: i32, yaw: i32, tr: [i32; 3], focus: [i32; 3]) {
        self.pitch = pitch;
        self.yaw = yaw;
        self.tr = [tr[0], tr[1], (tr[2] << 8) / 0xA0];
        self.focus = focus;
    }
}

/// Something a module asks the rest of the game to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageEffect {
    /// `FUN_8004FCC8(id)` - a sound cue.
    Cue(u16),
    /// `FUN_80050ED4(pos, rot, record, 0x1000)` - one in-image spawn record.
    Spawn { record: u32 },
    /// `FUN_80024E80(&DAT_801C9070)` - a fade.
    Fade(FadeTemplate),
    /// `FUN_801D99BC` - the HUD handle list's hard reset.
    HudReset,
    /// The boss-name banner: `FUN_8003541C` of the first monster's name at
    /// `(0xA0 - width / 2, 0x96)`.
    Banner,
    /// The arrival's phase 6 rebind of both backdrop copies' object table:
    /// slot 1 over slot 0 (`0x801F7148..0x801F7180`), so a two-object stage
    /// shell draws its object 1 alone from here on
    /// ([`legaia_asset::battle_backdrop::drawn_object_indices_rebound`]).
    RebindBackdrop,
    /// `FUN_80058490` (`MoveImage`): VRAM rect `(x, y, w, h)` onto
    /// `(dst_x, dst_y)`.
    MoveImage {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        dst_x: u16,
        dst_y: u16,
    },
    /// The arrival's phase 6: stage id, `ctx[+0x289]` and `ctx[+0x6D6]`
    /// cleared, flow `ctx[+0x06] = 0x0B` - round one may open.
    HandBack,
    /// The form transition's last phase: mode word `2` (back to the field),
    /// the battle-won bit `DAT_8007BD60 |= 0x80`.
    ExitBattle,
}

/// Retail's fade block `DAT_801C9070`, halfword by halfword: kind, duration,
/// start RGB, end RGB, start delay, hold.
const fn fade(
    kind: i16,
    duration: i16,
    start: i16,
    end: i16,
    delay: i16,
    hold: i16,
) -> FadeTemplate {
    FadeTemplate {
        kind,
        duration,
        start_rgb: [start; 3],
        end_rgb: [end; 3],
        mode: [delay, hold, 0],
    }
}

/// The arrival's two fades: a white flash held open (`0x801F6B34..0x801F6B74`)
/// and its release (`0x801F6C10..0x801F6C4C`).
pub const ARRIVAL_FLASH: FadeTemplate = fade(1, 0x40, 0, 0xFF, 0x40, -1);
pub const ARRIVAL_RELEASE: FadeTemplate = fade(1, 0x80, 0xFF, 0, 0, 0);
/// The form transition's two fades (`0x801F6DA0..` / `0x801F6EFC..`).
pub const TRANSITION_FLASH: FadeTemplate = fade(1, 0x40, 0, 0xFF, 0, -1);
pub const TRANSITION_RELEASE: FadeTemplate = fade(1, 0x80, 0xFF, 0, 0, -1);

/// The arrival module's own data words (`0x801F73F8` countdown,
/// `0x801F73FC` frame counter).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArrivalState {
    pub countdown: i32,
    pub frames: u32,
}

/// The boss seat as the arrival reads and writes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArrivalSeat {
    /// `+0x34` / `+0x36` / `+0x38`.
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// `+0x46`.
    pub facing: u16,
    /// `+0x0C` - tint blend.
    pub blend: u32,
    /// `+0x21C`.
    pub render_flag: u8,
    /// `+0x21D`.
    pub anim_rate: u8,
}

/// Everything the arrival reads or writes outside its own image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArrivalView {
    /// `ctx[+0x289]`.
    pub phase: u8,
    /// `ctx[+0x6D6]`.
    pub ramp_delay: i16,
    /// `ctx[+0x243]` / `ctx[+0x278]`.
    pub ctx_243: u8,
    pub ctx_278: u8,
    /// `ctx[+0x06]`.
    pub flow: u8,
    /// `_DAT_8007B64A`.
    pub stage_id: u8,
    pub camera: StageCamera,
    pub seat: ArrivalSeat,
}

/// One arrival tick (`FUN_801F69F4`, PROT 0968). `step` is `DAT_1F800393`.
pub fn arrival_tick(st: &mut ArrivalState, v: &mut ArrivalView, step: u8) -> Vec<StageEffect> {
    use StageEffect::*;
    let mut out = Vec::new();
    let s = i32::from(step);
    v.ramp_delay = ARRIVAL_RAMP_DELAY;
    let seat_focus = |v: &ArrivalView| [i32::from(v.seat.x), 0, i32::from(v.seat.z)];
    // Phase 2 / 3's trail: one record every eighth frame (`andi a0,v1,0x7`).
    let trail = |st: &mut ArrivalState, out: &mut Vec<StageEffect>| {
        let f = st.frames;
        st.frames = st.frames.wrapping_add(1);
        if f & 7 == 0 {
            out.push(Spawn {
                record: records::ARRIVAL_TRAIL,
            });
        }
    };
    match v.phase {
        // 0x801F6A74: walk the camera out, then the cue, the flash and the
        // entry records.
        0 => {
            if v.camera.tr[1] < ARRIVAL_PULLBACK_TR_Y {
                v.camera.tr[1] += 4 * s;
                v.camera.tr[2] += 14 * s;
                return out;
            }
            out.push(Cue(ARRIVAL_CUE));
            v.phase = v.phase.wrapping_add(1);
            out.extend(records::ARRIVAL_ENTRY.map(|record| Spawn { record }));
            out.push(Fade(ARRIVAL_FLASH));
            st.countdown = 0x80;
            st.frames = 0;
        }
        // 0x801F6B98: wait, then drop the boss in from above under the
        // release fade and frame it from below.
        1 => {
            st.countdown -= s;
            if st.countdown > 0 {
                return out;
            }
            out.extend(records::ARRIVAL_DROP.map(|record| Spawn { record }));
            out.push(Fade(ARRIVAL_RELEASE));
            v.camera.frame(
                0x80,
                -i32::from(v.seat.facing),
                [0, 0x200, 0xC00],
                seat_focus(v),
            );
            v.seat.blend = 0x1000;
            v.seat.render_flag = 0;
            v.seat.y = ARRIVAL_DROP_HEIGHT;
            v.seat.anim_rate = 1;
            st.countdown += 0x100;
            v.phase = v.phase.wrapping_add(1);
        }
        // 0x801F6CD4: the descent, camera dollying back and tilting.
        2 => {
            trail(st, &mut out);
            v.camera.tr[2] += 12 * s;
            v.camera.pitch = (v.camera.pitch + s) & 0xFFFF;
            v.seat.y = v.seat.y.wrapping_sub((3 * s) as i16);
            st.countdown -= s;
            if st.countdown > 0 {
                return out;
            }
            v.seat.y = 0x300;
            v.camera
                .frame(0x60, -0x1B0, [0, 0x200, 0xAE8], seat_focus(v));
            st.countdown += 0x100;
            v.phase = v.phase.wrapping_add(1);
        }
        // 0x801F6E0C: the landing.
        3 => {
            trail(st, &mut out);
            v.camera.tr[1] += 4 * s;
            v.camera.pitch = (v.camera.pitch - s) & 0xFFF;
            v.seat.y = v.seat.y.wrapping_sub((3 * s) as i16);
            st.countdown -= s;
            if st.countdown > 0 {
                return out;
            }
            v.seat.y = 0;
            v.camera.frame(0, 0x770, [0, 0x2C0, 0x890], seat_focus(v));
            st.countdown += 0x100;
            v.phase = v.phase.wrapping_add(1);
        }
        // 0x801F6F3C: a slow orbit, then the HUD latches.
        4 => {
            v.camera.tr[2] += 8 * s;
            v.camera.yaw = (v.camera.yaw - s) & 0xFFF;
            st.countdown -= s;
            if st.countdown > 0 {
                return out;
            }
            v.camera.frame(0, 0, [0, 0x200, 0x800], seat_focus(v));
            st.countdown += 0x1E0;
            v.ctx_243 = 1;
            v.ctx_278 = 2;
            v.phase = v.phase.wrapping_add(1);
        }
        // 0x801F7004: pull back, then the name banner.
        5 => {
            v.camera.tr[2] += 2 * s;
            v.camera.tr[1] += 4 * s;
            st.countdown -= s;
            if st.countdown > 0 {
                return out;
            }
            out.push(Banner);
            st.countdown += 0xB4;
            v.ctx_278 = 3;
            v.phase = v.phase.wrapping_add(1);
        }
        // 0x801F70D8: hold the banner, then hand the battle back.
        6 => {
            st.countdown -= s;
            if st.countdown > 0 {
                return out;
            }
            v.ctx_243 = 0;
            v.ctx_278 = 0;
            v.stage_id = 0;
            v.ramp_delay = 0;
            v.phase = 0;
            v.seat.anim_rate = 8;
            v.flow = ARRIVAL_HANDBACK_FLOW;
            out.push(RebindBackdrop);
            out.push(ARRIVAL_GROUND_BLANK);
            out.push(HandBack);
        }
        // `sltiu a0,7` bounds the table; anything past it is a no-op.
        _ => {}
    }
    out
}

/// The form transition's own data words (`0x801F70DC` countdown,
/// `0x801F70E0` shake counter, `0x801F70E4` spawn counter).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormTransitionState {
    pub countdown: i32,
    pub shake: u32,
    pub spawn: u32,
}

/// One actor-table slot as the form transition writes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransitionSeat {
    /// `false` for an actor-table slot the engine has no actor in (a short
    /// party); writes to it are dropped.
    pub present: bool,
    /// `+0x34` / `+0x38`.
    pub x: i16,
    pub z: i16,
    /// `+0x46`.
    pub facing: u16,
    /// `+0x04` - the colour word.
    pub colour: u32,
    /// `+0x14C` - live HP.
    pub hp: u16,
    /// `+0x1DA` - queued anim.
    pub queued_anim: u8,
    /// `+0x1DC` - flag byte.
    pub flags: u8,
    /// `+0x1DD` - target slot.
    pub target: u8,
    /// `+0x21C` - render flag.
    pub render_flag: u8,
    /// `+0x225` - capture state.
    pub capture_state: u8,
}

/// Everything the form transition reads or writes outside its own image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormTransitionView {
    /// `ctx[+0x289]`.
    pub phase: u8,
    /// `ctx[+0x07]`.
    pub action_state: u8,
    /// `ctx[+0x13]` - the acting slot, retail numbering.
    pub acting: u8,
    /// `ctx[+0x243]` / `ctx[+0x278]`.
    pub ctx_243: u8,
    pub ctx_278: u8,
    pub camera: StageCamera,
    /// Actor-table slots `0..=3`: the three party seats and the boss.
    pub seats: [TransitionSeat; 4],
}

/// One form-transition tick (`FUN_801F69D8`, PROT 0969). `step` is
/// `DAT_1F800393`; `rand` is the battle RNG `FUN_80056798`, drawn exactly as
/// often as retail draws it.
pub fn form_transition_tick(
    st: &mut FormTransitionState,
    v: &mut FormTransitionView,
    step: u8,
    rand: &mut dyn FnMut() -> u16,
) -> Vec<StageEffect> {
    use StageEffect::*;
    let mut out = Vec::new();
    let s = i32::from(step);
    // Phases 1 / 2 alternate `TR.y` between two positions every frame.
    let shake = |st: &mut FormTransitionState, v: &mut FormTransitionView| {
        let k = st.shake;
        st.shake = st.shake.wrapping_add(1);
        v.camera.tr[1] = if k & 1 != 0 {
            FORM_TRANSITION_SHAKE.0
        } else {
            FORM_TRANSITION_SHAKE.1
        };
    };
    match v.phase {
        // 0x801F6A70: the killing blow does not kill.
        0 => {
            out.push(Cue(FORM_TRANSITION_CUE));
            out.push(HudReset);
            v.action_state = FORM_TRANSITION_ACTION_STATE;
            let boss = &mut v.seats[3];
            boss.render_flag = 0x0A;
            boss.z = 0x320;
            boss.facing = 0x800;
            boss.colour = FORM_TRANSITION_SEAT_COLOUR;
            boss.hp = 1;
            boss.queued_anim = 0;
            boss.capture_state = 0;
            boss.x = 0;
            boss.flags = boss.flags.wrapping_add(1);
            let acting = usize::from(v.acting);
            if let Some(a) = v.seats.get_mut(acting) {
                a.x = 0;
                a.z = FORM_TRANSITION_ACTOR_Z;
                a.facing = 0;
                a.target = 8;
            }
            let mut x: i16 = 0x12C;
            for (i, seat) in v.seats.iter_mut().enumerate().take(3) {
                if i == acting {
                    continue;
                }
                seat.x = x;
                seat.z = FORM_TRANSITION_PARTY_Z;
                x -= 0x258;
                seat.facing = 0;
            }
            v.camera.frame(0, 0, [0, 0x800, 0x1000], [0, 0, 0]);
            v.phase = v.phase.wrapping_add(1);
            st.countdown = 0x200;
            st.shake = 0;
            st.spawn = 0;
            out.extend(records::TRANSITION_ENTRY.map(|record| Spawn { record }));
        }
        // 0x801F6C70: shake, bursts every eighth frame, then the flash.
        1 => {
            shake(st, v);
            let k = st.spawn;
            st.spawn = st.spawn.wrapping_add(1);
            if k & 7 == 0 {
                // Offset draw, then the spawned node's speed and sign draws.
                rand();
                out.push(Spawn {
                    record: records::TRANSITION_BURST,
                });
                rand();
                rand();
            }
            st.countdown -= s;
            v.camera.tr[2] += 4 * s;
            if st.countdown > 0 {
                return out;
            }
            out.push(Fade(TRANSITION_FLASH));
            v.phase = v.phase.wrapping_add(1);
            st.countdown += 0x80;
        }
        // 0x801F6E1C: shake, then blank the field under the flash.
        2 => {
            shake(st, v);
            st.countdown -= s;
            v.camera.tr[2] += 4 * s;
            if st.countdown > 0 {
                return out;
            }
            for seat in &mut v.seats {
                seat.colour = 0;
                seat.render_flag = 0xFF;
            }
            v.ctx_243 = 1;
            v.ctx_278 = 3;
            out.push(Fade(TRANSITION_RELEASE));
            st.countdown += 0x80;
            v.phase = v.phase.wrapping_add(1);
        }
        // 0x801F6F6C: wait, then leave the battle.
        3 => {
            st.countdown -= s;
            v.camera.tr[2] += 4 * s;
            if st.countdown > 0 {
                return out;
            }
            v.phase = v.phase.wrapping_add(1);
            out.push(ExitBattle);
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_arrival(v: &mut ArrivalView, st: &mut ArrivalState, max: usize) -> Vec<StageEffect> {
        let mut all = Vec::new();
        for _ in 0..max {
            let e = arrival_tick(st, v, 1);
            let done = e.contains(&StageEffect::HandBack);
            all.extend(e);
            if done {
                break;
            }
        }
        all
    }

    #[test]
    fn the_arrival_walks_the_camera_to_the_pullback_before_anything_else() {
        let mut st = ArrivalState::default();
        let mut v = ArrivalView::default();
        v.camera.tr[1] = ARRIVAL_PULLBACK_TR_Y - 8;
        assert!(arrival_tick(&mut st, &mut v, 1).is_empty());
        assert_eq!(v.camera.tr, [0, ARRIVAL_PULLBACK_TR_Y - 4, 14]);
        assert_eq!(v.ramp_delay, ARRIVAL_RAMP_DELAY);
        arrival_tick(&mut st, &mut v, 1);
        // At the cap: the next tick raises the cue and moves on.
        let e = arrival_tick(&mut st, &mut v, 1);
        assert_eq!(e[0], StageEffect::Cue(ARRIVAL_CUE));
        assert!(e.contains(&StageEffect::Fade(ARRIVAL_FLASH)));
        assert_eq!(v.phase, 1);
        assert_eq!(st.countdown, 0x80);
    }

    #[test]
    fn the_arrival_drops_the_boss_in_and_lands_it_on_the_floor() {
        let mut st = ArrivalState::default();
        let mut v = ArrivalView {
            camera: StageCamera {
                tr: [0, ARRIVAL_PULLBACK_TR_Y, 0],
                ..Default::default()
            },
            seat: ArrivalSeat {
                z: 800,
                facing: 0x100,
                anim_rate: 8,
                ..Default::default()
            },
            ..Default::default()
        };
        arrival_tick(&mut st, &mut v, 1); // phase 0 -> 1
        for _ in 0..0x80 {
            arrival_tick(&mut st, &mut v, 1);
        }
        assert_eq!(v.phase, 2);
        assert_eq!(v.seat.y, ARRIVAL_DROP_HEIGHT);
        assert_eq!(v.seat.anim_rate, 1);
        assert_eq!(v.camera.pitch, 0x80);
        assert_eq!(v.camera.yaw, -0x100);
        assert_eq!(v.camera.focus, [0, 0, 800]);
        assert_eq!(v.camera.tr, [0, 0x200, (0xC00 << 8) / 0xA0]);
        let e = run_arrival(&mut v, &mut st, 0x1000);
        assert!(e.contains(&StageEffect::Banner));
        assert_eq!(e.last(), Some(&StageEffect::HandBack));
        assert_eq!(v.seat.y, 0, "landed");
        assert_eq!(v.seat.anim_rate, 8);
        assert_eq!(v.flow, ARRIVAL_HANDBACK_FLOW);
        assert_eq!((v.stage_id, v.phase, v.ramp_delay), (0, 0, 0));
        assert_eq!((v.ctx_243, v.ctx_278), (0, 0));
    }

    #[test]
    fn the_hand_back_rebinds_the_backdrop_and_blanks_the_ground_tile() {
        let mut st = ArrivalState {
            countdown: 1,
            ..Default::default()
        };
        let mut v = ArrivalView {
            phase: 6,
            ..Default::default()
        };
        let e = arrival_tick(&mut st, &mut v, 1);
        // Retail order: the rebind (0x801F7148), the `MoveImage`
        // (0x801F71CC), all inside the phase-6 arm that hands back.
        assert_eq!(
            e,
            vec![
                StageEffect::RebindBackdrop,
                ARRIVAL_GROUND_BLANK,
                StageEffect::HandBack
            ]
        );
        // The strip lands on the ground grid's tile window: page 13's
        // `(192..=255)` rows at halfword column `832 + 192 / 4`.
        let StageEffect::MoveImage {
            dst_x, dst_y, w, h, ..
        } = ARRIVAL_GROUND_BLANK
        else {
            unreachable!()
        };
        let (page_x, page_y) = legaia_asset::battle_backdrop::ground_page_xy();
        assert_eq!((dst_x, dst_y), (page_x + 192 / 4, page_y + 192));
        assert_eq!((w, h), (64 / 4, 64));
    }

    #[test]
    fn the_arrival_is_cadence_invariant_in_its_countdowns() {
        // Phase 1 burns 0x80 frames at step 1 and 0x40 ticks at step 2.
        let mut a = ArrivalState {
            countdown: 0x80,
            ..Default::default()
        };
        let mut va = ArrivalView {
            phase: 1,
            ..Default::default()
        };
        let mut n = 0;
        while va.phase == 1 {
            arrival_tick(&mut a, &mut va, 2);
            n += 1;
        }
        assert_eq!(n, 0x40);
    }

    #[test]
    fn the_form_transition_drops_the_boss_to_one_hp_and_reseats_the_party() {
        let mut st = FormTransitionState::default();
        let mut v = FormTransitionView {
            acting: 1,
            ..Default::default()
        };
        for s in &mut v.seats {
            s.present = true;
        }
        let mut draws = 0;
        let e = form_transition_tick(&mut st, &mut v, 1, &mut || {
            draws += 1;
            7
        });
        assert_eq!(e[0], StageEffect::Cue(FORM_TRANSITION_CUE));
        assert_eq!(v.action_state, FORM_TRANSITION_ACTION_STATE);
        assert_eq!(v.seats[3].hp, 1);
        assert_eq!(
            (v.seats[3].x, v.seats[3].z, v.seats[3].facing),
            (0, 0x320, 0x800)
        );
        assert_eq!(
            (v.seats[1].x, v.seats[1].z, v.seats[1].target),
            (0, -0x339, 8)
        );
        // The two onlookers take 0x12C and -0x12C, skipping the actor.
        assert_eq!((v.seats[0].x, v.seats[0].z), (0x12C, -0x307));
        assert_eq!((v.seats[2].x, v.seats[2].z), (-0x12C, -0x307));
        assert_eq!(v.phase, 1);
        assert_eq!(draws, 0);
        // Phase 1 draws three RNG values on its first frame.
        form_transition_tick(&mut st, &mut v, 1, &mut || {
            draws += 1;
            7
        });
        assert_eq!(draws, 3);
    }

    #[test]
    fn the_form_transition_shakes_blanks_the_field_and_exits() {
        let mut st = FormTransitionState::default();
        let mut v = FormTransitionView::default();
        let mut rng = || 0u16;
        form_transition_tick(&mut st, &mut v, 1, &mut rng);
        let mut ys = Vec::new();
        let mut exit_at = None;
        for i in 0..0x400 {
            let e = form_transition_tick(&mut st, &mut v, 1, &mut rng);
            if v.phase <= 2 {
                ys.push(v.camera.tr[1]);
            }
            if e.contains(&StageEffect::ExitBattle) {
                exit_at = Some(i);
                break;
            }
        }
        assert!(ys.windows(2).take(8).all(|w| w[0] != w[1]), "{ys:?}");
        assert!(
            v.seats
                .iter()
                .all(|s| s.render_flag == 0xFF && s.colour == 0)
        );
        assert_eq!((v.ctx_243, v.ctx_278), (1, 3));
        // 0x200 + 0x80 + 0x80 frames of countdown.
        assert_eq!(exit_at, Some(0x200 + 0x80 + 0x80 - 1));
    }
}
