//! Party load / save, the scene save permission and the pause menu's entry gates.
//!
//! The full LGSF save-file round trip over these (`save_full` / `load_full`)
//! is `legaia_engine_save_state::WorldSaveState`.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// Scratchpad `_DAT_1F800394` bit 10, the system lock: while set the player
/// tick skips the pad controller outright (`0x801D16A8..0x801D16B8`).
pub const FIELD_SYSTEM_LOCK_BIT: u32 = 0x400;
/// Scratchpad `_DAT_1F800394` bit 27, the menu lock: a menu-button press
/// buzzes instead of opening (`0x801D02C0..0x801D02E4`).
pub const FIELD_MENU_LOCK_BIT: u32 = 0x0800_0000;
/// The locked press's deny buzz (`li a0,0x23` at `0x801D02DC`).
pub const FIELD_MENU_DENY_CUE: i16 = 0x23;

impl World {
    /// Whether actor slots `0..party_count` hold the party's own combatants,
    /// so their HP / MP mirrors are live state a save must fold back into
    /// the records: in a battle (the seats `enter_battle` builds), and on the
    /// title screen, where no scene owns the table and a synthetic host
    /// drives the mirrors directly. Every other mode runs over a scene's
    /// actor table (field, overworld, the pause menu and minigames that
    /// suspend one), whose slots past the walking player are not the party.
    pub(crate) fn party_actors_are_party(&self) -> bool {
        matches!(self.mode, SceneMode::Battle | SceneMode::Title)
    }

    /// Whether projecting a party record onto actor `slot`
    /// ([`Self::load_party`], [`Self::set_active_party`]) may raise the slot.
    ///
    /// On a field or the overworld the actor table is not the party: the
    /// walking player's slot (slot 0 until a host installs another) is the
    /// party's, and every other slot belongs to the scene - spawned records,
    /// clones, effects, and the idle slots `init_scene_animations` leaves
    /// pre-bound to scene-pack meshes. Raising one of those spawned a phantom
    /// actor: drawn at the origin with whatever mesh the slot was pre-bound to
    /// (uru's sky and cliff pack across the frame after a card load of a
    /// four-member save), and held out of the spawn allocator's free list.
    /// Retail's card load copies the records into `0x80084708` and touches no
    /// actor; a battle seats its party itself ([`Self::enter_battle`]). The
    /// HP / MP mirrors are still written - only the raise is withheld.
    pub(crate) fn party_mirror_activates(&self, slot: usize) -> bool {
        if !matches!(self.mode, SceneMode::Field | SceneMode::WorldMap) {
            return true;
        }
        usize::from(self.player_actor_slot.unwrap_or(0)) == slot
    }

    /// Load a `Party` (per-character roster) into the world's actor table.
    ///
    /// Per-character record 0 maps to actor slot 0, record 1 to slot 1, …
    /// up to `party.len()` (capped by `MAX_ACTORS`). For each loaded slot
    /// the world:
    ///
    /// - activates the actor - except on a field or the overworld, where
    ///   only the walking player's slot is the party's
    ///   ([`Self::party_mirror_activates`]),
    /// - copies HP / MP from the record's [`HpMpSp`] block into the
    ///   `BattleActor` mirrors,
    /// - stows the full record bytes via [`legaia_engine_menus::party_state::PartyState::roster`] for later
    ///   round-trip via [`World::save_party`].
    ///
    /// The `legaia-save` crate's [`legaia_save::CharacterRecord::parse`] is
    /// the lossless deserializer; this method is the runtime-side glue that
    /// projects the persistent record into the per-VM actor state.
    ///
    /// [`HpMpSp`]: legaia_save::HpMpSp
    pub fn load_party(&mut self, party: legaia_save::Party) {
        // A loaded roster starts from no party-op history.
        self.party.field_list_emptied = false;
        let n = party.members.len().min(self.actors.len());
        for (slot, rec) in party.members.iter().take(n).enumerate() {
            let hms = rec.hp_mp_sp();
            let activate = self.party_mirror_activates(slot);
            let a = &mut self.actors[slot];
            if activate {
                a.active = true;
            }
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            // Seed the per-slot turn-order SPD from the record's live stats so
            // a battle's next-actor selector can run the initiative scheme.
            // A zeroed record leaves SPD at 0 -> round-robin fallback.
            if let Some(s) = self.battle.speed.get_mut(slot) {
                *s = rec.live_stats().spd;
            }
        }
        self.party.party_count = n as u8;
        self.party.roster = party;
        // Hydrate the level-up tracker's per-slot cumulative XP and level
        // from the installed records. Without this the tracker keeps its
        // default 0-XP / level-1 state even when the record has the party
        // deep into the game, and the next grant would re-run the whole
        // curve from L1. Level is the record's `+0x130` byte - the same cell
        // for engine LGSF saves and records lifted from retail cards.
        for (slot, rec) in self.party.roster.members.iter().enumerate() {
            if slot < self.party.level_up_tracker.level.len() {
                self.party.level_up_tracker.xp[slot] = rec.cumulative_xp();
                self.party.level_up_tracker.level[slot] = rec.level().max(1);
            }
        }
        // Adopt each record's stored display name (`+0x2A7`) so a loaded save's
        // custom names reach the dialog renderer's `0xC1 XX` substitutions.
        //
        // Non-shrinking and skip-empty on purpose: a cold-boot save has a
        // one-member roster, and truncating here would drop the Noa / Gala /
        // Terra defaults `seed_starting_party` installs for the slots that have
        // not joined yet.
        for (slot, rec) in self.party.roster.members.iter().enumerate() {
            let name = rec.name();
            if name.is_empty() {
                continue;
            }
            if self.party.party_names.len() <= slot {
                self.party.party_names.resize(slot + 1, String::new());
            }
            self.party.party_names[slot] = name;
        }
    }

