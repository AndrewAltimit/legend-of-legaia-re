use super::*;

/// The member's EXP share the results sequencer hands out, `gp+0xA04`
/// (`sw s6,0xA04(gp)` at `0x8004F684`).
pub(super) const END_XP_SHARE: u32 = 0x8007_BD1C;

/// Take a results-frame capture's rewards back off the party it seeds.
///
/// The results sequencer `FUN_8004E568` grants the fight's EXP and runs the
/// level-up applier `FUN_801E9504` when it opens the results frame, so a
/// capture on that frame or after it (`SpanGate::Results` / `Exit`) holds the
/// party past the grant. The seed replays the fight from that party, and the
/// engine grants again on its own results frame: `noa_levelup_banner`'s Noa,
/// already level 3 in the capture, gained nothing the second time, and the
/// engine frame carried no "level increased" line.
///
/// Every living member (`+0x14C > 0` on its seat) loses the share. A member
/// the applier levelled is recognised by its record stat window
/// (`+0x11C` HP max, `+0x11E` MP max, `+0x122..+0x12D` the six stats)
/// standing apart from the live window it is mirrored into one phase later
/// (`+0x104`, `+0x108`, `+0x110..+0x11B` -
/// `docs/subsystems/level-up.md#phase-split-multi-frame-writes`); that member
/// gets the live values back in its record window and its level byte
/// `+0x130` one lower. A capture past the live copy keeps the growth, which
/// the engine's grant does not repeat because the level stays where it is.
pub fn ungrant_results_rewards(
    save: &mut legaia_save::SaveFile,
    battle: &RetailBattle,
    ram: &[u8],
) {
    if !matches!(
        battle.span_gate,
        SpanGate::Results { .. } | SpanGate::Exit { .. }
    ) {
        return;
    }
    let share = game_anchors::u32_at(ram, END_XP_SHARE);
    for (seat, &char_id) in battle.seat_chars.iter().enumerate() {
        let alive = battle
            .party
            .get(seat)
            .and_then(|c| c.as_ref())
            .is_some_and(|c| c.hp > 0);
        let Some(rec) = usize::from(char_id)
            .checked_sub(1)
            .and_then(|i| save.party.members.get_mut(i))
        else {
            continue;
        };
        if alive {
            rec.set_cumulative_xp(rec.cumulative_xp().saturating_sub(share));
        }
        // (record window, live window) halfword pairs.
        const PAIRS: [(usize, usize); 8] = [
            (0x11C, 0x104),
            (0x11E, 0x108),
            (0x122, 0x110),
            (0x124, 0x112),
            (0x126, 0x114),
            (0x128, 0x116),
            (0x12A, 0x118),
            (0x12C, 0x11A),
        ];
        let raw = &mut rec.raw;
        if raw.len() < 0x130 || PAIRS.iter().all(|&(r, l)| raw[r..r + 2] == raw[l..l + 2]) {
            continue;
        }
        for (r, l) in PAIRS {
            raw.copy_within(l..l + 2, r);
        }
        let level = rec.level();
        rec.set_level(level.saturating_sub(1).max(1));
    }
}

/// The magic-level-increased screen element `FUN_801E70BC` raises and stores
/// on `ctx[+0x26]`.
pub(super) const MAGIC_LEVEL_BANNER: u8 = 0x65;

/// Take a cast capture's magic level-up back off the caster it seeds.
///
/// The summon return's level check (`FUN_801E70BC`) bumps the cast spell's
/// level byte (`record[+0x161 + slot]`) and raises the "magic level
/// increased" banner, so a capture taken after it in the same action
/// ([`RetailBattle::magic_level_up`]) holds the caster already a level up,
/// with XP past the old threshold. The seed replays the cast from that
/// record, and the engine's check then compares the XP against the **next**
/// level's threshold: `shiny_refactor_gimard_levelup` levelled nothing the
/// second time and the engine frame carried no banner. The level goes back
/// one; the XP stays, and still clears the old threshold, so the replay's
/// own check levels it again.
pub fn ungrant_magic_level_up(save: &mut legaia_save::SaveFile, battle: &RetailBattle) {
    if !battle.magic_level_up || battle.queued_category != 2 {
        return;
    }
    let Some(&char_id) = battle.seat_chars.get(usize::from(battle.active_actor)) else {
        return;
    };
    let Some(rec) = usize::from(char_id)
        .checked_sub(1)
        .and_then(|i| save.party.members.get_mut(i))
    else {
        return;
    };
    let mut list = rec.spell_list();
    let count = usize::from(list.count).min(list.ids.len());
    if let Some(at) = list.ids[..count]
        .iter()
        .position(|&id| id == battle.queued_action)
        && list.levels[at] > 1
    {
        list.levels[at] -= 1;
        rec.set_spell_list(list);
    }
}

/// Take spell `spell_id` back off a record's list - the inverse of the
/// Done band's prepend (`legaia_engine_core::magic_xp::learn_spell_prepend`):
/// ids, levels and the parallel XP words above it shift down one. A list
/// without the spell is left alone.
pub(super) fn unlearn_spell(record: &mut legaia_save::CharacterRecord, spell_id: u8) {
    const SPELL_XP_OFFSET: usize = 0x8;
    let mut list = record.spell_list();
    let count = usize::from(list.count).min(list.ids.len());
    let Some(at) = list.ids[..count].iter().position(|&id| id == spell_id) else {
        return;
    };
    for i in at..count - 1 {
        list.ids[i] = list.ids[i + 1];
        list.levels[i] = list.levels[i + 1];
        let src = SPELL_XP_OFFSET + (i + 1) * 4;
        let dst = SPELL_XP_OFFSET + i * 4;
        record.raw.copy_within(src..src + 4, dst);
    }
    list.ids[count - 1] = 0;
    list.levels[count - 1] = 0;
    let last = SPELL_XP_OFFSET + (count - 1) * 4;
    record.raw[last..last + 4].fill(0);
    list.count -= 1;
    record.set_spell_list(list);
}
