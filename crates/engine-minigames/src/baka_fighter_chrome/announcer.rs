//! Announcer XA cues and the round-result banner sprites.

/// A CD-XA one-shot the chrome fires through `FUN_8003D53C(clip, chan, dur)`.
///
/// `clip` indexes the runtime clip table at `0x801C6ED8` (slot `i` = `XA<i+1>`),
/// so `0x20` = `XA33.XA` (the announcer bank) and `0x1F` = `XA32.XA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XaCue {
    pub clip: u8,
    pub chan: u8,
    pub dur: u16,
}

impl From<XaCue> for legaia_engine_system::sound_state::XaVoiceClip {
    fn from(c: XaCue) -> Self {
        Self {
            clip: u32::from(c.clip),
            channel: u32::from(c.chan),
            duration_sectors: u32::from(c.dur),
        }
    }
}

/// The intro title card's first announcer line (`XA33` channel `0x0E`),
/// started once as the logo fades in ([`IntroTitle::frame`]).
pub const INTRO_LOGO_XA: XaCue = XaCue {
    clip: 0x20,
    chan: 0x0E,
    dur: 0x3F,
};
/// The intro title card's second line (`XA33` channel `0x0F`), started once
/// as the subtitle holds.
pub const INTRO_SUBTITLE_XA: XaCue = XaCue {
    clip: 0x20,
    chan: 0x0F,
    dur: 0x76,
};
/// The countdown's state-`0` line (`XA33` channel `0x0A`,
/// [`countdown_frame`]).
pub const COUNTDOWN_ARM_XA: XaCue = XaCue {
    clip: 0x20,
    chan: 0x0A,
    dur: 0x46,
};
/// The countdown's state-`1` line, once the scene load has cleared (`XA33`
/// channel `0x0B`).
pub const COUNTDOWN_LOADED_XA: XaCue = XaCue {
    clip: 0x20,
    chan: 0x0B,
    dur: 0x4D,
};
/// The countdown's timer-expiry line on an ordinary round (`XA33` channel
/// `0x0C`).
pub const COUNTDOWN_ROUND_XA: XaCue = XaCue {
    clip: 0x20,
    chan: 0x0C,
    dur: 0x5A,
};
/// The countdown's timer-expiry line on the final round
/// ([`COUNTDOWN_FINAL_ROUND`], `XA33` channel `0x0D`).
pub const COUNTDOWN_FINAL_XA: XaCue = XaCue {
    clip: 0x20,
    chan: 0x0D,
    dur: 0x66,
};
/// Clip slot of the round banner's line (`XA32.XA`).
pub const ROUND_BANNER_XA_CLIP: u8 = 0x1F;
/// Read span of the round banner's line.
pub const ROUND_BANNER_XA_DURATION: u16 = 0x48;

/// The round banner's announcer line for `round` (0-based): `XA32`, and the
/// channel is the round index itself ([`round_banner_frame`]).
pub fn round_banner_xa(round: i32) -> XaCue {
    XaCue {
        clip: ROUND_BANNER_XA_CLIP,
        chan: (round & 0xFF) as u8,
        dur: ROUND_BANNER_XA_DURATION,
    }
}

/// How a round ended, as the resolution SM's banner tail reads it off the two
/// fighters' HP (`FUN_801D3468`, `0x801D3864..0x801D39F0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundResult {
    /// The player standing, the opponent down, the player hit this round.
    Win,
    /// The player standing, the opponent down, the player never hit - the
    /// untouched flag `DAT_801DBF24` (raised at the round's go, cleared by
    /// both arms that damage slot 0) still up.
    Perfect,
    /// The player down, the opponent standing.
    Lose,
    /// Both down on the same exchange.
    Draw,
}