    /// Capture the world's current actor state back into a `Party`. The
    /// roster bytes are returned verbatim except for the HP / MP / max-HP
    /// fields, which are resynced from the live `BattleActor` mirrors so
    /// in-battle damage / heals end up in the saved record.
    ///
    /// Round-trip: `world.load_party(p); world.save_party() == p` modulo
    /// the HP/MP resync (which is a no-op when no battle has run yet).
    pub fn save_party(&mut self) -> legaia_save::Party {
        // Outside a fight the records are the pools: retail keeps a member's
        // HP / MP only at `0x80084708 + n*0x414`, and every field heal, the
        // inn's `4C 82` restore and the card load write the record first. The
        // actor table there is the scene's - slot 0 walks, every other slot
        // is a P1 placement, clone or effect a scene script may re-stamp from
        // a zeroed node - so a resync read an NPC's empty mirror into the
        // second and third records. `opurud` does exactly that, and the next
        // load took the zeroed record for an unjoined member and re-seeded
        // it from the New Game template.
        if !self.party_actors_are_party() {
            return self.party.roster.clone();
        }
        // Actor slot -> roster record follows the present-party composition:
        // under an [`legaia_engine_menus::party_state::PartyState::active_party`] mapping, actor ordinal `i` mirrors
        // the character at `active_party[i]`, and characters NOT in the
        // present party keep their record values untouched. The identity
        // default resyncs every record from its same-index actor, the
        // historical behaviour.
        let members = if self.party.active_party.is_empty() {
            self.party.roster.members.len().min(self.actors.len())
        } else {
            self.party.active_party.len().min(self.actors.len())
        };
        for member in 0..members {
            let rslot = self.party_roster_slot(member);
            let a = &self.actors[member];
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut hms = rec.hp_mp_sp();
                hms.hp_cur = a.battle.hp;
                hms.hp_max = a.battle.max_hp;
                hms.mp_cur = a.battle.mp;
                rec.set_hp_mp_sp(hms);
            }
        }
        self.party.roster.clone()
    }

    /// Write the **battle party's** live HP / MP / AP into their roster records.
    ///
    /// The narrow sibling of [`Self::save_party`], for
    /// [`Self::finish_battle`]. Two differences, both deliberate:
    ///
    /// - It stops at [`legaia_engine_menus::party_state::PartyState::party_count`]. In battle the actor slots past
    ///   the party band hold *monsters*, and `save_party`'s identity default
    ///   walks the whole roster - so running it verbatim at battle end would
    ///   copy a monster's HP into the fourth character's record.
    /// - It writes `hp_cur` / `mp_cur` / `sp_cur` only, never `hp_max`. Max HP does not
    ///   move during a fight, and the level-up applier has already written
    ///   the post-victory maxima into the records by the time this runs.
    pub(in crate::world) fn persist_battle_party_hp(&mut self) {
        let n = (self.party.party_count as usize).min(self.actors.len());
        for member in 0..n {
            let rslot = self.party_roster_slot(member);
            let (hp, mp, ap) = {
                let a = &self.actors[member].battle;
                (a.hp, a.mp, a.spirit_gauge)
            };
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut hms = rec.hp_mp_sp();
                hms.hp_cur = hp;
                hms.mp_cur = mp;
                // The Spirit (AP) gauge goes back to the record's `+0x10E`
                // with MP: both results arms of `FUN_8004E568` store
                // `+0x150 -> +0x10A` and `+0x170 -> +0x10E` per member
                // (`0x8004F1E8..0x8004F220`, `0x8004FBE8..0x8004FC20`).
                hms.sp_cur = ap;
                rec.set_hp_mp_sp(hms);
            }
        }
    }

    /// Project each present-party record's HP / MP back onto its party actor's
    /// [`BattleActor`] mirrors - the inverse of the [`Self::save_party`]
    /// resync, over the same `party_roster_slot` mapping.
    ///
    /// Used by [`Self::finish_battle`] after the field actor table is restored
    /// from the pre-battle snapshot: the snapshot's mirrors are stale, and the
    /// records (just written by [`Self::persist_battle_party_hp`]) hold the
    /// post-battle truth.
    ///
    /// Bounded by [`legaia_engine_menus::party_state::PartyState::party_count`]: in the restored *field* table the
    /// slots past the party band are NPCs, and pushing a character record's
    /// HP onto an NPC's mirrors is never right.
    pub fn resync_party_actors_from_roster(&mut self) {
        let members = (self.party.party_count as usize)
            .min(self.actors.len())
            .min(if self.party.active_party.is_empty() {
                self.party.roster.members.len()
            } else {
                self.party.active_party.len()
            });
        for member in 0..members {
            let rslot = self.party_roster_slot(member);
            let Some(hms) = self.party.roster.members.get(rslot).map(|r| r.hp_mp_sp()) else {
                continue;
            };
            let a = &mut self.actors[member];
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
        }
    }
}

