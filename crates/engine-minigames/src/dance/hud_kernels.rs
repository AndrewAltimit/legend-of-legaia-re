//! HUD kernels: the step-mark spawn, digit glyphs, beat track, hit stings and the GOOD banner.
//! Split out of `dance.rs`.

use super::*;

// Wired: [`good_banner_spawn`] composes three of these records, and the RUN's
// own part pool hosts them - [`DanceGame::judge_press`] issues the spawns on
// the human's scoring judge, and every host draws what
// [`DanceGame::sprite_part_emits`] emits. Not the shared
// [`crate::minigame_fx`] pool: these spawns come from the judge, which is
// gameplay rather than host presentation. The overlay's own sprite page is
// still not uploaded on two of the three surfaces, so those draw each part
// through a placeholder cell rather than the `sprite_id` one.
/// PORT: FUN_801d3fd0 - the dance overlay's cell-placed effect spawn (the
/// step-mark flash): retail zero-fills a spawn record, spawns through the
/// shared part-spawn API `FUN_80021B04` at scale `0x1000`, stamps
/// `sprite_id` into the part's `+0x50` and places it at the dance-grid cell
/// (`x << 3`, `y << 3` into `+0x14`/`+0x16`). The Baka Fighter's
/// screen-centre twin is [`crate::baka_fighter::center_effect_spawn`]
/// (`FUN_801d6e04`).
pub fn step_mark_effect_spawn(
    cell_x: i16,
    cell_y: i16,
    sprite_id: u16,
) -> crate::baka_fighter::EffectSpawnSpec {
    crate::baka_fighter::EffectSpawnSpec {
        x: cell_x << 3,
        y: cell_y << 3,
        scale: 0x1000,
        sprite_id,
    }
}

// Wired: [`DanceGame::gauge_readout_quads`] patches this into widget `7`'s
// texture-U before emitting the pair, on the live HUD path
// ([`DanceGame::hud_draw_quads`] -> the play window's dance block).
/// PORT: FUN_801d3e28 - the score-banner thousands-digit glyph selector:
/// retail stores `(score / 1000) * 8 - 0x30` into the banner widget's
/// texture-U byte (`DAT_801d4760`) - each digit glyph is 8 texels wide and
/// the cell base sits `0x30` texels left of digit `0` - then draws widget
/// ids 6 and 7 (the two banner halves, 8 px apart) through the hub sprite
/// emitter `FUN_801d2f38` at brightness `0x80`, scale `0x1000`.
pub fn score_thousands_glyph_u(score: i32) -> i8 {
    ((score / 1000) * 8 - 0x30) as i8
}

// ------------------------------------------------------------ HUD kernels
//
// The overlay draws its whole HUD through the shared textured-quad emitter
// (`FUN_801d2f38`, an engine-ui concern). These functions carry the *arithmetic*
// those draws are parameterised by - the value a widget's texture-U / screen-x /
// CLUT is patched to - which is disc-derived game logic, not rendering. Each is
// the computational content of a HUD render routine; the quad emit stays a host
// job, exactly as [`step_mark_effect_spawn`] / [`score_thousands_glyph_u`]
// already split `FUN_801d3fd0` / `FUN_801d3e28`.

/// PORT: FUN_801d32f8 - the multi-digit number renderer's decimal split.
///
/// Retail fills eight slots with the `-1` sentinel, then **seeds the units
/// slot with `0`** (`sw zero,0x34(sp)` at `0x801D3358`, the eighth word of the
/// `sp+0x18` array), then walks the eight decimal places most-significant
/// first, storing the running quotient `value / 10^(7-i)` only when it is
/// non-zero. Per drawn slot it rewrites one HUD widget's texture-U (via
/// [`dance_score_digit_u`] / [`dance_level_digit_u`]) and x before emitting
/// it. This is the split: `Some(digit)` per drawn slot, `None` for a
/// suppressed leading zero - so `0` draws a single `0` in the units slot, the
/// same fill the fishing and dome digit fields share
/// (`legaia_engine_ui::other_game_hud::decimal_slots`).
///
/// Wired: the dance HUD's score readout in the play window runs through this,
/// so a blank slot really does draw nothing there.
pub fn dance_number_digits(value: u32) -> [Option<u8>; 8] {
    let mut out = [None; 8];
    out[7] = Some(0);
    let mut place = 10_000_000u32; // 10^7 - the leading of eight digit slots
    for slot in out.iter_mut() {
        let quotient = value / place;
        if quotient != 0 {
            *slot = Some((quotient % 10) as u8);
        }
        place /= 10;
    }
    out
}

