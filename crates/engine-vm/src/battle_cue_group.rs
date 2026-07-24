//! Two battle-overlay kernels that turn a committed action into presentation
//! work: the cue-group expander that spawns an action's effect/SFX set, and
//! the target-banner planner that decides which HUD banner the action gets.
//!
//! PORT: FUN_801E22C8
//! PORT: FUN_801E6D84
//!
//! NOT WIRED: the engine has no effect-spawn pool or battle HUD banner stack
//! to hand these plans to; both are the decision half only.
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
/// PORT: FUN_801E22C8
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

/// Action category `5` short-circuits the whole routine.
pub const CATEGORY_SKIP: u8 = 5;

/// Action categories `0` and `4` skip the target arm (the caster banner has
/// already been raised by then).
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