/// Per-scene save permission and the pause menu's entry-context kind - the two
/// gate inputs the retail root command picker reads before it lets a row
/// through.
impl World {
    /// Seed [`legaia_engine_menus::party_state::PartyState::scene_save_allowed`] from the scene MAN just loaded.
    ///
    /// Retail's MAN loader does this inline, one instruction after it takes
    /// the header's status word: it reads byte `+1` of the resident MAN
    /// buffer `_DAT_8007B898`, masks bit `0`, and stores the result **byte
    /// wide** into the per-scene save-allow flag `_DAT_8007B6A8`.
    ///
    /// ```text
    /// 8003af48  lbu   v0,0x1(v1)        ; v1 = _DAT_8007B898 (the MAN)
    /// 8003af4c  lbu   s7,0x0(v1)
    /// 8003af50  andi  v0,v0,0x1
    /// 8003af54  sb    v0,-0x4958(a0)    ; a0 = 0x80080000 -> 0x8007B6A8
    /// ```
    ///
    /// `None` (the scene carries no MAN, or its MAN did not parse) clears the
    /// flag, which is the state retail's own init leaves the byte in
    /// (`FUN_80025980` zeroes it) - no MAN, no permission.
    ///
    /// On the retail disc the bit is set on exactly the three kingdom
    /// world-map scenes and clear on every field scene, which is why saving
    /// outside a scripted save point is a world-map-only affordance.
    ///
    /// PORT: FUN_8003aeb0 (`0x8003AF48..0x8003AF54`)
    pub fn install_scene_save_permission(
        &mut self,
        man: Option<&legaia_scene_formats::man_section::ManFile>,
    ) {
        self.party.scene_save_allowed = man.is_some_and(|m| m.header.low_flag);
    }

