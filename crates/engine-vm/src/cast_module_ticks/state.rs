//! State views: the actor and module-context slices a cast routine reads and writes.
//! Split out of `cast_module_ticks.rs`.

// ---------------------------------------------------------------------------
// State views
// ---------------------------------------------------------------------------

/// The slice of one battle actor's record a slot-B routine reads or writes.
///
/// Field names carry their retail offsets in the doc comments; a host bridges
/// this to its own actor record rather than the kernels reaching into a
/// global pool, so every routine here is directly testable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastActorState {
    /// `+0x0C` - the root-speed / magnitude word the stagers ramp.
    pub root_speed: i32,
    /// `+0x10` - pending HP-bar delta, accumulated by every apply site.
    pub hp_bar_delta: i32,
    /// `+0x14C` - live HP. Also the liveness flag: `0` means dead, and both
    /// AoE stagers skip such a seat.
    pub hp: u16,
    /// `+0x16E` - per-actor flag bank. Only [`FLAG_NON_TARGETABLE`](super::FLAG_NON_TARGETABLE) is read
    /// in this band.
    pub flags: u16,
    /// `+0x1D9` - the clip currently playing.
    pub playing_anim: u8,
    /// `+0x1DA` - the staged (queued) clip id.
    pub staged_anim: u8,
    /// `+0x1DC` - the restage counter, bumped with most `+0x1DA` writes.
    pub restage: u8,
    /// `+0x1DD` - active-target byte.
    pub target_code: u8,
    /// `+0x1F1` - this actor's own knockdown-reaction clip id, which the AoE
    /// stagers stage back onto it.
    pub knockdown_anim: u8,
    /// `+0x21C` - render flag (`0` visible, `0xFF` hidden by a summon fade).
    pub render_flag: u8,
    /// `+0x21D` - animation-rate scalar, normal [`ANIM_RATE_NORMAL`](super::ANIM_RATE_NORMAL).
    pub anim_rate: u8,
    /// `+0x154` - AGL **working**, the per-round action gauge.
    pub agl: u16,
    /// `+0x156` - AGL **base**, the value the round boundary restores
    /// [`CastActorState::agl`] to. PROT 0942's Power Up writes this half and
    /// only this half.
    pub agl_base: u16,
    /// `+0x158` / `+0x15A` - ATK working / base. Raised as a pair by PROT
    /// 0955's Power Charge and lowered as a pair by its Melt Spray.
    pub atk: u16,
    /// See [`CastActorState::atk`].
    pub atk_base: u16,
    /// `+0x15C` / `+0x15E` - UDF (upper defence) working / base.
    pub udf: u16,
    /// See [`CastActorState::udf`].
    pub udf_base: u16,
    /// `+0x160` / `+0x162` - LDF (lower defence) working / base.
    pub ldf: u16,
    /// See [`CastActorState::ldf`].
    pub ldf_base: u16,
    /// `+0x164` / `+0x166` - SPD working / base.
    pub spd: u16,
    /// See [`CastActorState::spd`].
    pub spd_base: u16,
    /// `+0x168` / `+0x16A` - INT working / base.
    pub intel: u16,
    /// See [`CastActorState::intel`].
    pub intel_base: u16,
    /// `+0x170` - the spirit-art charge gauge, filled on the defender of
    /// every damaging hit by the shared finisher. Monster `0x8A`'s pick
    /// reads its own as the Chaos Breath gate, and PROT 0938's `0x4E` body
    /// drains it back down ([`super::chaos_breath_tick`]).
    pub spirit_gauge: u16,
    /// `+0x16C` - the per-round **initiative key**, doubling as "has not
    /// acted yet". The turn-steal idiom clears it.
    pub init_key: u16,
    /// `+0x1DE` - action category. `1` is Item, which is what the refund
    /// arm gates on.
    pub action_category: u8,
    /// `+0x1DF` - the queued action byte. For an Item action it is the item
    /// id the refund hands back.
    pub queued_action: u8,
    /// `+0x1EF` / `+0x1F0` - the two alternative reaction clips the band
    /// stages when `+0x1F2` is zero.
    pub reaction_alt: u8,
    /// See [`CastActorState::reaction_alt`].
    pub reaction_alt2: u8,
    /// `+0x04` - the per-actor **mesh tint word** the band's hit sites stamp
    /// alongside the HP write (PROT 0904's ring sweep stores `0x3FF0000`
    /// there per hit). It is the same word
    /// [`BattleActor::render_color`](crate::battle_action::BattleActor::render_color)
    /// already models, so a store here reaches the renderer's tint pass
    /// rather than stopping at this view.
    pub present_04: u32,
    /// `+0x21F` - the **impact-effect selector** a hit arm sets beside
    /// [`Self::render_flag`] (`2` on PROT 0904's ring-sweep victims): which
    /// entry of the impact-config table owns [`Self::present_04`]. Mirrors
    /// [`BattleActor::impact_state`](crate::battle_action::BattleActor::impact_state).
    pub render_21f: u8,
    /// `+0x225` - the capture-state byte PROT 0907's kill arm writes together
    /// with `+0x21C` (one `li v0,0x2` at `0x801F7E54` feeds both stores).
    /// Mirrors
    /// [`BattleActor::capture_state`](crate::battle_action::BattleActor::capture_state).
    pub render_225: u8,
    /// `+0x1F2` - the gate that picks [`CastActorState::knockdown_anim`]
    /// over [`CastActorState::reaction_alt`].
    pub reaction_gate: u8,
}

