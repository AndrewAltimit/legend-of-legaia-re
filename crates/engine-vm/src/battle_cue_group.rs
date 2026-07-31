//! Two battle-overlay kernels that turn a committed action into presentation
//! work: the cue-group expander that spawns an action's effect/SFX set, and
//! the target-banner planner that decides which HUD banner the action gets.
//!
//! Each address is tagged on the function that implements it; the two have
//! different wiring status, which a file-wide anchor could not express.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801e22c8.txt`
//! and `overlay_battle_action_801e6d84.txt`. `FUN_801E6D84` is reached from
//! the action state machine at `0x801E3028`
//! (`overlay_battle_action_801e295c.txt`).

// ---------------------------------------------------------------------------
// FUN_801E22C8 - cue-group expansion
// ---------------------------------------------------------------------------

/// Byte stride of one record in the cue-group table at `0x801F6470`.
/// Layout is `[count: u8][id: u8; 4]`, so a group holds at most four cues.
///
/// The disc-side parser states the same stride as
/// `legaia_asset::move_power::CUE_GROUP_STRIDE`; the two are pinned equal by
/// `constants_agree_with_the_disc_parser` below.
pub const CUE_GROUP_STRIDE: usize = 5;

/// Maximum cues one group can name.
pub const CUE_GROUP_MAX: usize = CUE_GROUP_STRIDE - 1;

/// Bit that marks a cue id as an **actor** cue rather than an effect cue.
/// Set means `FUN_801DFDF0(id & 0x7F, pos, yaw)`; clear means the SFX +
/// effect-spawn pair.
pub const CUE_ACTOR_FLAG: u8 = 0x80;

/// The yaw bias `FUN_801E22C8` adds to the actor's `+0x46` heading before it
/// builds the spawn transform: a half turn, so the effect faces the actor.
/// The actor-cue arm subtracts it again and passes the original heading.
pub const CUE_YAW_BIAS: u16 = 0x800;

/// The tint word that means "leave the spawned effect's colour alone".
pub const CUE_TINT_NEUTRAL: u32 = 0x0080_8080;

/// The high byte OR-ed into a non-neutral tint before it is written to the
/// spawned effect's `+0x74`.
pub const CUE_TINT_MODE: u32 = 0x8900_0000;

/// The `+0x78` blend word written alongside a non-neutral tint.
pub const CUE_TINT_BLEND: u16 = 0x0800;

/// The actor `+0x04` value that suppresses the `+0x0C = 0x2000` follow-up.
pub const CUE_ACTOR_STATE_SKIP: u32 = 0x2008_0200;

/// The `q12` scale `FUN_801E22C8` passes as the effect spawn's fourth
/// argument - unity, never varied.
pub const CUE_SPAWN_SCALE: i32 = 0x1000;

/// One expanded cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueSpawn {
    /// `id & 0x80` set: `FUN_801DFDF0(id & 0x7F, &pos, yaw)`, where `yaw` is
    /// the actor's **unbiased** `+0x46` heading.
    Actor { id: u8, yaw: i16 },
    /// `id & 0x80` clear. `sfx` is the byte the SFX map at `0x801F6418`
    /// carries for this id - `None` when that byte is zero, in which case
    /// retail emits no sound packet at all. `effect_index` indexes the
    /// effect-parameter table at `0x801F6324` (word stride).
    Effect {
        id: u8,
        sfx: Option<u8>,
        effect_index: u8,
        /// `Some(word)` when the caller's tint is not [`CUE_TINT_NEUTRAL`];
        /// the value is what retail stores at the spawned effect's `+0x74`.
        tint: Option<u32>,
    },
}

/// What one call to `FUN_801E22C8` produces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CueGroupExpansion {
    /// Spawns in retail order.
    pub spawns: Vec<CueSpawn>,
    /// The actor `+0x04` write, always performed.
    pub actor_state: u32,
    /// `Some(0x2000)` when the `+0x0C` follow-up write runs. It is skipped
    /// only for the exact state word [`CUE_ACTOR_STATE_SKIP`].
    pub actor_flags: Option<u32>,
}

/// The tables `FUN_801E22C8` reads out of the battle overlay's data band.
#[derive(Debug, Clone, Copy)]
pub struct CueTables<'a> {
    /// `0x801F6470` - the `[count][id;4]` groups, `CUE_GROUP_STRIDE` apart.
    pub groups: &'a [u8],
    /// `0x801F6418` - per-cue-id SFX byte; zero means "no sound".
    pub sfx_map: &'a [u8],
}