    /// Release an op-`0x49` **menu-entry-context** park once the pause menu
    /// has closed. Returns whether a park was released.
    ///
    /// A scripted menu press ([`World::scripted_menu_open_pending`]) ends the
    /// way retail's does: the pause-menu session's last phase clears the
    /// cursor context's completion gate (`sh zero,0x3e(v0)` at `0x801ED52C`),
    /// and the dispatcher `FUN_801F159C` then retires the subsystem actor and,
    /// because the park is still live, writes the Done sentinel into it
    /// (`0x801F1678` `lw v1,-0x4bb0(a2)`, `bne v1,zero,0x801F16A8`,
    /// `sw 1,-0x4bb0(a2)`). So the parked op resumes once - a save point's
    /// `49 01` takes its Done arm, a `49 0D` advances - rather than re-arming
    /// and pressing the menu again. The port raises the screen's Done for the
    /// park's own context and drops the kind byte.
    ///
    /// Any other park is dropped without a Done, as before.
    ///
    /// The three-instruction leaf `FUN_8003540C` (`sw zero,0x148(gp)` /
    /// `sw zero,0x138(gp)` / `jr ra`) is **not** this release: it has no
    /// reference of any form on the disc (`find-address-word-refs.py 8003540c
    /// --prot`), so nothing ever zeroes the park on a menu close. Its sibling
    /// `FUN_800353E0`, which makes the same two stores, is reached only from
    /// the scene loaders (`0x8003B2C8`, `0x80055FC8`).
    ///
    /// REF: FUN_801F159C (retire arm), FUN_801ED308 (phase 5)
    pub fn release_menu_entry_context_park(&mut self) -> bool {
        let Some(kind) = self.field_vm.submode_screen.park_sub_op else {
            return false;
        };
        if crate::field_submode_screen::op49_sub_op_is_menu_press(kind) {
            self.field_vm.submode_screen.done = true;
        }
        self.clear_op49_park();
        true
    }

    /// Kind byte of the op-`0x49` entry context the pause menu tests - the
    /// engine's read of retail `*_DAT_8007B450`.
    ///
    /// Retail parks the field VM on op `0x49` by storing the **operand
    /// pointer** into `_DAT_8007B450` (`sw s6,-0x4bb0(s0)` in the op's Idle
    /// arm), and that operand opens on its sub-op byte (`lbu v0,0x0(s6)` two
    /// instructions earlier). So the "kind byte" every consumer dereferences
    /// is just the armed sub-op: `1` is a field save point (which enters the
    /// card driver directly), `0x0D` is the context that blocks the menu's
    /// Load row and turns its cancel into a Yes/No confirm.
    ///
    /// The port has no single global to read, because it tags each park with
    /// the context that armed it
    /// ([`crate::field_submode_screen::Op49ParkOwner`]), so it keeps the byte
    /// itself instead - on
    /// [`crate::field_submode_screen::SubmodeScreen::park_sub_op`], written
    /// by the arm ([`World::record_op49_park`]) and cleared by the resume.
    /// That covers every sub-op, including the three the port resolves
    /// through dedicated host paths, because retail's store happens before
    /// those paths diverge.
    ///
    /// The two legacy derivations below it stay as a fallback for a host
    /// that arms a shop or a tile board **without** going through the field
    /// VM (`World::try_arm_field_shop` and `World::try_install_tile_board`
    /// are both callable directly, and several tests do exactly that). They
    /// agree with the park by construction - an inline shop is sub-op `0`
    /// and a tile board sub-op `5`.
    ///
    /// Retail's own consumers of this byte, and which value selects each:
    ///
    /// | kind | consumer |
    /// |---|---|
    /// | `0` | save/menu driver opens sub-screen `0x1A` |
    /// | `1` | opens `0x19` - a field save point goes straight to the card |
    /// | `7` | opens `0x20` - the casino prize exchange |
    /// | `0x0D` | opens `4`, blocks the root Load row, arms the leave confirm |
    ///
    /// (`FUN_801DC6B4` at `0x801dc88c..0x801dc8e4`.) Which of those a real
    /// disc ever arms is measured by
    /// `crates/engine-scene/tests/op49_sub_op_census.rs`.
    ///
    /// REF: FUN_801de840 (op `0x49` Idle arm, `_DAT_8007B450 = operand`)
    /// REF: FUN_801dc6b4 (the routing consumer)
    pub fn menu_entry_context_kind(&self) -> Option<u8> {
        if let Some(sub_op) = self.field_vm.submode_screen.park_sub_op {
            return Some(sub_op);
        }
        if self.shops.shop_armed {
            return Some(0);
        }
        if self.board.armed {
            return Some(5);
        }
        None
    }