// Wired: [`DanceGame::number_quads`] (style A) patches this into widget `1`'s
// texture-U per drawn digit slot, on the live HUD path
// ([`DanceGame::hud_draw_quads`] -> the play window's dance block).
/// PORT: FUN_801d32f8 style-A digit glyph-U (the score boxes, widget `1`):
/// `digit * 0x10` - each score glyph is 16 texels wide, drawn at a 16-px x step.
pub fn dance_score_digit_u(digit: u8) -> u8 {
    digit * 0x10
}

// Wired: [`DanceGame::number_quads`] (style B) patches this into widget
// `0x21`'s texture-U per drawn digit slot, on the same live HUD path as
// [`dance_score_digit_u`].
/// PORT: FUN_801d32f8 style-B digit glyph-U (widget `0x21`, the narrow counter):
/// `digit * 8 + 0x40` - 8-texel glyphs offset `0x40` into the page, 8-px x step.
pub fn dance_level_digit_u(digit: u8) -> u8 {
    digit * 8 + 0x40
}

/// Beat-track CLUT ids (`FUN_801d2524`): the caps + body idle palette.
pub const BEAT_TRACK_CLUT_IDLE: u16 = 0x7d08;
/// Beat-track combo-window flash palette (caps + body, on the combo slot).
pub const BEAT_TRACK_CLUT_COMBO: u16 = 0x7d0d;
/// Beat-track scrolling-note palette.
pub const BEAT_TRACK_CLUT_NOTE: u16 = 0x7d0e;
/// Intra-beat phase below which the combo slot flashes (`FUN_801d2524`: `< 0x46`).
pub const COMBO_FLASH_WINDOW: u32 = 0x46;

/// Wired: [`dance_combo_window_bright`], which the play window's beat-track row
/// reads every frame.
///
/// PORT: FUN_801d2524 - the beat-track's combo-slot mask. The beat index is
/// masked to 8 once the dancer has promoted a level (`gauge / 1000 > 0`), else 4,
/// so the flash + note read-out cadence widens on the higher rows.
pub fn dance_beat_level_mask(level: u32) -> u32 {
    if level > 0 { 7 } else { 3 }
}

/// This is the **displayed** combo slot, and it is not the judged one:
/// [`DanceGame::on_combo_slot`] masks the beat by `3` and accepts the whole
/// window, while the track's flash widens its mask with the dancer's level and
/// uses the much narrower [`COMBO_FLASH_WINDOW`]. Keep the two apart - the cell
/// the track lights is not the cell the judge scores.
///
/// Wired: the play window's dance HUD lights its beat-track row from this.
///
/// PORT: FUN_801d2524 - the combo-window flash test: the beat track lights its
/// caps + body ([`BEAT_TRACK_CLUT_COMBO`]) on the masked combo slot inside the
/// flash window, else it stays [`BEAT_TRACK_CLUT_IDLE`].
pub fn dance_combo_window_bright(beat: u32, level: u32, frac: u32) -> bool {
    beat & dance_beat_level_mask(level) == 3 && frac < COMBO_FLASH_WINDOW
}

/// Wired: the play window's dance HUD places its upcoming-note row at these
/// offsets (from its own pen, not the overlay's screen constant).
///
/// PORT: FUN_801d2524 - the scrolling note's screen-x. Note `i` sits at
/// `base_x + i*16`, scrolled left by the intra-beat fraction
/// (`(frac * 16) / BEAT_PERIOD + 5`) and a fixed 4-texel inset, so the row of
/// notes slides one 16-px cell per beat toward the judge line.
pub fn dance_beat_track_note_x(base_x: i32, i: u32, frac: u32) -> i32 {
    base_x + (i as i32) * 16 - ((frac * 16 / BEAT_PERIOD) as i32 + 5) - 4
}

// REF: FUN_80065034 (the voice-attr primitive both key-ons go through)
/// The `a1` both sting voices are keyed with (`li a1,0x2`): the voice-attr
/// primitive's **VAB id** (`legaia_engine_audio::VoiceAttr::vab_id`), i.e.
/// the dance's own bank in slot 2. The name predates that reading.
pub const STING_LEVEL: i8 = 2;