/// Expand one action's cue group into its spawns. `FUN_801E22C8`.
///
/// `actor_yaw` is the actor's `+0x46` heading **before** retail's `+= 0x800`
/// bias. The bias only matters for the effect arm, which passes the biased
/// rotation blob to the spawn call; the actor arm re-subtracts it, so this
/// port passes the unbiased value straight through and records the bias as a
/// constant instead of round-tripping it.
///
/// `tint` is `param_1`. Any value other than [`CUE_TINT_NEUTRAL`] recolours
/// every spawned effect.
///
/// A group whose count byte is zero produces no spawns; the two actor writes
/// still happen.
///
/// Live from the battle-action SM's state `0x3F`
/// (`battle_action::spirit`'s `spirit_fire_damage`, retail `0x801E4134`): the
/// committed action's `(class, tier)` pair picks a site through
/// [`cue_group_for`], this expands that site's group, and each
/// [`CueSpawn`] is handed to
/// [`BattleActionHost::spawn_cue`](crate::battle_action::BattleActionHost::spawn_cue).
/// Both tables come off the disc - `legaia_asset::move_power::EffectAuxTables`
/// reads all three regions (`0x801F6324` prototypes, `0x801F6418` SFX map,
/// `0x801F6470` groups) off PROT 0898 - through
/// [`BattleActionHost::cue_tables`](crate::battle_action::BattleActionHost::cue_tables),
/// so a host with no overlay expands nothing rather than expanding synthetic
/// records. Composition oracle: `crates/engine-vm/tests/battle_cue_group_real.rs`.
///
/// Retail reaches this from `FUN_800402F4`, the item / restore applier, at
/// eleven branches of its 132-entry class jump table (`0x80014FA0`); the port
/// reaches it from the applier's one SM call site instead, because the port
/// models the applier as a host hook rather than porting its 1976
/// instructions. What crosses that seam is exactly what retail's branch
/// selection reads - see [`cue_group_for`].
///
/// The art record's own effect / hit cues (`ArtStrikeInfo::hit_cue` and
/// `BattleSfxCue`) stay the presentation source for the *attack* band; this is
/// the Item / Spirit band's, and the two do not overlap.
///
/// PORT: FUN_801E22C8
/// REF: FUN_800402F4 (the damage primitive that picks the group id),
/// REF: FUN_801E295C (its one call site in the action SM, `0x801E4134`),
/// REF: FUN_801EC3E4 (what the strike loop resolves damage through instead),
/// REF: FUN_801DFDF0 (actor-cue spawn), FUN_80050ED4 (effect spawn),
/// REF: FUN_80058490 (sound packet submit)
pub fn expand_cue_group(
    tint: u32,
    actor_state: u32,
    actor_yaw: i16,
    group_id: u8,
    tables: &CueTables<'_>,
) -> CueGroupExpansion {
    let mut out = CueGroupExpansion {
        actor_state,
        actor_flags: if actor_state == CUE_ACTOR_STATE_SKIP {
            None
        } else {
            Some(0x2000)
        },
        ..Default::default()
    };

    let base = group_id as usize * CUE_GROUP_STRIDE;
    let count = tables.groups.get(base).copied().unwrap_or(0);
    if count == 0 {
        return out;
    }

    // Retail's loop counter is a byte compared against the count byte, so a
    // count above 4 walks into the next group's record. The port keeps that
    // reachable rather than clamping, but stops at the end of the slice.
    for i in 0..count as usize {
        let Some(&id) = tables.groups.get(base + 1 + i) else {
            break;
        };
        if id & CUE_ACTOR_FLAG != 0 {
            out.spawns.push(CueSpawn::Actor {
                id: id & !CUE_ACTOR_FLAG,
                yaw: actor_yaw,
            });
        } else {
            let sfx = tables.sfx_map.get(id as usize).copied().filter(|&s| s != 0);
            out.spawns.push(CueSpawn::Effect {
                id,
                sfx,
                effect_index: id,
                tint: (tint != CUE_TINT_NEUTRAL).then_some(tint | CUE_TINT_MODE),
            });
        }
    }
    out
}

