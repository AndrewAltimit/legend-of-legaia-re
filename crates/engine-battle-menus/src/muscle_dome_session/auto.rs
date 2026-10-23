//! The dome fighter's **Auto** attack: the saved command string and the
//! round's rebuild of it.
//!
//! A dome round is the battle's round, so its Auto is the battle's
//! (`docs/subsystems/minigame-muscle-dome.md`, "The Auto arm"):
//!
//! 1. **Load.** The ring's Attack confirm and the prompt's `Command` both
//!    run `FUN_801DA34C` (`0x801D15C8`, `0x801D1734`): the character
//!    record's 16-byte band at `+0x1A7` / `+0x1B7` (the band picked by the
//!    `+0x156 < +0x154` gauge test) lands in `actor+0x1DF`. The entry screen
//!    opens over it - its pennants are the previous round's, and a bare
//!    confirm replays it.
//! 2. **Save.** The review confirm runs `FUN_801DA59C` (`0x801D22BC`): the
//!    committed string goes back into the same band.
//! 3. **Auto.** The prompt's `Auto` chip skips the entry: the review shows the
//!    loaded string and the per-fighter flag `ctx[+0x266 + slot]` is raised
//!    (`0x801D17D0`). At the round's start the action SM's state `0x00`
//!    runs `FUN_801F0450`, whose pool arm **rebuilds** a flagged Attack
//!    seat's queue - a weighted draw of the four direction commands spent
//!    against the action gauge, then learned arts spliced over it by the
//!    Spirit-budgeted tail. The saved string is what the review shows; the
//!    rebuilt one is what the round plays.
//!
//! The session holds the two bands (the host seeds them from the character
//! record and banks them back on leave) and the per-fighter flag. The pool
//! arm reads disc inputs a host installs ([`DomeAutoInputs`]); the kernels are
//! `legaia_engine_battle_vm::battle_arts_auto_combo`'s, the ones the regular
//! battle's Auto runs.

use super::*;
use legaia_engine_battle_vm::battle_arts_auto_combo as combo;
use legaia_save::{AUTO_COMMAND_STRING_LEN, AutoCommandBand};

/// The disc inputs the round's Auto rebuild reads for the dome fighter - the
/// per-character half of `engine-core`'s `AutoComboInputs` plus the record
/// and opponent words the pool arm and its tail test.
#[derive(Debug, Clone, Default)]
pub struct DomeAutoInputs {
    /// The first four bytes of each direction command's action entry
    /// (`DAT_801C9360[slot][0xC + i]`, Left / Right / Down / Up), the weight
    /// ladder's input. `None` weighs every command at the default `8`.
    pub command_heads: Option<[[u8; 4]; 4]>,
    /// The four status-guard masks at PROT 0898 `0x801F672C`. `None` guards
    /// nothing.
    pub guards: Option<[u16; 4]>,
    /// The fighter's art-animation bank (`record[0] +0x58`, `0xD0` stride),
    /// which the tail walks. Empty skips the tail.
    pub art_records: Vec<[u8; 0xD0]>,
    /// The bank's count byte.
    pub art_count: u8,
    /// The character record's learned-arts list (`+0x186..`, count `+0x185`).
    pub learned: Vec<u8>,
    /// Roster character id (`1` Vahn, `2` Noa, `3` Gala).
    pub char_id: u8,
    /// Character record `+0xF8` (the upper accessory-passive word).
    pub ability_high: u32,
    /// The opponent monster record's swing class (`+0x1E`), which picks the weight
    /// family.
    pub opponent_swing_class: u8,
}

/// The status-guard table the pool arm reads, `*(i16 *)(0x801F672C + i*2)`
/// in the battle overlay (PROT 0898).
pub const AUTO_GUARD_TABLE_VA: u32 = 0x801F_672C;