/// VAB **program** both sting voices come from (`li a2,0x1`) - the argument
/// the port used to drop, and the one that says which tone bank the
/// `2r` / `2r + 1` tone indices are inside.
pub const STING_PROGRAM: u8 = 1;

/// Sting variants the tier-2 award site draws between: `r = rand() % 3`, the
/// `0x55555556` magic-multiply divide at `FUN_801d1af4 + 0x64C`.
pub const STING_RANDOM_VARIANTS: u16 = 3;

/// The **fixed** sting the three groovy-move tiers key instead, and it is
/// outside the random space: all three of the tier-3 / 4 / 5 arms of
/// `FUN_801d1af4` reach `FUN_801d3d78` with a literal `5` (two `li a0,0x5`,
/// and a `move a0,v0` off the `li v0,0x5` the tier compare just loaded). So a
/// groovy move is not "cue only" - it fires cue `0x202` / `0x203` / `0x205`
/// *and* this sting, at tones `0xA` / `0xB` and note `0x41`.
pub const STING_TIER_VARIANT: u16 = 5;

/// One of the two voices a good-step sting keys (`FUN_801d3d78`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceStingVoice {
    /// Voice id handed to the SPU key-on primitive (`0x12` / `0x13`).
    pub voice: u16,
    /// The primitive's `a1`, the VAB id ([`STING_LEVEL`]).
    pub level: i8,
    /// VAB program ([`STING_PROGRAM`]).
    pub program: u8,
    /// Tone within [`STING_PROGRAM`] (`2r` / `2r + 1`).
    pub tone: i16,
    /// Note the voice is keyed at (`0x3c + r`).
    pub note: i16,
}

// Wired: the browser dance page's `dance_sting`
// (`web-viewer::minigames_dance`) takes its `(program, tone, note)` triple
// from here rather than recomputing it, and decodes the named tone out of the
// overlay's own VAB - which is what makes the bank index a read of
// [`STING_PROGRAM`] instead of a literal `1` that happened to agree. In the
// world, `World::tick_dance` queues both voices (via [`award_sounds`]) on the
// direct voice-key queue the session's `route_world_sfx` keys on both play
// hosts.
/// PORT: FUN_801d3d78 - the on-beat "good step" sting. A judged direction fires
/// **no** ring cue; it keys two voices together through the SPU voice-attr
/// primitive (`FUN_80065034`, whose eight arguments are `(voice, vab_id,
/// program, tone, note, 0x40, vol_l, vol_r)`): voice `0x12` at tone `2r` and
/// voice `0x13` at tone `2r + 1`, both in program [`STING_PROGRAM`] of VAB
/// [`STING_LEVEL`] and note `0x3c + r`. Both volume slots are the voice-volume
/// config `_DAT_80084580` halved, the same value
/// [`crate::other_game_overlay::cue_volume`] decodes. Returns the two voice
/// descriptors; the key-on itself is the audio host's.
///
/// `r` is not always a random pick. `FUN_801d1af4` reaches this from **four**
/// sites: the tier-2 chain-closed award passes `rand() % 3`
/// ([`STING_RANDOM_VARIANTS`]), and each of the three groovy-move tiers passes
/// the literal [`STING_TIER_VARIANT`]. Anything that enumerates "the stings"
/// over `0..3` is missing the one the higher tiers play.
pub fn dance_hit_sting_voices(r: u16) -> [DanceStingVoice; 2] {
    let note = 0x3c + r as i16;
    let voice = |voice: u16, tone: i16| DanceStingVoice {
        voice,
        level: STING_LEVEL,
        program: STING_PROGRAM,
        tone,
        note,
    };
    [voice(0x12, (2 * r) as i16), voice(0x13, (2 * r + 1) as i16)]
}

/// Ring slot the award's cues are stored into: `sh id, 0x8007B6DE` - slot 3
/// of the cue ring `DAT_8007B6D8`, written straight, without the cursor pair.
pub const AWARD_CUE_RING_SLOT: u8 = 3;

/// Ring slot the count-in's and the how-to tutorial's cues are stored into:
/// `sh id, 0x8007B6D8` (`FUN_801d2d98`, `FUN_801cf470`, `FUN_801d0750`).
pub const STAGE_CUE_RING_SLOT: u8 = 0;