    /// Whether this scene mode is one whose player is walked by the field
    /// locomotion controller - and therefore one whose pad reaches the
    /// **menu-open accept**.
    ///
    /// Retail has no global Start handler. The accept is a leg of the
    /// pre-movement header inside `FUN_801D01B0`
    /// (`0x801D0250..0x801D032C`): it tests the newly-pressed pad word
    /// `_DAT_8007B874` against the configurable menu-button mask
    /// `_DAT_800846D8`, and on a hit plays cue `0x20`, raises the player's
    /// engaged bit `+0x10 |= 0x80000`, and spawns the menu actor through the
    /// shared allocator `FUN_80020DE0(&DAT_8007065C, _DAT_8007C34C)`. So
    /// "can the menu open here" is exactly "does this scene run
    /// `FUN_801D01B0`".
    ///
    /// The **overworld runs it too**, and that is what makes the Save row
    /// reachable at all. All three kingdom overworlds are ordinary
    /// `game_mode 0x03` field-run scenes driven by the same
    /// `FUN_801D1344` -> `FUN_801D01B0` chain as a town, so the same accept
    /// covers both. Retail's own proof sits inside the controller: the
    /// base-step selector's `s4 = 5` arm at `0x801D0354` is taken exactly
    /// when the world-map flag `_DAT_8007B6A8` is set, which would be dead
    /// code if a `_DAT_8007B6A8` scene never entered this function.
    ///
    /// `FUN_801E76D4` is **not** a second controller that would need its own
    /// arm - it is the top-view debug renderer, and it branches straight to
    /// its epilogue (`0x801E9B14`) whenever `DAT_801F2B94 == 0`. Entering
    /// top view at all needs the debug flag `_DAT_8007B98C`, which retail
    /// leaves clear. See [`legaia_engine_field::world_map::WorldMapController`].
    ///
    /// The port splits the one retail mode in two -
    /// [`SceneMode::Field`](legaia_engine_mode::mode::SceneMode::Field) for towns and
    /// fields, [`SceneMode::WorldMap`](legaia_engine_mode::mode::SceneMode::WorldMap)
    /// for the kingdom overworlds - so the predicate has to name both. Every
    /// other mode (battle, cutscene, the minigames, an already-open menu)
    /// suspends field dispatch and never reaches the accept.
    ///
    /// REF: FUN_801D01B0 (`0x801D0250..0x801D032C`, the menu-open accept)
    /// REF: FUN_801D1344 (the per-frame tick that calls it, `0x801D16F4`)
    /// REF: FUN_801E76D4 (`0x801E779C`, the not-top-view branch to the epilogue)
    pub fn scene_mode_takes_menu_open(&self) -> bool {
        matches!(
            self.mode,
            legaia_engine_mode::mode::SceneMode::Field
                | legaia_engine_mode::mode::SceneMode::WorldMap
        )
    }