impl DomeAutoInputs {
    /// Read the per-character half off the fighter's player battle file
    /// (`data\battle\PLAYER1..3`) for the `equipped` section ids (record
    /// `+0x196..+0x19A`): each direction command's leading entry bytes and
    /// the art-animation bank. What does not decode is left as it was - the
    /// pool arm then weighs at the default and the tail has nothing to walk.
    ///
    /// The same reads the field scene stages for the regular battle's Auto
    /// (`SceneHost::refresh_battle_auto_combo_inputs`).
    pub fn read_player_file(&mut self, player_file: &[u8], equipped: &[u8; 5]) {
        use legaia_actor_formats::battle_char_assembly as bca;
        if let Some(pack) = legaia_actor_formats::battle_data_pack::detect(player_file)
            && let Ok(swings) = bca::swing_battle_animations(player_file, &pack, equipped)
        {
            let mut heads = [[0u8; 4]; 4];
            for sw in &swings {
                if let Some(i) = sw.slot.checked_sub(bca::SWING_SLOT_BASE)
                    && let Some(h) = heads.get_mut(usize::from(i))
                {
                    for (d, b) in h.iter_mut().zip(sw.anim.effect_script.iter()) {
                        *d = *b;
                    }
                }
            }
            self.command_heads = Some(heads);
        }
        if let Ok(record0) = bca::decode_record0(player_file)
            && let Some(bank_off) = record0
                .get(0x58..0x5C)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize)
            && let Some(count) = record0.get(bank_off).copied()
        {
            let first = bank_off + 4;
            self.art_records = (0..usize::from(count))
                .filter_map(|i| {
                    let at = first + i * bca::ART_RECORD_STRIDE;
                    record0
                        .get(at..at + bca::ART_RECORD_STRIDE)
                        .and_then(|r| <[u8; 0xD0]>::try_from(r).ok())
                })
                .collect();
            self.art_count = count;
        }
    }

    /// Read the four status-guard masks off the battle overlay's as-loaded
    /// image (`loaded`, based at `base_va`).
    pub fn read_guards(&mut self, loaded: &[u8], base_va: u32) {
        let Some(off) = AUTO_GUARD_TABLE_VA.checked_sub(base_va) else {
            return;
        };
        let Some(raw) = loaded.get(off as usize..off as usize + 8) else {
            return;
        };
        let mut guards = [0u16; 4];
        for (i, g) in guards.iter_mut().enumerate() {
            *g = u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
        }
        self.guards = Some(guards);
    }
}

/// The dome fighter's Auto state: the record's two saved bands and the
/// per-fighter flag.
#[derive(Debug, Clone, Default)]
pub struct DomeAuto {
    /// Record `+0x1A7` ([`AutoCommandBand::Primary`]) and `+0x1B7`
    /// ([`AutoCommandBand::Secondary`]).
    pub(super) strings: [[u8; AUTO_COMMAND_STRING_LEN]; 2],
    /// Which band the fighter's gauge pair selects.
    pub(super) band: Option<AutoCommandBand>,
    /// `ctx[+0x266]` for the player's fighter.
    pub(super) flag: bool,
    /// Disc inputs for the round rebuild.
    pub(super) inputs: Option<DomeAutoInputs>,
    /// The rebuild's `rand()` cursor when no damage model lends its stream.
    pub(super) rng: u32,
}

impl MuscleDomeSession {
    /// Seat the player's fighter's two saved command strings - record
    /// `+0x1A7` and `+0x1B7` - and the band its gauge pair selects
    /// ([`AutoCommandBand::for_gauge`] over `+0x154` / `+0x156`).
    pub fn set_auto_command_strings(
        &mut self,
        primary: [u8; AUTO_COMMAND_STRING_LEN],
        secondary: [u8; AUTO_COMMAND_STRING_LEN],
        band: AutoCommandBand,
    ) {
        self.auto.strings = [primary, secondary];
        self.auto.band = Some(band);
    }

    /// The two saved command strings as they stand, `(primary, secondary)` -
    /// what a host banks back into the character record when the leg ends.
    pub fn auto_command_strings(
        &self,
    ) -> ([u8; AUTO_COMMAND_STRING_LEN], [u8; AUTO_COMMAND_STRING_LEN]) {
        (self.auto.strings[0], self.auto.strings[1])
    }

    /// Install the disc inputs the round's Auto rebuild reads.
    pub fn install_auto_inputs(&mut self, inputs: DomeAutoInputs) {
        self.auto.inputs = Some(inputs);
    }

    /// Whether the player's fighter took `Auto` this turn
    /// (`ctx[+0x266 + slot]`).
    pub fn auto_flag(&self) -> bool {
        self.auto.flag
    }

    fn auto_band(&self) -> AutoCommandBand {
        self.auto.band.unwrap_or(AutoCommandBand::Secondary)
    }

    /// The saved string `FUN_801DA34C` loads into the fighter's queue
    /// window, as queue bytes, up to its terminator.
    ///
    /// PORT: FUN_801DA34C (the band pick and the head-byte fallback, through
    /// the shared `preseed_action_queue`)
    pub(super) fn load_auto_string(&self) -> Vec<u8> {
        let mut window = [0u8; legaia_engine_battle_vm::battle_action::ACTION_QUEUE_CAP];
        legaia_engine_battle_vm::battle_action::preseed_action_queue(
            &mut window,
            // The staging gate `DAT_8007BD04`: a dome fighter choosing is the
            // situation it is raised for.
            true,
            self.auto_band() == AutoCommandBand::Secondary,
            &self.auto.strings[0],
            &self.auto.strings[1],
        );
        window
            .iter()
            .take(AUTO_COMMAND_STRING_LEN)
            .take_while(|&&b| b != 0)
            .copied()
            .collect()
    }