/// Which of the applier's eleven `jal 0x801e22c8` sites a committed
/// `(class, tier)` pair reaches, and the four arguments it passes.
///
/// Every field is a literal in the site's own instruction stream, so a site is
/// fully determined by the branch - nothing about the acting actor varies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueGroupSite {
    /// The `a0` tint word ([`expand_cue_group`]'s `tint`), built by the
    /// site's `lui`/`ori` pair.
    pub tint: u32,
    /// The `a1` actor-state word written to `actor[+0x04]`. The class-`4`
    /// revive site is the one that passes [`CUE_ACTOR_STATE_SKIP`], so it is
    /// also the only arm whose `+0x0C = 0x2000` follow-up is suppressed.
    pub actor_state: u32,
    /// The `a3` group id the site passes to [`expand_cue_group`].
    pub group: u8,
    /// `true` for the class-`1` arm, whose `jal` sits **inside** a per-slot
    /// loop: a party-wide restore fires the expander once per living member,
    /// with `a2` the loop's own slot index rather than the action's target.
    pub per_target: bool,
    /// `true` when the site is one of retail's battle-mode-gated arms
    /// (`*(s16 *)0x8007B83C == 0x15`). Inside this state machine the mode
    /// byte is battle by construction, so the flag is recorded rather than
    /// tested - it is what tells a *non*-battle caller of the applier which
    /// arms it must skip.
    pub battle_only: bool,
}

