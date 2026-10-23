//! Screen chrome gates and labels: breadcrumb tabs, panel visibility, the plaque element badge
//! and the encounter / battle banners.

use super::*;

/// Does the frame carry the `Begin` breadcrumb tab at `(16, 14)` with the
/// plaque slid behind it to `(68, 14)`?
///
/// Step 1 turns the round prompt's `Begin` chip into the tab (record 1,
/// `(104, 88)` to `(16, 14)`, the gold plate class) and drops the plaque
/// (record 26) in beside it; every later menu step keeps both with a snap
/// (`01/3 1A/3`). The item window continues the trail as `Begin | <name>
/// | Item` through its own breadcrumb builder, so it is excluded here.
///
/// The arts-entry screen (`0x50`, step 9: `01/3 .. 1A/3`) keeps both, with
/// the `Attack` tab after them ([`battle_breadcrumb_third_tab`]) - the
/// `arts_bar_*` captures read `Begin | Vahn | Attack` over the chips.
///
/// The commit-confirm screen `0x6E` keeps the tab and nothing behind it: the
/// plaque has gone back up (`party_basic_attack_vs_gobu_gobu` holds record
/// `0x1A` at `(68, -24)` and a `Begin` plate at `(16, 14)`, `36` wide, over
/// the `Begin` / `Reselect` chips), so [`battle_begin_tab_alone`] is the
/// draw's question there.
pub fn battle_begin_tab_visible(world: &legaia_engine_core::world::World) -> bool {
    matches!(
        battle_command_surface(world),
        Some(
            CommandSurface::Ring
                | CommandSurface::AttackMode
                | CommandSurface::Targeting
                | CommandSurface::ArtsInput
                | CommandSurface::ArtsList
                | CommandSurface::SpellBrowse
                | CommandSurface::SpellTarget(_)
        )
    ) || battle_begin_tab_alone(world)
}

/// Is the frame the commit-confirm screen `0x6E`, where the `Begin` tab
/// stands at `(16, 14)` with no plaque beside it?
pub fn battle_begin_tab_alone(world: &legaia_engine_core::world::World) -> bool {
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    world.mode == legaia_engine_mode::mode::SceneMode::Battle
        && battle_command_surface(world) == Some(CommandSurface::Other)
        && world
            .battle
            .command
            .as_ref()
            .is_some_and(|c| matches!(c.phase, CommandPhase::CommitConfirm { .. }))
}

/// The third breadcrumb tab behind `Begin | <name>`, or `None`.
///
/// Choosing a ring arm glides that arm's chip onto the trail: record `0x0D`
/// (`Attack`, the SCUS label `0x8007B674`) opens at the attack-mode prompt
/// (`0x78`, step `0x2A`: `0D/0`) and stays through the target cursor
/// (`0x5A`: `0D/3`) and the arts entry (`0x50`: `0D/0`); record `0x0E` (the
/// magic arm, whose label case `0xE` of `FUN_801D8DE8` points at the
/// member's Ra-Seru name) opens with the spell window (`0x46`, step 7:
/// `0E/0`) and stays through its target step. Both rest at
/// `x = width(name) + 0x54` (`0x801D39A0..0x801D39AC` for `0x0D`,
/// `0x801D3968..0x801D3974` for `0x0E`), i.e. abutting the plaque. The item
/// window draws its own trail (record `0x0C`), so it is not named here.
pub fn battle_breadcrumb_third_tab(world: &legaia_engine_core::world::World) -> Option<String> {
    use legaia_engine_battle_menus::battle_input::BattleCommand;
    match battle_command_surface(world)? {
        CommandSurface::AttackMode
        | CommandSurface::Targeting
        | CommandSurface::ArtsInput
        | CommandSurface::ArtsList => Some(BattleCommand::Attack.label().to_string()),
        // The dome's Ra-Seru list is its own projection (`dome_command_chips`
        // carries its arm word); the battle's resolver is not its source.
        CommandSurface::SpellBrowse | CommandSurface::SpellTarget(_)
            if world.mode != legaia_engine_mode::mode::SceneMode::MuscleDome =>
        {
            let slot = command_entry_actor(world)?;
            Some(battle_magic_chip(world, slot).0)
        }
        _ => None,
    }
}