/// The battle-context bytes a slot-B routine drives (`ctx` is
/// `*0x8007BD24`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastModuleCtx {
    /// `ctx+0` - the **party** count, not the actor count. `FUN_8004B3E8`
    /// reads it as the bound of a loop over `DAT_8007BD10[i]` (the per-seat,
    /// 1-based party character id) whose body indexes the `0x414`-byte party
    /// records at `0x80084140 + 0x6C0 + 0x414*(id-1)` (`0x8004B420..0x8004B448`),
    /// so only the party seats are in range. The Evil Seru Magic loop's bound.
    pub party_count: u8,
    /// `ctx+1` - monster count: the bound of the wipe sweeps at `0x8004B10C`
    /// and `0x8005039C`, whose bodies index `actor_table[(i + 3)]`. The
    /// Juggernaut loop's bound.
    pub monster_count: u8,
    /// `ctx+0x13` - the caster's seat.
    ///
    /// **Not** the wrapper's `a1` on the summon band. Every `jal 0x801DD0AC`
    /// word in that band bakes `addiu a1, zero, 7` (the summon seat) instead:
    /// 34 sites over PROT `0903..=0934` and the inherited-tail copies of
    /// them, `0x801F74A8` in PROT 0903, `0x801F7C98` in PROT 0933,
    /// `0x801F8880` in PROT 0910 - the last being the only one in a `jal`
    /// delay slot, along with the copy of it in PROT 0911's tail. The seat
    /// byte reaches a wrapper only on the capture-class
    /// band's bypass sites, which spell it `lbu a1, 0x13(ctx)`
    /// (`0x801F71E4` / `0x801F7A8C` / `0x801F7EB8` in PROT 0959, and the
    /// same form in 0944 / 0952 / 0953 / 0958 / 0960). The band's own
    /// kernels read it for `+0x1DD` target codes and record lookups, never
    /// as a damage argument.
    pub caster_seat: u8,
    /// `ctx+0x278` - a module scratch byte; three of these routines write it.
    pub ctx_278: u8,
    /// `ctx+0x279` - the **module phase**, the second phase space riding
    /// under battle phase `0x70`.
    pub phase: u8,
    /// `ctx+0x0D` - the band's own "this cast is finished" byte, which every
    /// PROT 0955 body clears in its terminal arm (`sb zero,0xd(ctx)`).
    pub ctx_0d: u8,
    /// `ctx+0x1A` - the **turn cursor**. The turn-steal arms bump it, which
    /// is how they consume the victim's turn.
    pub turn_cursor: u8,
    /// `ctx+0x27A` - a second scratch byte, cleared beside `ctx+0x278` by
    /// PROT 0965's damage arm (`0x801F789C`).
    pub ctx_27a: u8,
}

/// What a tick reported to the drive loop. Retail's `0x801E4CA8` /
/// `0x801E50C8` sites hold battle phase `0x70` while the tick returns
/// non-zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastTickStep {
    /// The choreography is still running.
    Busy,
    /// The phase walked past the module's last arm, so the dispatch falls
    /// through to the epilogue and the routine writes nothing.
    Done,
}