    /// The whole menu-open precondition, in the order retail tests it.
    ///
    /// Both shipped hosts and the shared `BootSession` driver must route
    /// their Start edge through this one predicate rather than spelling the
    /// mode test out locally - a host that writes its own copy is how the
    /// overworld lost the pause menu in the first place.
    ///
    /// 1. The engaged bit. `FUN_801D01B0`'s **first** test (`0x801D01F0`)
    ///    branches past the entire header when `player+0x10 & 0x80000` is
    ///    set, so a talking player's Start opens nothing and does not even
    ///    buzz. The engine's stand-in is
    ///    [`World::dialogue_owns_input`](crate::world::World::dialogue_owns_input).
    ///    A conversation is only one of the bit's raisers. The per-actor
    ///    script runner `FUN_80039B7C` raises it too, on every frame it steps
    ///    an engaged context (`0x80039DB8..0x80039DD4`, with the running
    ///    count at `_DAT_801C6EA4+0xA`), and clears it only once that count
    ///    drains on a `0x21` yield (`0x80039EE8..0x80039F14`). So while a
    ///    spawned record is parked mid-script - a walk-on beat's camera
    ///    move or `ExecMove` wait - the caller `FUN_801D1344` never calls
    ///    the controller (`0x801D1694`) and the menu button does nothing.
    ///    A Door of Light that lands on `map01`'s cave-mouth trigger tile
    ///    runs `P2[9]` this way and holds the menu for its whole run. The
    ///    engine's counterpart is
    ///    [`World::script_context_engages_player`](crate::world::World::script_context_engages_player):
    ///    the modal timeline or any live concurrent helper record.
    /// 2. The scene mode - [`Self::scene_mode_takes_menu_open`].
    ///
    /// Note this is the *open* gate only. Whether the opened menu's **Save**
    /// row then accepts is a separate, per-scene question answered by
    /// [`legaia_engine_menus::party_state::PartyState::scene_save_allowed`](legaia_engine_menus::party_state::PartyState::scene_save_allowed)
    /// at the row's confirm, exactly as retail keeps `_DAT_800846D8` (which
    /// button opens the menu) and `_DAT_8007B6A8` (whether Save is legal
    /// here) as two independent globals.
    ///
    /// 3. The successor the menu button's subsystem actor picks
    ///    ([`Self::field_menu_button_state`]) is the pause-menu session.
    ///    Only a debug build holding the pad's `0x100` bit picks anything else.
    ///
    /// 4. No shop / prize counter and no narration crawl or title card holds
    ///    the screen. A shop runs at game mode `0x17` with the menu overlay
    ///    resident and the field overlay swapped out, so the locomotion
    ///    controller whose header reads Start is not running at all; the
    ///    crawl owns the scene and freezes the pad. The native window used to
    ///    spell both out beside this call (`menu_runtime.is_open()` and its
    ///    `narration` local) while the browser page asked only this
    ///    predicate - and read the pause-menu Start before its shop - so on
    ///    the page Start opened the pause menu over an open shop.
    ///
    /// 5. The player tick calls the pad controller at all. `FUN_801D1344`
    ///    skips `FUN_801D01B0` - and with it the menu accept - while the
    ///    scratchpad system lock `_DAT_1F800394 & 0x400` is set
    ///    (`0x801D16A8..0x801D16B8`), while a kind-0 warp's timer
    ///    `_DAT_8007B6B0` runs, and while its post-warp pad hold
    ///    `_DAT_8007B6B4` drains (`0x801D16C8..0x801D16E4`,
    ///    [`legaia_engine_field::field_warp_tile::pad_suppressed`]). The engine
    ///    already held the player still through the warp; the menu button
    ///    stayed live through it. No shipped script issues `2E 0A`, so the
    ///    lock's writer is not a field-VM op.
    ///
    /// 6. The menu lock `_DAT_1F800394 & 0x8000000` is clear. Unlike every
    ///    gate above it is tested *inside* the accept, after the press
    ///    (`0x801D02C0..0x801D02E4`): a locked press plays the deny buzz
    ///    `0x23` through `FUN_80035BD0` instead of opening anything - see
    ///    [`Self::field_menu_press_denied`]. `town0e` and `urudre1` raise it
    ///    (`2E 1B`); `urudre1`, `edteien` and `edbalden` drop it (`2F 1B`).
    ///
    /// 7. The New Game opening chain is not playing
    ///    ([`crate::world::CutsceneState::opening_chain_active`]). Its legs are
    ///    script records end to end, so retail's engaged bit stands through
    ///    all of them; the engine seats each leg's entry record one tick
    ///    after the scene loads, and this keeps that tick shut the way the
    ///    record does (`tests/opening_chain_menu_refusal_disc.rs`).
    ///
    /// 8. No fight is committed ([`Self::field_scripts_held_for_battle`]).
    ///    Between a `3E FF` (or an encounter roll) and the battle, retail
    ///    pages the battle-intro overlay (PROT 0979, base `0x801CE818`) over
    ///    the field overlay's head, which holds `FUN_801D1344` - so neither
    ///    the pad controller nor its menu accept runs on those frames. The
    ///    engine keeps ticking the field through the intro, and a Start
    ///    there opened the pause menu and its Save row onto a save that holds
    ///    the record's flags past the `3E FF` but not the fight: `jouine`
    ///    P2[5] commits Cort on arrival, and that save reloaded onto the
    ///    epilogue band P2[2] with Cort unbeaten.
    ///
    /// REF: FUN_801D01B0 (`0x801D01F0` engaged bit, `0x801D0250` accept)
    /// REF: FUN_80039B7C (the script runner's engaged-bit raise and clear)
    /// REF: FUN_801D1344 (`0x801D16A8..0x801D16E4`, the pad-controller gates)
    pub fn field_menu_open_allowed(&self) -> bool {
        self.field_menu_press_reaches_accept() && self.flags.story_flags & FIELD_MENU_LOCK_BIT == 0
    }