/// The first action state past the item band's panels: the Done band's
/// `0x51` hold sends them off as its countdown runs out (`ctx[+0x18] = 6`
/// read back at `0x801E6178..0x801E61B0`), so they are up through `0x51`.
const ITEM_PANELS_CLOSE_STATE: u8 = 0x52;

/// Whether action state `s` runs ahead of the Done band's close - every
/// state below [`ITEM_PANELS_CLOSE_STATE`], and the capture bands
/// `0x68..=0x71`, whose bytes sit above it but which hand off into the Done
/// band (`0x70` exits to `0x50`). A Mystic Circle capture holds `0x70` with
/// the seed opener's panels up (`cort_mystic_circle_mid_cast`, target `8`).
fn panels_still_open(s: u8) -> bool {
    use legaia_engine_battle_vm::battle_action::ActionState;
    s < ITEM_PANELS_CLOSE_STATE
        || (ActionState::CaptureStart.as_byte()..=ActionState::MagicCaptureFinalize.as_byte())
            .contains(&s)
}

/// Are the resting roster panels (records 6 / 78 / 79) on screen this
/// frame?
///
/// Up at the round prompt (step 0), parked by the ring (step 1, mode 1),
/// **back up** while the item or magic window is browsed (steps 5 and 7),
/// parked again for their target steps (`0x12` / `0x1B`), and raised in an
/// action by two openers, each for a party-wide target (`t2 == 8`), each
/// leaving `ctx[+0x18] = 6` for the Done hold's close:
///
/// * the seed's plate routine `FUN_801E6D84` (every category arm of `0x0C`
///   ends in it), only for a **monster** caster: `sltiu v0,v0,3` on the
///   caster seat, then `t2 == 8`, then records 6 / `0x4E` / `0x4F`
///   (`0x801E7038..0x801E7080`), and only on the arms that reach it past the
///   Run / Arts / Spirit returns (`0x801E6DF0`, `0x801E6FA0..0x801E6FA8`);
/// * the item band's `0x3E` arm (`0x801E404C`), reached from one branch at
///   `0x801E3E88` off its non-gauge-extend path, for any caster.
///
/// A party member's magic cast on the whole party therefore raises none at
/// the seed: `orb_summon_mid_cast` holds `ctx[+0x18] = 0` with Orb's
/// `+0x1DD` at `8`, and its frame shows the scene where the panels would
/// sit.
///
/// A cast module can raise them itself, the third opener: PROT 0911 (Orb)
/// arm 9's pass (`FUN_801D8DE8(6, 0)` and `(0x4E + i, 0)`, then
/// `ctx[+0x18] = 6`, `0x801F7E64..0x801F7ED4`) puts them up under the heal,
/// and the Done hold closes them like the others
/// ([`legaia_engine_core::world::BattleState::module_panels_glide`]).
pub fn battle_panels_visible(world: &legaia_engine_core::world::World) -> bool {
    match battle_hud_phase(world) {
        BattleHudPhase::RoundPrompt => true,
        // The ring parks the panels on a glide (step 1's `06/1 4E/1 4F/1`):
        // they are still on their way down while it runs.
        BattleHudPhase::CommandEntry if ring_open_glide(world).is_some() => true,
        BattleHudPhase::CommandEntry => matches!(
            battle_command_surface(world),
            Some(CommandSurface::ItemBrowse | CommandSurface::SpellBrowse)
        ),
        // The two `t2 == 8` openers (see above); either keeps the panels up
        // until the Done band's `0x51` hold ends.
        BattleHudPhase::Action if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome => {
            false
        }
        BattleHudPhase::Action => {
            let a = world.battle_ctx.active_actor;
            world.actors.get(a as usize).is_some_and(|actor| {
                use legaia_engine_battle_vm::battle_action::{ActionCategory, ActionState};
                let s = world.battle_ctx.action_state;
                let cat = actor.battle.action_category;
                let party_wide = actor.battle.active_target
                    == legaia_engine_battle_vm::battle_cue_group::TARGET_PARTY_WIDE;
                let open = panels_still_open(s);
                let monster_seed =
                    a >= party_count(world) as u8 && seed_plates_reach_the_target_arm(cat);
                let item_band =
                    cat == ActionCategory::Item.as_byte() && s >= ActionState::SpiritFire as u8;
                let module_raise = world.battle.module_panels_glide.is_some();
                open && (module_raise || (party_wide && (monster_seed || item_band)))
            })
        }
        BattleHudPhase::Idle => false,
    }
}

