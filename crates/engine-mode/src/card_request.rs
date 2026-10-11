//! The CARD request leaf: the two stores that hand the frame to the
//! memory-card / menu mode.
//!
//! PORT: FUN_801D84B4
//!
//! `FUN_801D84B4` is a six-store leaf in the field overlay that hands the
//! master mode to CARD; [`request_card_mode`] is wired through the mode seat
//! ([`crate::mode::ModeSeat::request_card_mode`]). It was ported beside the
//! op-`0x49` submode family (`legaia_engine_field::field_submode`), whose
//! provenance note covers the dumps it was read from.

/// Master game mode the CARD request leaf installs (`0x16` = 22, the
/// memory-card / menu overlay pair's init half - `crate::mode::GameMode::CardInit`).
pub const CARD_REQUEST_MODE: i16 = 0x16;

/// What the CARD request leaf writes.
///
/// REF: FUN_801D84B4 (`0x801d84b4..0x801d84cc`) - the `PORT:` for this address
/// sits on [`request_card_mode`], the free function that builds this record.
/// This is a plain data struct and the file declares no `impl` block at all, so
/// `port-catalog.py` widens the type anchor to the whole module - which is live
/// through `open_submode`, and that made a correct `NOT WIRED:` here read as
/// a stale disclosure. The wiring verdict lives on the function instead.
///
/// Seven instructions, no `jal` at all - two stores and `jr ra` with the second
/// store in the delay slot. It sets the master game mode word `_DAT_8007B83C`
/// to [`CARD_REQUEST_MODE`] and raises `_DAT_8007BB00`.
///
/// It is worth knowing that this leaf is the other half of a gate already
/// ported: the field initialiser's BGM wait barrier spins until the audio
/// driver acknowledges the track **unless** the master mode is `0x16`
/// (`legaia_engine_system::mode_entry_init::FIELD_BGM_WAIT_ABORT_MODE`). So a CARD request
/// raised while a scene is loading is what lets that barrier give up.
///
/// This function was previously in the port catalog's **ignore list** under a
/// "PADDING" reason. That reason holds for four minigame images that carry
/// filler at this VA; it does not hold for field (897), where the address is a
/// real entry with a real body. A six-store leaf is still a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardRequest {
    /// Value written to the master game mode word `_DAT_8007B83C`.
    pub game_mode: i16,
    /// Value written to `_DAT_8007BB00`.
    pub flag: u32,
}

/// The CARD request leaf's two stores.
///
/// PORT: FUN_801D84B4.
///
/// WIRED: `crate::mode::ModeSeat::request_card_mode` applies both stores,
/// and `engine-shell`'s `BootSession` opens the pause menu through it - the
/// seat is the production owner of the mode word this leaf writes.
///
/// The second store had "no engine counterpart at all, its consumer is not in
/// the ported set" written on it. That was wrong on both halves: `0x8007BB00`
/// is the front-end entry word, its two retail readers are `init.pak`'s
/// hand-off arm (`0x801CF4B0`, ported at `crate::mode::ModeSeat::boot_handoff`)
/// and the title dispatcher's `Init` routing (`0x801DD97C`, already ported as
/// `legaia_engine_system::mode_entry_init::ENTRY_WORD_ADDR`), and the seat now holds
/// the word both of them read.
pub const fn request_card_mode() -> CardRequest {
    CardRequest {
        game_mode: CARD_REQUEST_MODE,
        flag: 1,
    }
}