/// The tier cues a **landed** groovy move raises, by the lane the dancer
/// was on when it pressed (`FUN_801d1af4`: `s1 = lane + 3`, then `0x202` /
/// `0x203` / `0x205` for `s1 = 3 / 4 / 5`).
pub const GROOVY_TIER_CUES: [u16; 3] = [0x202, 0x203, 0x205];

/// The miss cue (`FUN_801d1af4` `0x801D2118`).
pub const AWARD_MISS_CUE: u16 = 0x210;

/// One sound the human's award raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceAwardSound {
    /// A cue id stored into ring slot [`AWARD_CUE_RING_SLOT`] (runtime-bank
    /// ids, resolved through the dance's own `efect.dat`).
    Cue(u16),
    /// A good-step sting ([`dance_hit_sting_voices`]) at variant `r`;
    /// `random` = the tier-2 site's `rand() % 3`, which the host draws
    /// ([`STING_RANDOM_VARIANTS`]) - `r` is then ignored.
    Sting { r: u16, random: bool },
}

/// The sounds `FUN_801d1af4` raises for the **human** (dancer 0 - the whole
/// block sits behind `bne s3,zero` at `0x801D20F0`) after one award:
///
/// * a miss (`s1 == 1`) stores cue [`AWARD_MISS_CUE`];
/// * a closed chain (`s1 == 2`) keys a sting at `rand() % 3`;
/// * a groovy move (`s1 = lane + 3`) that **landed** (`DAT_801d570c == 1`)
///   keys the sting at [`STING_TIER_VARIANT`] and stores its tier cue
///   ([`GROOVY_TIER_CUES`]); one that did not land raises nothing.
///
/// `lane` is the dancer's lane at the press - the gauge `/ 1000` read before
/// the groovy move's `+1000` step.
///
/// PORT: FUN_801d1af4 (the dancer-0 sound arm, `0x801D20F0..0x801D2240`)
pub fn award_sounds(ev: DanceEvent, lane: u32) -> Vec<DanceAwardSound> {
    match ev {
        DanceEvent::Miss => vec![DanceAwardSound::Cue(AWARD_MISS_CUE)],
        DanceEvent::Sequence { .. } => vec![DanceAwardSound::Sting { r: 0, random: true }],
        DanceEvent::Groovy { landed: true, .. } => {
            let mut out = vec![DanceAwardSound::Sting {
                r: STING_TIER_VARIANT,
                random: false,
            }];
            if let Some(&cue) = GROOVY_TIER_CUES.get(lane as usize) {
                out.push(DanceAwardSound::Cue(cue));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The sequence-clear ("Good!") banner and its two flanking star sparkles
/// (`FUN_801d40dc`). The two stars carry the press's accuracy weight - retail
/// stores it into each star actor's `+0x72`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoodBannerSpawns {
    /// The "Good!" banner sprite (id `0xb`) at screen centre.
    pub banner: crate::baka_fighter::EffectSpawnSpec,
    /// The two star sparkles (sprite id `0x16`) flanking the banner.
    pub stars: [crate::baka_fighter::EffectSpawnSpec; 2],
    /// The accuracy weight stamped into each star's `+0x72`.
    pub weight: u16,
}

// Wired: [`DanceGame::judge_press`] spawns this on the human's scoring judge
// (`Judge::Sequence`) into the run's own part pool, which ages the three
// parts; every host draws them off [`DanceGame::sprite_part_emits`]. The
// dance overlay's sprite ids `0xb` / `0x16` are not resident on the native
// window or the play page, so their placeholder cells stand in for the
// banner art; the minigames page draws the widget cells themselves.
/// PORT: FUN_801d40dc - spawn the sequence-clear banner + two stars. Retail
/// issues three `FUN_801d3fd0` spawns - `(0xa0, 0x90, sprite 0xb)` for the banner
/// and `(0x68, 0x90, sprite 0x16)` / `(0xd8, 0x90, sprite 0x16)` for the stars
/// (the banner centred, the stars `0x38` to either side) - then stamps the
/// accuracy `weight` into each star's `+0x72`. The spawn records go through the
/// same primitive as [`step_mark_effect_spawn`].
pub fn good_banner_spawn(weight: u16) -> GoodBannerSpawns {
    GoodBannerSpawns {
        banner: step_mark_effect_spawn(0xa0, 0x90, 0xb),
        stars: [
            step_mark_effect_spawn(0x68, 0x90, 0x16),
            step_mark_effect_spawn(0xd8, 0x90, 0x16),
        ],
        weight,
    }
}