/// Select the applier's cue-group site for a committed action's
/// `(class, tier)` - the `(a3, a2)` half of `FUN_800402F4`'s eleven
/// `jal 0x801e22c8` branches, without the 1976-instruction applier around it.
///
/// `class` is the actor's `+0x1E8` and `tier` its `+0x1E9`
/// ([`BattleActor::cast_class`](crate::battle_action::BattleActor::cast_class) /
/// [`cast_sub_class`](crate::battle_action::BattleActor::cast_sub_class)).
/// The mapping, read off the delay slots of the eleven `jal`s in
/// `ghidra/scripts/funcs/800402f4.txt` and tabulated in
/// `docs/subsystems/battle-action.md` § "`FUN_800402F4`'s cue-group sites":
///
/// | class | site | group | `a0` tint | `a1` actor state |
/// |---|---|---|---|---|
/// | `0` HP restore, single | `0x800408F8` | `tier` | `0x00808080` | `0x000FFFFF` |
/// | `1` HP restore, per-slot loop | `0x80040D70` | `tier + 1`, once per seated slot | `0x00808080` | `0x000FFFFF` |
/// | `2` MP restore | `0x80040E54` | `tier + 3` | `0x00FF8080` | `0x3FF80200` |
/// | `3` status cure | `0x80040F04` | `5` | `0x0080FFFF` | `0x200FFFFF` |
/// | `4` revive | `0x800410B8` | `6` | `0x0080FFFF` | `0x20080200` |
/// | `5` spirit shield | `0x8004111C` | `7` | `0x004040FF` | `0x100403FF` |
/// | `7` stat buff, `tier == 1` | `0x8004157C` | `8` | `0x00808080` | `0x3FF00000` |
/// | `7` stat buff, `tier == 2` | `0x80041718` | `9` | `0x00808080` | `0x000FF000` |
/// | `7` stat buff, `tier == 3` | `0x800417FC` | `0xA` | `0x000000FF` | `0x000003FF` |
/// | `7` stat buff, `tier == 4` | `0x80041BA0` | `0xB` | `0x0000FFFF` | `0x000FF3FF` |
/// | `8` status clear | `0x80041C60` | `0xC` | `0x0080C0C0` | `0x200C0300` |
///
/// The class-`0`/`1` tint is [`CUE_TINT_NEUTRAL`], so a plain HP restore
/// recolours nothing; every other arm recolours. The class-`4` `a1` is
/// [`CUE_ACTOR_STATE_SKIP`] exactly, which is what makes revive the one arm
/// that leaves `actor[+0x0C]` alone.
///
/// Everything else returns `None`: classes `6`, `9`, `0xA`, `0xB`..`0xD`,
/// `0xE` and `0x82` have arms that never reach the expander (class 6's own
/// 7-entry inner table at `0x800151B0` only bumps counters), a class-`7` tier
/// outside `1..=4` falls past all four of its `jal`s, and every class byte
/// from the spell table's routing band (`0x14` plain cast, `0x32` summon,
/// `0x63` capture) is above the jump table's `0x84` bound or lands on an arm
/// with no `jal`.
///
/// **What a missing tier costs.** Six of the eight rows are literals, so a
/// host that cannot supply `+0x1E9` still selects the right group for classes
/// `3`, `4`, `5` and `8`; only the two restore classes and the stat-buff class
/// move with it, and for those a zero tier reads as "tier 0" rather than as a
/// wrong branch (class `7` tier `0` correctly selects *no* site).
///
/// PORT: FUN_800402F4 (the cue-group branch selection only - the arms'
/// stat arithmetic is [`BattleActionHost::apply_damage`](crate::battle_action::BattleActionHost::apply_damage)'s
/// side of the seam)
/// REF: FUN_801E22C8 (what each site calls)
pub fn cue_group_for(class: u8, tier: u8) -> Option<CueGroupSite> {
    let site = |tint: u32, actor_state: u32, group: u8, battle_only: bool| {
        Some(CueGroupSite {
            tint,
            actor_state,
            group,
            per_target: false,
            battle_only,
        })
    };
    match class {
        0 => site(0x0080_8080, 0x000F_FFFF, tier, true),
        1 => Some(CueGroupSite {
            per_target: true,
            ..site(0x0080_8080, 0x000F_FFFF, tier.wrapping_add(1), true)?
        }),
        2 => site(0x00FF_8080, 0x3FF8_0200, tier.wrapping_add(3), true),
        3 => site(0x0080_FFFF, 0x200F_FFFF, 5, true),
        4 => site(0x0080_FFFF, CUE_ACTOR_STATE_SKIP, 6, true),
        5 => site(0x0040_40FF, 0x1004_03FF, 7, false),
        7 => match tier {
            1 => site(0x0080_8080, 0x3FF0_0000, 8, false),
            2 => site(0x0080_8080, 0x000F_F000, 9, false),
            3 => site(0x0000_00FF, 0x0000_03FF, 0xA, false),
            4 => site(0x0000_FFFF, 0x000F_F3FF, 0xB, false),
            _ => None,
        },
        8 => site(0x0080_C0C0, 0x200C_0300, 0xC, true),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// FUN_801E6D84 - target-banner planner
// ---------------------------------------------------------------------------

/// First monster slot in the battle actor pointer table.
pub const MONSTER_SLOT_FIRST: u8 = 3;
/// One past the last monster slot the banner planner scans (`sltiu ..., 0x7`).
pub const MONSTER_SLOT_END: u8 = 7;

/// The `+0x1DD` target value that means "every enemy" rather than a slot.
pub const TARGET_ALL_ENEMIES: u8 = 9;

/// The `+0x1DD` target value that selects the party-wide banner arm.
pub const TARGET_PARTY_WIDE: u8 = 8;

/// The three action ids that force the multi-target layout even when the
/// target byte names a single slot. All three sit in the player Seru-magic
/// block and are reached only with action category `2`.
pub const MULTI_TARGET_ACTION_IDS: [u8; 3] = [0x82, 0x86, 0x8D];

/// Action category `5` - **Run / Defend** - short-circuits the whole routine,
/// so a fleeing or defending actor raises no banner at all.
pub const CATEGORY_SKIP: u8 = 5;

/// Action categories `0` (Tactical Arts) and `4` (Spirit) skip the target arm
/// (the caster banner has already been raised by then).
pub const CATEGORY_NO_TARGET_BANNER: [u8; 2] = [0, 4];

/// The HUD element id the caster banner raises.
pub const HUD_CASTER_BANNER: u8 = 0x44;
/// The HUD element id the single-target banner raises.
pub const HUD_TARGET_BANNER: u8 = 0x51;
/// The three HUD element ids the party-wide arm raises, in order.
pub const HUD_PARTY_WIDE: [u8; 3] = [0x06, 0x4E, 0x4F];

/// The banner width base: the single-target arm stores `0x130 - width` in two
/// of the three HUD fields.
pub const BANNER_WIDTH_BASE: i16 = 0x130;

/// The width both HUD fields fall back to when no target banner is raised.
pub const BANNER_WIDTH_IDLE: i16 = 0x10;

/// The state byte the party-wide arm writes into `ctx[+0x18]`.
pub const CTX_18_PARTY_WIDE: u8 = 6;

/// The inputs `FUN_801E6D84` reads. Every field is named for its retail
/// source so the read is checkable against the dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BannerInputs {
    /// `ctx[+0x13]` - active actor slot.
    pub active_slot: u8,
    /// Active actor `+0x1DE` - action category.
    pub action_category: u8,
    /// Active actor `+0x1DF` - queued action id.
    pub action_id: u8,
    /// Active actor `+0x1DD` - target slot, or `8` / `9` for the two
    /// group forms.
    pub target: u8,
    /// `ctx[+0x24B]` - the override target slot the `target == 9` arms
    /// consult.
    pub ctx_override_slot: u8,
    /// Liveness of monster slots `3..=6` (`+0x14C != 0`), in slot order.
    pub monster_alive: [bool; 4],
}

/// Which banner layout `FUN_801E6D84` selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerLayout {
    /// Action category `5`: the routine returns immediately, raising nothing.
    Skipped,
    /// The multi-target arm. `slots` lists the living non-active monster
    /// slots in scan order; each takes pose-slot pattern entry
    /// `pattern[(slots.len() - 1) * 4 + i]` (the table at `0x801F6834`) and
    /// the descriptor `ctx + 0x292 + (slot - 3) * 0x20`.
    MultiTarget { slots: Vec<u8> },
    /// The single-target arm: one banner sized off the target's animation
    /// descriptor at `target + 0x1BC`.
    SingleTarget {
        /// The actor slot whose `+0x1BC` descriptor sizes the banner.
        source_slot: u8,
    },
    /// The party-wide arm (`target == 8` from a monster slot): three HUD
    /// elements and the `ctx[+0x18]` write, then the idle width reset.
    PartyWide,
    /// No target banner - just the idle width reset.
    Idle,
}