/// Element-badge index the actor-name plaque wears in front of the name, or
/// `None` for an actor that carries no badge.
///
/// Retail's plaque grows its interior by `20 + 5` when the actor has one
/// (`battle_chrome::name_plaque`), and the eight badge records `0x8B..=0x92`
/// are the only strip that fits that 20x12 slot.
///
/// **The selector is not code - it is markup in the monster's own name.**
/// No dumped caller computes a badge id because nothing computes one: the
/// archive name begins with a caret plus a letter and the plaque draws badge
/// `letter - 'A'`, drawing nothing at all when the name carries no escape
/// (`docs/subsystems/battle.md`, the element-badge section). So this reads
/// [`legaia_engine_battle::monster_catalog::MonsterDef::plaque_badge`], which
/// `legaia_actor_formats::monster_archive` carries straight off the name bytes.
///
/// Two things this deliberately does **not** do, both of which the earlier
/// element-byte selector got wrong: it does not badge every monster (only a
/// minority of the populated records carry an escape at all, and the rest
/// wear no badge in retail), and it does not index the strip in `+0x1D`
/// element order (the caret order is a different permutation -
/// `element -> caret index` is `[4, 3, 0, 2, 1, 5, 6, 7]`). A party member
/// gets no badge either way - the captured plaques that carry one are the
/// monster frames (`Gimard`), and the party ones (`Vahn`, `Noa`) do not.
pub fn battle_plaque_element_badge(world: &legaia_engine_core::world::World) -> Option<u8> {
    // A dome plaque's slot is a fighter seat, not a battle actor.
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        return None;
    }
    let (slot, _) = battle_active_actor(world)?;
    let pc = (world.party.party_count.clamp(1, 3) as usize).min(world.actors.len());
    if (slot as usize) < pc {
        return None;
    }
    let actor = world.actors.get(slot as usize)?;
    let def = world.tables.monster_catalog.get(actor.battle_monster_id?)?;
    def.plaque_badge
        .filter(|b| usize::from(*b) < BATTLE_PLAQUE_BADGE_COUNT)
}

/// Records in the element-badge strip (`0x8B..=0x92`) - the bound the
/// caret letter `A..=H` addresses.
pub const BATTLE_PLAQUE_BADGE_COUNT: usize = 8;

/// The caret-letter order the badge strip is drawn in, as a map from the
/// record's `+0x1D` **element id** to that element's strip index.
///
/// Retail never performs this mapping - the name escape names the badge
/// directly - but the two orders differ, so anything that has an element id
/// and wants the badge art (a doc census, an editor preview) has to go
/// through it rather than substituting one for the other. The letters are
/// A=Fire(2), B=Thunder(4), C=Wind(3), D=Water(1), E=Earth(0), F=Light(5),
/// G=Dark(6), H=Evil/neutral(7); inverting that gives the table below.
pub const ELEMENT_TO_PLAQUE_BADGE: [u8; BATTLE_PLAQUE_BADGE_COUNT] = [4, 3, 0, 2, 1, 5, 6, 7];