    /// A menu-button press the accept takes but the menu lock refuses
    /// (`0x801D02C0..0x801D02E4`): queues the deny buzz
    /// [`FIELD_MENU_DENY_CUE`] on the SFX ring (`FUN_80035BD0(0x23)`, the
    /// overwrite producer) and returns `true`. Every other refusal is silent -
    /// it never reaches the accept - and returns `false` without a cue.
    ///
    /// Hosts call this on a menu-button edge that
    /// [`Self::field_menu_open_allowed`] refused.
    ///
    /// REF: FUN_801D01B0 (`0x801D02C0..0x801D02E4`, the locked-press buzz)
    /// REF: FUN_80035BD0
    pub fn field_menu_press_denied(&mut self) -> bool {
        if self.field_menu_press_reaches_accept()
            && self.flags.story_flags & FIELD_MENU_LOCK_BIT != 0
        {
            self.replace_last_sfx_cue(FIELD_MENU_DENY_CUE);
            return true;
        }
        false
    }

    /// Every gate in front of the menu accept's own lock test - items 1 to 5
    /// and 7 of [`Self::field_menu_open_allowed`].
    fn field_menu_press_reaches_accept(&self) -> bool {
        self.scene_mode_takes_menu_open()
            // The overworld map display: while its fade-up ramp runs
            // `FUN_801D1344` returns before it calls the pad controller
            // (`0x801D1344..0x801D1388`), and once the display is up the
            // walk frame is not run at all (mode `0x0D`).
            && !self.map_display_owns_pad()
            && self.flags.story_flags & FIELD_SYSTEM_LOCK_BIT == 0
            && !legaia_engine_field::field_warp_tile::pad_suppressed(&self.locomotion.warp)
            && !self.cutscene.opening_chain_active
            && !self.dialogue_owns_input()
            && !self.script_context_engages_player()
            && !self.shops.shop_open
            && !self.shops.prize_exchange_open
            && !self.cutscene_narration_active()
            && self.cutscene.card.is_none()
            && self.field_menu_button_state() == legaia_engine_field::field_state_pick::STATE_NORMAL
            && !self.field_scripts_held_for_battle()
    }

    /// Whether a host may write a save of the live session now, outside the
    /// in-game save screen (the browser page's session export). Retail has
    /// no such path; its only saves are the save screen's, behind
    /// [`Self::field_menu_open_allowed`] and the save points' scripts, and
    /// neither runs between a fight's commit and the fight
    /// ([`Self::field_scripts_held_for_battle`]). A save taken there is a
    /// state retail never writes - the flags past the `3E FF` without the
    /// fight - so a host refuses it until the battle has begun.
    pub fn host_save_allowed(&self) -> bool {
        !self.field_scripts_held_for_battle()
    }

    /// The handler id the menu button's subsystem actor moves on to.
    ///
    /// Retail's menu-open accept spawns the field overlay's subsystem actor
    /// (`FUN_80020DE0(0x8007065C, ..)` at `0x801D0324`); its installer
    /// `FUN_801F1278` stores handler id `7` (`0x801F140C`), and handler `7`
    /// is `FUN_801F1F4C`, which picks the successor: `0x30`, the pause-menu
    /// session `FUN_801ED308`, unless a script is parked on op `0x49`, the
    /// debug word `_DAT_8007B98C` is set and the packed pad holds `0x100` -
    /// then `0x13`, a debug screen the engine does not have. The engine's
    /// debug word is the overworld controller's
    /// [`legaia_engine_field::world_map::WorldMapController::debug_enabled`] (the same
    /// `_DAT_8007B98C` gate the top-view toggle reads); in a field scene it is
    /// retail's zero.
    ///
    /// PORT driver for FUN_801F1F4C (the kernel is
    /// `legaia_engine_field::field_state_pick::state_pick`)
    pub fn field_menu_button_state(&self) -> u16 {
        use legaia_engine_field::field_state_pick::{StatePickInputs, state_pick};
        let debug = self.debug_mode_word_set();
        let inputs = StatePickInputs {
            script_resume_slot: u32::from(self.field_vm.submode_screen.park_sub_op.is_some()),
            debug_mode: u32::from(debug),
            pad_mask: u32::from(legaia_engine_field::world_map_panel_host::packed_pad(
                self.input.pad(),
            )),
        };
        state_pick(
            inputs,
            legaia_engine_field::field_subsystem_enter::STATE_PICK_HANDLER,
        )
        .actor_state
    }
}