/// What one call to `FUN_801E6D84` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerPlan {
    /// Whether the caster banner ran. It runs for every category except
    /// [`CATEGORY_SKIP`], and it is what writes the caster's own animation
    /// descriptor into `DAT_80077284`.
    pub caster_banner: bool,
    /// The selected layout.
    pub layout: BannerLayout,
    /// HUD element ids raised, in the order retail calls `FUN_801D8DE8`.
    pub hud_elements: Vec<u8>,
    /// `ctx[+0x18]` write, if any.
    pub ctx_18: Option<u8>,
    /// The value written to both `DAT_800773AA` and `DAT_800773B2` when the
    /// arm reaches a width write. `None` when the arm returned earlier.
    pub banner_width: Option<i16>,
}

/// Plan the per-action target banner. `FUN_801E6D84`.
///
/// The routine is a three-way branch after the caster banner:
///
/// - **Multi-target** when `target == 9 && ctx[+0x24B] == 0`, or when the
///   action category is `2` and the id is one of
///   [`MULTI_TARGET_ACTION_IDS`]. It counts and lists the living monster
///   slots other than the active one.
/// - **Single-target** when the category is neither `0` nor `4`, and either
///   the target names a monster slot (`3..=7`) or it is `9` with a non-zero
///   override slot.
/// - **Party-wide / idle** otherwise. The party-wide extra (three HUD
///   elements plus `ctx[+0x18] = 6`) needs the active slot to be a monster
///   (`>= 3`) and the target to be `8`.
///
/// `target_anim_width` is what `FUN_80035F04` returns for the selected
/// target's `+0x1BC` descriptor; the banner width is `0x130 - width`. The
/// caller supplies it because the lookup walks the animation pool.
///
/// The battle-action SM's `ActionSeed` raises this plan's `hud_elements`
/// through `BattleActionHost::ui_element` - retail's own placement, the
/// unconditional `jal 0x801e6d84` every category arm of the seed body falls
/// into (`0x801E3028`). See `battle_action::dispatch::raise_target_banner`
/// for the two inputs the engine abstracts (`ctx[+0x24B]` and the descriptor
/// width) and why neither reaches the id list.
///
/// PORT: FUN_801E6D84
/// REF: FUN_80035F04 (animation-descriptor width), FUN_801D8DE8 (HUD element)
pub fn plan_target_banner(inputs: &BannerInputs, target_anim_width: i16) -> BannerPlan {
    if inputs.action_category == CATEGORY_SKIP {
        return BannerPlan {
            caster_banner: false,
            layout: BannerLayout::Skipped,
            hud_elements: Vec::new(),
            ctx_18: None,
            banner_width: None,
        };
    }

    let mut hud = vec![HUD_CASTER_BANNER];

    let forced_multi =
        inputs.action_category == 2 && MULTI_TARGET_ACTION_IDS.contains(&inputs.action_id);
    let all_enemies_multi = inputs.target == TARGET_ALL_ENEMIES && inputs.ctx_override_slot == 0;

    if all_enemies_multi || forced_multi {
        let slots: Vec<u8> = (MONSTER_SLOT_FIRST..MONSTER_SLOT_END)
            .filter(|&slot| {
                slot != inputs.active_slot
                    && inputs.monster_alive[(slot - MONSTER_SLOT_FIRST) as usize]
            })
            .collect();
        return BannerPlan {
            caster_banner: true,
            layout: BannerLayout::MultiTarget { slots },
            hud_elements: hud,
            ctx_18: None,
            banner_width: None,
        };
    }

    if CATEGORY_NO_TARGET_BANNER.contains(&inputs.action_category) {
        return BannerPlan {
            caster_banner: true,
            layout: BannerLayout::Idle,
            hud_elements: hud,
            ctx_18: None,
            banner_width: None,
        };
    }

    // `target - 3 < 5`, i.e. `target` in `3..=7`.
    let named_slot = (MONSTER_SLOT_FIRST..=MONSTER_SLOT_END).contains(&inputs.target);
    let source_slot = if named_slot {
        Some(inputs.target)
    } else if inputs.target == TARGET_ALL_ENEMIES && inputs.ctx_override_slot != 0 {
        Some(inputs.ctx_override_slot)
    } else {
        None
    };

    if let Some(source_slot) = source_slot {
        hud.push(HUD_TARGET_BANNER);
        return BannerPlan {
            caster_banner: true,
            layout: BannerLayout::SingleTarget { source_slot },
            hud_elements: hud,
            ctx_18: None,
            banner_width: Some(BANNER_WIDTH_BASE - target_anim_width),
        };
    }

    let party_wide = inputs.active_slot >= MONSTER_SLOT_FIRST && inputs.target == TARGET_PARTY_WIDE;
    let mut ctx_18 = None;
    if party_wide {
        hud.extend_from_slice(&HUD_PARTY_WIDE);
        ctx_18 = Some(CTX_18_PARTY_WIDE);
    }

    BannerPlan {
        caster_banner: true,
        layout: if party_wide {
            BannerLayout::PartyWide
        } else {
            BannerLayout::Idle
        },
        hud_elements: hud,
        ctx_18,
        banner_width: Some(BANNER_WIDTH_IDLE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel and the disc-side parser each state the record shape; a
    /// silent disagreement would put the parser's records out of phase with the
    /// expander's indexing.
    #[test]
    fn constants_agree_with_the_disc_parser() {
        assert_eq!(CUE_GROUP_STRIDE, legaia_asset::move_power::CUE_GROUP_STRIDE);
        assert_eq!(CUE_ACTOR_FLAG, legaia_asset::move_power::CUE_ACTOR_FLAG);
    }

    fn tables() -> (Vec<u8>, Vec<u8>) {
        // Group 0: empty. Group 1: two effect cues (ids 4, 5).
        // Group 2: one actor cue (id 0x80 | 3) and one effect cue (id 9).
        let mut groups = vec![0u8; CUE_GROUP_STRIDE * 3];
        groups[CUE_GROUP_STRIDE] = 2;
        groups[CUE_GROUP_STRIDE + 1] = 4;
        groups[CUE_GROUP_STRIDE + 2] = 5;
        groups[CUE_GROUP_STRIDE * 2] = 2;
        groups[CUE_GROUP_STRIDE * 2 + 1] = 0x83;
        groups[CUE_GROUP_STRIDE * 2 + 2] = 9;
        let mut sfx = vec![0u8; 16];
        sfx[4] = 0x21;
        // id 5 and id 9 have no sound.
        (groups, sfx)
    }

    #[test]
    fn empty_group_still_writes_the_actor_fields() {
        let (groups, sfx_map) = tables();
        let t = CueTables {
            groups: &groups,
            sfx_map: &sfx_map,
        };
        let out = expand_cue_group(CUE_TINT_NEUTRAL, 0x1234, 0, 0, &t);
        assert!(out.spawns.is_empty());
        assert_eq!(out.actor_state, 0x1234);
        assert_eq!(out.actor_flags, Some(0x2000));
    }

    #[test]
    fn skip_state_suppresses_the_flags_write() {
        let (groups, sfx_map) = tables();
        let t = CueTables {
            groups: &groups,
            sfx_map: &sfx_map,
        };
        let out = expand_cue_group(CUE_TINT_NEUTRAL, CUE_ACTOR_STATE_SKIP, 0, 0, &t);
        assert_eq!(out.actor_flags, None);
    }

    #[test]
    fn effect_cues_carry_their_sfx_only_when_the_map_byte_is_set() {
        let (groups, sfx_map) = tables();
        let t = CueTables {
            groups: &groups,
            sfx_map: &sfx_map,
        };
        let out = expand_cue_group(CUE_TINT_NEUTRAL, 0, 0, 1, &t);
        assert_eq!(
            out.spawns,
            vec![
                CueSpawn::Effect {
                    id: 4,
                    sfx: Some(0x21),
                    effect_index: 4,
                    tint: None
                },
                CueSpawn::Effect {
                    id: 5,
                    sfx: None,
                    effect_index: 5,
                    tint: None
                },
            ]
        );
    }

    #[test]
    fn actor_cues_strip_the_flag_and_use_the_unbiased_yaw() {
        let (groups, sfx_map) = tables();
        let t = CueTables {
            groups: &groups,
            sfx_map: &sfx_map,
        };
        let out = expand_cue_group(0x00FF_0000, 0, 0x400, 2, &t);
        assert_eq!(out.spawns[0], CueSpawn::Actor { id: 3, yaw: 0x400 });
        assert_eq!(
            out.spawns[1],
            CueSpawn::Effect {
                id: 9,
                sfx: None,
                effect_index: 9,
                tint: Some(0x00FF_0000 | CUE_TINT_MODE)
            }
        );
    }

    #[test]
    fn the_three_computed_sites_move_with_the_tier() {
        // class 0: group == tier. class 1: tier + 1. class 2: tier + 3.
        for tier in 0..4u8 {
            assert_eq!(cue_group_for(0, tier).unwrap().group, tier);
            assert_eq!(cue_group_for(1, tier).unwrap().group, tier + 1);
            assert_eq!(cue_group_for(2, tier).unwrap().group, tier + 3);
        }
    }

    #[test]
    fn the_literal_sites_ignore_the_tier_entirely() {
        for tier in [0u8, 1, 9, 0xFF] {
            assert_eq!(cue_group_for(3, tier).unwrap().group, 5);
            assert_eq!(cue_group_for(4, tier).unwrap().group, 6);
            assert_eq!(cue_group_for(5, tier).unwrap().group, 7);
            assert_eq!(cue_group_for(8, tier).unwrap().group, 0xC);
        }
    }

    #[test]
    fn class_seven_has_one_site_per_tier_and_none_outside_one_to_four() {
        for (tier, group) in [(1u8, 8u8), (2, 9), (3, 0xA), (4, 0xB)] {
            assert_eq!(cue_group_for(7, tier).unwrap().group, group);
        }
        assert!(cue_group_for(7, 0).is_none());
        assert!(cue_group_for(7, 5).is_none());
    }

    #[test]
    fn only_the_class_one_loop_arm_is_per_target() {
        assert!(cue_group_for(1, 0).unwrap().per_target);
        for class in [0u8, 2, 3, 4, 5, 8] {
            assert!(
                !cue_group_for(class, 1).unwrap().per_target,
                "class {class}"
            );
        }
    }

    #[test]
    fn the_classes_with_no_expander_branch_select_no_site() {
        // Class 6's inner table only bumps counters; 9..=0xE and 0x82 have no
        // `jal`; the spell-table routing bytes are above the arms entirely.
        for class in [6u8, 9, 0xA, 0xB, 0xC, 0xD, 0xE, 0x14, 0x32, 0x63, 0x82] {
            assert!(cue_group_for(class, 1).is_none(), "class {class:#x}");
        }
    }

    #[test]
    fn revive_is_the_one_site_that_passes_the_skip_state() {
        let revive = cue_group_for(4, 0).unwrap();
        assert_eq!(revive.actor_state, CUE_ACTOR_STATE_SKIP);
        // ... which is what suppresses the `+0x0C` follow-up write.
        let groups = vec![0u8; CUE_GROUP_STRIDE * 8];
        let sfx = vec![0u8; 16];
        let t = CueTables {
            groups: &groups,
            sfx_map: &sfx,
        };
        let out = expand_cue_group(revive.tint, revive.actor_state, 0, revive.group, &t);
        assert_eq!(out.actor_flags, None);
        // Every other site writes it.
        for class in [0u8, 1, 2, 3, 5, 8] {
            let s = cue_group_for(class, 1).unwrap();
            let out = expand_cue_group(s.tint, s.actor_state, 0, s.group, &t);
            assert_eq!(out.actor_flags, Some(0x2000), "class {class}");
        }
    }

    #[test]
    fn only_the_two_hp_restore_sites_pass_the_neutral_tint() {
        assert_eq!(cue_group_for(0, 0).unwrap().tint, CUE_TINT_NEUTRAL);
        assert_eq!(cue_group_for(1, 0).unwrap().tint, CUE_TINT_NEUTRAL);
        // The class-7 buff sites reuse the neutral word for tiers 1 and 2 but
        // not 3 and 4 - so "neutral" is per-site, not per-class.
        assert_eq!(cue_group_for(7, 1).unwrap().tint, CUE_TINT_NEUTRAL);
        assert_ne!(cue_group_for(7, 3).unwrap().tint, CUE_TINT_NEUTRAL);
        for class in [2u8, 3, 4, 5, 8] {
            assert_ne!(cue_group_for(class, 1).unwrap().tint, CUE_TINT_NEUTRAL);
        }
    }

    fn inputs() -> BannerInputs {
        BannerInputs {
            active_slot: 0,
            action_category: 1,
            action_id: 0x20,
            target: 3,
            ctx_override_slot: 0,
            monster_alive: [true, true, false, true],
        }
    }

    #[test]
    fn category_five_skips_everything() {
        let plan = plan_target_banner(
            &BannerInputs {
                action_category: CATEGORY_SKIP,
                ..inputs()
            },
            0,
        );
        assert_eq!(plan.layout, BannerLayout::Skipped);
        assert!(!plan.caster_banner);
        assert!(plan.hud_elements.is_empty());
    }

    #[test]
    fn all_enemies_takes_the_multi_arm_and_skips_the_active_slot() {
        let plan = plan_target_banner(
            &BannerInputs {
                active_slot: 4,
                target: TARGET_ALL_ENEMIES,
                ..inputs()
            },
            0,
        );
        // Slots 3..=6 alive as [true, true, false, true]; slot 4 is active.
        assert_eq!(plan.layout, BannerLayout::MultiTarget { slots: vec![3, 6] });
        assert_eq!(plan.hud_elements, vec![HUD_CASTER_BANNER]);
    }

    #[test]
    fn the_three_forced_ids_take_the_multi_arm_from_category_two() {
        for id in MULTI_TARGET_ACTION_IDS {
            let plan = plan_target_banner(
                &BannerInputs {
                    action_category: 2,
                    action_id: id,
                    target: 3,
                    ..inputs()
                },
                0,
            );
            assert!(matches!(plan.layout, BannerLayout::MultiTarget { .. }));
        }
        // Same ids under a different category do not force it.
        let plan = plan_target_banner(
            &BannerInputs {
                action_category: 1,
                action_id: MULTI_TARGET_ACTION_IDS[0],
                ..inputs()
            },
            0,
        );
        assert!(matches!(plan.layout, BannerLayout::SingleTarget { .. }));
    }

    #[test]
    fn all_enemies_with_an_override_slot_becomes_single_target() {
        let plan = plan_target_banner(
            &BannerInputs {
                target: TARGET_ALL_ENEMIES,
                ctx_override_slot: 5,
                ..inputs()
            },
            0x30,
        );
        assert_eq!(plan.layout, BannerLayout::SingleTarget { source_slot: 5 });
        assert_eq!(
            plan.hud_elements,
            vec![HUD_CASTER_BANNER, HUD_TARGET_BANNER]
        );
        assert_eq!(plan.banner_width, Some(BANNER_WIDTH_BASE - 0x30));
    }

    #[test]
    fn categories_zero_and_four_stop_after_the_caster_banner() {
        for category in CATEGORY_NO_TARGET_BANNER {
            let plan = plan_target_banner(
                &BannerInputs {
                    action_category: category,
                    ..inputs()
                },
                0,
            );
            assert_eq!(plan.layout, BannerLayout::Idle);
            assert_eq!(plan.banner_width, None, "the arm returns before the width");
        }
    }

    #[test]
    fn party_wide_needs_a_monster_caster_and_target_eight() {
        let plan = plan_target_banner(
            &BannerInputs {
                active_slot: 3,
                target: TARGET_PARTY_WIDE,
                ..inputs()
            },
            0,
        );
        assert_eq!(plan.layout, BannerLayout::PartyWide);
        assert_eq!(plan.hud_elements, vec![HUD_CASTER_BANNER, 0x06, 0x4E, 0x4F]);
        assert_eq!(plan.ctx_18, Some(CTX_18_PARTY_WIDE));
        assert_eq!(plan.banner_width, Some(BANNER_WIDTH_IDLE));

        // Party caster, same target: idle, no extra HUD.
        let plan = plan_target_banner(
            &BannerInputs {
                active_slot: 1,
                target: TARGET_PARTY_WIDE,
                ..inputs()
            },
            0,
        );
        assert_eq!(plan.layout, BannerLayout::Idle);
        assert_eq!(plan.hud_elements, vec![HUD_CASTER_BANNER]);
        assert_eq!(plan.ctx_18, None);
    }
}