/// Is the port's encounter-transition banner enabled?
///
/// The "ENCOUNTER!" head line has no retail counterpart - retail's
/// `Field -> Battle` edge draws no banner at all - so it is off by default
/// and rides the same shared toggle as the diagnostic HUD rows:
/// `LEGAIA_DIAG_HUD` set to anything but `0` / empty. Reading the
/// environment keeps both hosts on one answer; on wasm the variable never
/// exists and the banner stays off.
pub fn encounter_banner_enabled() -> bool {
    std::env::var("LEGAIA_DIAG_HUD")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// Formation label for the encounter-transition banner, reusing the battle
/// HUD's monster naming: the live catalog name when the formation resolved
/// one, `M<n>` otherwise. Slots with `max_hp == 0` (unseeded / cleared) are
/// skipped, so a formation that has not resolved HP yet yields an empty label
/// and the banner shows only its "ENCOUNTER!" head line. Shared by both hosts
/// (armed on each `Field -> Battle` mode edge).
///
/// The banner itself is a port invention - retail shows nothing on the
/// `Field -> Battle` edge - so hosts arm it only when
/// [`encounter_banner_enabled`] says so.
pub fn encounter_banner_label(world: &legaia_engine_core::world::World) -> String {
    let pc = (world.party.party_count.clamp(1, 3) as usize).min(world.actors.len());
    // `World::actors` is the fixed 64-slot table, not a battle-sized list;
    // a formation seats at most 5 monsters directly after the party
    // (`World::enter_battle`), so only those slots can be formation members.
    const MAX_FORMATION_MONSTERS: usize = 5;
    let mut names: Vec<String> = Vec::new();
    for (i, a) in world
        .actors
        .iter()
        .enumerate()
        .skip(pc)
        .take(MAX_FORMATION_MONSTERS)
    {
        if a.battle.max_hp == 0 {
            continue;
        }
        let name = a
            .battle_monster_id
            .and_then(|id| world.tables.monster_catalog.get(id))
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("M{}", i - pc + 1));
        names.push(name);
    }
    names.join("  ")
}

/// The line retail's top-of-screen message banner holds this frame, if any -
/// the one read both play hosts draw into that widget.
///
/// Three sources, in precedence order:
///
/// 1. the **battle message banner** - screen element `0x59` (a Seru absorbed
///    on a killing blow) or `0x65` (a Seru spell's magic level went up), live
///    from its raise to its unload ([`legaia_engine_core::world::BattleMessageBanner`]).
///    Battle mode only: the Done band retires it, and a line a battle left
///    standing must not follow the party back into the field;
/// 2. the post-battle **level-up** line;
/// 3. the **Seru-capture** line.
///
/// The last two are deliberately not gated on battle mode: the port grants XP
/// after the mode has flipped back to Field, where retail still shows the
/// result screen, so a battle-mode gate would leave them wired and never
/// drawn.
///
/// The tactical-arts learn line ([`legaia_engine_menus::party_state::PartyState::current_art_banner`])
/// is **not** a source: retail announces a newly learned art with the
/// `NEW ARTS!!` sprite banner (`World::battle_arts_banner_quads`), not with a
/// line in this widget, and the engine's `Learned <art>!` string is its own.
///
/// Before this read existed each host carried a copy of sources 2 and 3 and
/// neither drew source 1, so no host ever showed the absorb or magic-level
/// line.
pub fn battle_banner_message(world: &legaia_engine_core::world::World) -> Option<String> {
    if world.mode == legaia_engine_mode::mode::SceneMode::Battle
        && let Some(b) = &world.battle.message_banner
    {
        return Some(b.text.clone());
    }
    if let Some(b) = &world.party.current_level_up_banner {
        // Name the character, not their roster ordinal - `P3` is an index
        // only this codebase knows. `char_id` is the ROSTER slot the level-up
        // applier wrote, so it indexes `roster.members` directly.
        let who = world
            .party
            .roster
            .members
            .get(b.char_id as usize)
            .map(|r| r.name())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("P{}", b.char_id + 1));
        return Some(format!(
            "LEVEL UP!  {who} -> LV {}\nHP +{}  MP +{}",
            b.new_level, b.hp_gained, b.mp_gained
        ));
    }
    world
        .party
        .current_capture_banner
        .as_ref()
        .and_then(|b| b.current_banner())
}