    /// The loaded string as the entry screen's preseed: direction commands
    /// up to the first byte that is not one.
    pub(super) fn auto_preseed(&self) -> Vec<u8> {
        self.load_auto_string()
            .into_iter()
            .map_while(dome_command_of_action_byte)
            .map(legaia_art::Command::as_byte)
            .collect()
    }

    /// Write the fighter's committed string back into its band - the review
    /// confirm's `FUN_801DA59C`. A cast is not an Attack commit
    /// (`+0x1DE != 3`) and writes nothing.
    ///
    /// PORT: FUN_801DA59C (through the shared `save_action_queue`)
    pub(super) fn save_auto_string(&mut self) {
        let mut queue = [0u8; AUTO_COMMAND_STRING_LEN];
        for (d, s) in queue.iter_mut().zip(&self.f[0].queue) {
            *d = *s;
        }
        let alive = self.f[0].hp > 0;
        let attack = self.cast[0].is_none();
        let second = self.auto_band() == AutoCommandBand::Secondary;
        let [first, rest] = &mut self.auto.strings;
        legaia_engine_battle_vm::battle_action::save_action_queue(
            &queue, alive, attack, second, first, rest,
        );
    }

    /// Lower the flag - the prompt's `Command` (`0x801D1760`) and every
    /// frame the ring is up (`0x801D11A8`).
    pub(super) fn clear_auto_flag(&mut self) {
        self.auto.flag = false;
    }

    /// The round-start rebuild of a flagged fighter's queue: `FUN_801F0450`'s
    /// pool arm and art-insertion tail over the dome fighter.
    ///
    /// Without installed inputs the commands take the default weight and the
    /// tail has no bank to walk, which leaves a pool-arm string of plain
    /// swings. A cast is not an Attack and is never rebuilt
    /// (`0x801F0704..0x801F0730` tests the category).
    pub(super) fn run_auto_rebuild(&mut self) {
        if !self.auto.flag || self.cast[0].is_some() {
            return;
        }
        let inputs = self.auto.inputs.clone().unwrap_or_default();
        let guards = inputs.guards.unwrap_or([0; 4]);
        let mut commands: Vec<combo::ArtsCommand> = (0..4u8)
            .map(|i| {
                let id = combo::FIRST_COMMAND + i;
                let cost = self.f[0]
                    .hand
                    .iter()
                    .find(|c| c.command_id == id)
                    .map_or(0xFF, |c| c.cost.min(0xFF) as u8);
                combo::ArtsCommand {
                    id,
                    cost,
                    bytes: inputs
                        .command_heads
                        .map(|h| h[i as usize])
                        .unwrap_or_default(),
                    guard: guards[i as usize],
                }
            })
            .collect();
        commands.sort_by_key(|c| c.id);
        let family = combo::weight_family(inputs.opponent_swing_class);
        let pool = combo::build_candidate_pool(&commands, family, self.ring(0).status);
        let mut rng = match self.damage.as_ref() {
            Some(m) => m.rng_seed(),
            None => self.auto.rng,
        };
        let mut draw = || legaia_engine_system::rand::world_rand(&mut rng) as i32;
        let spent = combo::spend_gauge(
            &pool,
            self.f[0].budget_pool.min(i16::MAX as u16) as i16,
            |id| {
                commands
                    .iter()
                    .find(|c| c.id == id)
                    .map_or(0xFF, |c| c.cost)
            },
            &mut draw,
        );
        let mut queue = spent.queue;
        if !inputs.art_records.is_empty() {
            let spirit = self.damage.as_ref().map_or(0, |m| m.spirit(0));
            let tail = combo::ArtsTailInput {
                char_id: inputs.char_id,
                spirit,
                ability_high: inputs.ability_high,
                learned: &inputs.learned,
                // The slot's Miracle marker `ctx[+0x25F + slot]` is the same
                // byte the ring's Ra-Seru member gate reads, written once by
                // the party battle-actor init `FUN_80053CB8` from the record's
                // Ra-Seru slot.
                miracle_marker: self.ring(0).has_raseru,
                records: &inputs.art_records,
                art_count: inputs.art_count,
            };
            combo::insert_arts(&mut queue, &tail, &mut draw);
        }
        match self.damage.as_mut() {
            Some(m) => m.set_rng_seed(rng),
            None => self.auto.rng = rng,
        }
        let f = &mut self.f[0];
        queue.truncate(QUEUE_CAP);
        f.spent = queue
            .iter()
            .filter_map(|b| f.hand.iter().find(|c| c.command_id == *b))
            .map(|c| c.cost)
            .sum::<u16>()
            .min(f.budget_pool);
        f.budget = f.budget_pool - f.spent;
        f.queue = queue;
    }
}