/// Widget ids the result banners spawn (`FUN_801D6E04`'s first argument):
/// the duel's own sheet cells "YOU" / "WIN!" / "LOSE..." / "DRAW" /
/// "PERFECT!!".
pub const RESULT_YOU_SPRITE: u16 = 0x07;
pub const RESULT_WIN_SPRITE: u16 = 0x08;
pub const RESULT_LOSE_SPRITE: u16 = 0x09;
pub const RESULT_DRAW_SPRITE: u16 = 0x0A;
pub const RESULT_PERFECT_SPRITE: u16 = 0x11;
/// The x the two-word banners shift their halves by off the spawn's screen
/// centre (`addiu v0,v0,0x30` / `-0x30` on the part's `+0x14`).
pub const RESULT_WORD_SHIFT: i16 = 0x30;
/// How long a round's result banners stand before the next round's banner
/// takes the screen: the duel state's round-over hold (`0x64`'s
/// `state_timer` runs to `0xB5` before the cabinet moves to the round setup
/// `0x32`, whose beats `0x33..0x36` draw the round banner).
pub const RESULT_HOLD_FRAMES: i32 = 0xB5;

/// The announcer line each result starts (`FUN_8003D53C(0x20, chan, dur)`
/// right before its spawn).
pub fn result_xa(r: RoundResult) -> XaCue {
    let (chan, dur) = match r {
        RoundResult::Win => (2, 0x45),
        RoundResult::Lose => (3, 0x6D),
        RoundResult::Draw => (4, 0x35),
        RoundResult::Perfect => (5, 0x39),
    };
    XaCue {
        clip: 0x20,
        chan,
        dur,
    }
}

/// The banner sprites one result spawns, as `(widget, x, y)`.
///
/// Every spawn goes through the screen-centre wrapper (`FUN_801D6E04`,
/// `(0xA0, 0x78)`). The two-word banners spawn the second word first and
/// shift it right by [`RESULT_WORD_SHIFT`] (stored in the delay slot of the
/// next spawn's `jal`), then spawn "YOU" and shift it left by the same: WIN /
/// LOSE at `0xD0`, YOU at `0x70`. DRAW and PERFECT!! stand alone at the
/// centre.
///
/// PORT: FUN_801d3468 (`0x801D3864..0x801D39F0`, the round-result banner tail)
pub fn result_banner_spawns(r: RoundResult) -> Vec<(u16, i16, i16)> {
    let c = crate::baka_fighter::center_effect_spawn(0);
    let (x, y) = (c.x, c.y);
    match r {
        RoundResult::Win => vec![
            (RESULT_WIN_SPRITE, x + RESULT_WORD_SHIFT, y),
            (RESULT_YOU_SPRITE, x - RESULT_WORD_SHIFT, y),
        ],
        RoundResult::Lose => vec![
            (RESULT_LOSE_SPRITE, x + RESULT_WORD_SHIFT, y),
            (RESULT_YOU_SPRITE, x - RESULT_WORD_SHIFT, y),
        ],
        RoundResult::Draw => vec![(RESULT_DRAW_SPRITE, x, y)],
        RoundResult::Perfect => vec![(RESULT_PERFECT_SPRITE, x, y)],
    }
}

/// Every announcer line the chrome can start from round `round`'s banner on,
/// for a host that stages a clip ahead of the request that plays it (the
/// browser page decodes a clip the bank lacks one request per frame, so a
/// line first asked for at its frame sounds late there): the intro card's
/// two lines, the countdown's four, and the banner lines for `round` and the
/// round after it. The round-advance edge lists the next banner line again
/// ([`crate::baka_fighter::BakaFight::take_xa_prestage`]). Deduplicated,
/// in the order the chrome starts them.
pub fn announcer_xa_prestage(round: i32) -> Vec<XaCue> {
    vec![
        INTRO_LOGO_XA,
        INTRO_SUBTITLE_XA,
        round_banner_xa(round),
        COUNTDOWN_ARM_XA,
        COUNTDOWN_LOADED_XA,
        COUNTDOWN_ROUND_XA,
        COUNTDOWN_FINAL_XA,
        round_banner_xa(round + 1),
        result_xa(RoundResult::Win),
        result_xa(RoundResult::Lose),
        result_xa(RoundResult::Draw),
        result_xa(RoundResult::Perfect),
    ]
}
