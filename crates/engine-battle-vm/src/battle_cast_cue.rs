//! Cast audio-cue dispatcher - the per-cast sound-cue resolver the battle
//! action SM invokes when a cast starts (`jal 0x801E3E04` in
//! `FUN_801E295C`).
//!
//! PORT: FUN_801F3990
//!
//! Battle overlay (PROT 0898, file `+0x25178`); the port is decoded from a
//! resident battle-overlay capture (`overlay_muscle_dome_801f3990.txt` -
//! the muscle-dome capture is the battle-action overlay in residence, see
//! `docs/subsystems/minigame-muscle-dome.md`). The 0897-labelled dump at
//! this VA prints the over-read `FUN_801F3894` body and is NOT this
//! function (`docs/reference/functions.md` row `801F3990`).
//!
//! Retail inputs: acting slot `ctx[+0x13]`, its char-kind byte
//! `DAT_8007BD10[slot]`, the actor's cast class `actor[+0x1E8]` (seeded at
//! action state `0x3C` from the spell table's class byte), the queue head
//! `actor[+0x1DF]` and the sub-class byte `actor[+0x1E9]`. Output: one
//! `FUN_8004FCC8` SFX id, the `0xFE` item-give special, or nothing.
//!
//! # Where it runs
//!
//! Retail's `jal` sits in the arm that stamps state `0x3E` after the queued
//! anim settles (`overlay_battle_action_801e295c.txt`
//! `0x801E3DD8..0x801E3E08`), which the port has as `battle_action::spirit`'s
//! `spirit_wait` - and that is where this is called from. All five arguments
//! now have carriers:
//!
//! * `actor[+0x1E8]` / `actor[+0x1E9]` are
//!   [`BattleActor::cast_class`](crate::battle_action::BattleActor::cast_class)
//!   and
//!   [`cast_sub_class`](crate::battle_action::BattleActor::cast_sub_class),
//!   seeded at state `0x3C` from the item-effect descriptor table or the
//!   spell table depending on the category byte (retail
//!   `0x801E3B70..0x801E3CB0`);
//! * the char-kind byte `DAT_8007BD10[slot]` is
//!   [`BattleActionHost::roster_character_id`](crate::battle_action::BattleActionHost::roster_character_id) -
//!   it is the `* 0x10` term that separates one character's cue band from the
//!   next, so it has to be the roster id and not the battle slot;
//! * the queue head is `actor[+0x1DF]`.
//!
//! The sink is
//! [`BattleActionHost::one_shot_sfx`](crate::battle_action::BattleActionHost::one_shot_sfx)
//! (retail `FUN_8004FCC8`), with the `0xFE` item-give special routed to
//! [`cast_item_give`](crate::battle_action::BattleActionHost::cast_item_give).
//! These are a **different id space** from the art-record hit cues that ride
//! `ArtStrikeInfo` / `BattleSfxCue`: a cast cue is a `FUN_8004FCC8` dispatch
//! id (`>= 0xF8`, and `0x20C..=0x20E` on the enemy leg), which the host
//! classifies before it reaches a bank.

use crate::battle_slot::RetailSlot;

/// Outcome of the cast audio-cue dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastCueOutcome {
    /// No cue for this cast class.
    None,
    /// Play SFX `id` through the retail one-shot player (`FUN_8004FCC8`).
    Sfx(u16),
    /// The `actor[+0x1DF] == 0xFE` special: give item `0xFE` x1
    /// (`FUN_800421D4(0xFE, 1)`), play voice cue `voice_arg`
    /// (`FUN_8003D53C(char_kind + 0x19, 0, 0x5A)`) and stamp
    /// `_DAT_8007BD08` from the frame-speed byte.
    ItemGive {
        /// `char_kind + 0x19`, the first `FUN_8003D53C` argument.
        voice_arg: u8,
    },
}

/// PORT: FUN_801F3990 - resolve the cast-start audio cue.
///
/// Laws (capture-resident disassembly, jump tables reconstructed by the
/// decompiler from the resident image):
/// - enemy-side leg (`char_kind == 4` **or** `slot >= 3`, on the retail
///   pool slot - [`RetailSlot`]): cast class
///   `0..=2` -> SFX `0x20C`, `3 | 8` -> `0x20D`, `4` -> `0x20E`, all
///   other classes silent;
/// - player leg, queue head `0xFE`: the item-give special (no class cue);
/// - player leg otherwise: class `0 | 1` -> `char_kind*0x10 + 0xF8`,
///   `2` -> `+0xF9`, `3 | 8` -> `+0xFA`, `4` -> `+0xFB`, `5` -> `+0xFC`,
///   `7` -> `+0xFC` only when the sub-class byte `actor[+0x1E9]` is in
///   `1..=4`, classes `6` and `> 8` silent.
pub fn cast_audio_cue(
    slot: RetailSlot,
    char_kind: u8,
    cast_class: u8,
    queue_head: u8,
    sub_class: u8,
) -> CastCueOutcome {
    if char_kind == 4 || !slot.is_party() {
        return match cast_class {
            0..=2 => CastCueOutcome::Sfx(0x20C),
            3 | 8 => CastCueOutcome::Sfx(0x20D),
            4 => CastCueOutcome::Sfx(0x20E),
            _ => CastCueOutcome::None,
        };
    }
    if queue_head == 0xFE {
        return CastCueOutcome::ItemGive {
            voice_arg: char_kind.wrapping_add(0x19),
        };
    }
    let base = u16::from(char_kind) * 0x10;
    match cast_class {
        0 | 1 => CastCueOutcome::Sfx(base + 0xF8),
        2 => CastCueOutcome::Sfx(base + 0xF9),
        3 | 8 => CastCueOutcome::Sfx(base + 0xFA),
        4 => CastCueOutcome::Sfx(base + 0xFB),
        5 => CastCueOutcome::Sfx(base + 0xFC),
        7 if (1..=4).contains(&sub_class) => CastCueOutcome::Sfx(base + 0xFC),
        _ => CastCueOutcome::None,
    }
}

// ---------------------------------------------------------------------------
// The cast's own voice: the slot-B module's head cue
// ---------------------------------------------------------------------------

/// `jal FUN_8004FCC8` - the cue dispatcher every slot-B module raises its own
/// voice through (`docs/subsystems/cast-module.md`, "The cast's own CD-XA
/// voice").
const JAL_CUE_DISPATCH: u32 = jal_word(0x8004_FCC8);
/// `jal FUN_8004FE5C` - the battle sound funnel a few modules reach instead;
/// its `(id, category)` voice leg runs the same two gates and the same
/// arithmetic, for `category < 3` only.
const JAL_SFX_FUNNEL: u32 = jal_word(0x8004_FE5C);
/// `jal FUN_80056798` - the BIOS `rand` the two coin-flip modules (PROT 0936
/// / 0937) reduce to `v0 % 2` and add to their base id.
const JAL_BIOS_RAND: u32 = jal_word(0x8005_6798);
/// `jal FUN_8003EAE4` - the drive seek a module parks the head with ahead of
/// its voice, `(0, clip slot)`.
const JAL_DRIVE_SEEK: u32 = jal_word(0x8003_EAE4);

/// Words scanned back from a call site for the instruction that formed its
/// argument register. Twelve covers every literal site in the band and the
/// `rand` -> `div` -> `mfhi` -> `addiu a0,v0,base` run of the two coin-flip
/// modules; a longer window only admits stale writes.
const ARG_BACKSCAN_WORDS: usize = 12;

const fn jal_word(target: u32) -> u32 {
    0x0C00_0000 | ((target >> 2) & 0x03FF_FFFF)
}

/// How a slot-B module names its head cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleHeadCue {
    /// `li a0, id` at the call - 62 of the 64 modules.
    Literal(u16),
    /// `base + (rand() % span)` - PROT 0936 (`0x1B0`, `0x1B1`) and 0937
    /// (`0x1B2`, `0x1B3`): one coin flip per cast.
    Random { base: u16, span: u8 },
}

impl ModuleHeadCue {
    /// The cue this cast raises, drawing the coin flip from `rand` (the
    /// caller's BIOS `rand` port; only consumed for the two random modules).
    pub fn resolve(self, rand: impl FnOnce() -> u32) -> u16 {
        match self {
            ModuleHeadCue::Literal(id) => id,
            ModuleHeadCue::Random { base, span } => {
                base.wrapping_add((rand() % u32::from(span.max(1))) as u16)
            }
        }
    }
}

/// What formed an argument register at a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgSource {
    Literal(u16),
    /// `addiu <reg>, v0, imm` with a BIOS `rand` call in the window.
    RandPlus(u16),
    Unknown,
}

/// Resolve the value of argument register `reg` (`4` = `a0`, `5` = `a1`) at
/// the call in `words[site]`: the delay slot first, then the most recent
/// write inside [`ARG_BACKSCAN_WORDS`]. A write the scan cannot value (a
/// `move` from a saved register, a `lui` pair, a load) is `Unknown`, never a
/// guess.
fn resolve_arg(words: &[u32], site: usize, reg: u32) -> ArgSource {
    let classify = |w: u32| -> Option<ArgSource> {
        let op = w >> 26;
        let rs = (w >> 21) & 0x1F;
        let rt = (w >> 16) & 0x1F;
        let imm = (w & 0xFFFF) as u16;
        match op {
            // addiu rt, rs, imm
            0x09 if rt == reg => Some(match rs {
                0 => ArgSource::Literal(imm),
                2 => ArgSource::RandPlus(imm),
                _ => ArgSource::Unknown,
            }),
            // ori rt, rs, imm
            0x0D if rt == reg => Some(if rs == 0 {
                ArgSource::Literal(imm)
            } else {
                ArgSource::Unknown
            }),
            // lui rt / addi rt / any load into rt
            0x0F | 0x08 | 0x20..=0x26 if rt == reg => Some(ArgSource::Unknown),
            // SPECIAL: register-register writes name their destination in rd.
            0x00 if (w >> 11) & 0x1F == reg && w != 0 => Some(ArgSource::Unknown),
            _ => None,
        }
    };
    let rand_before = |from: usize| {
        let lo = from.saturating_sub(ARG_BACKSCAN_WORDS);
        words[lo..from].contains(&JAL_BIOS_RAND)
    };
    let settle = |src: ArgSource, at: usize| match src {
        ArgSource::RandPlus(base) if rand_before(at) => ArgSource::RandPlus(base),
        ArgSource::RandPlus(_) => ArgSource::Unknown,
        other => other,
    };
    if let Some(&slot) = words.get(site + 1)
        && let Some(src) = classify(slot)
    {
        return settle(src, site);
    }
    let lo = site.saturating_sub(ARG_BACKSCAN_WORDS);
    for at in (lo..site).rev() {
        if let Some(src) = classify(words[at]) {
            return settle(src, at);
        }
    }
    ArgSource::Unknown
}

/// The `v0 % N` divisor a coin-flip site reduces `rand()` with: the
/// `addiu v1, zero, N` between the `rand` call and the `addiu a0, v0, base`.
/// `2` when no divisor is in the window (both retail sites carry one).
fn rand_span_before(words: &[u32], site: usize) -> u8 {
    let lo = site.saturating_sub(ARG_BACKSCAN_WORDS);
    words[lo..site]
        .iter()
        .rev()
        .find_map(|&w| ((w >> 16) == 0x2403).then_some((w & 0xFF) as u8))
        .filter(|&n| n > 0)
        .unwrap_or(2)
}

/// The **head cue** of a slot-B cast module image - the first dispatcher
/// call inside the image's own content, never its inherited tail, and the
/// argument that call forms. This is the cue a cast is heard through
/// (`capture`, two live casts: PROT 0905 started `FUN_8003D53C(6, 1, 568)`,
/// PROT 0903 `(6, 4, 686)` - the rows this scan reproduces).
///
/// `image` is one PROT entry of `0903..=0966` as extracted. The content
/// bound is `legaia_overlay_images::slot_b_module::content_end` at the band's link
/// base; a funnel site whose category is not a literal below `3` is not a
/// voice and is skipped. `None` when no site inside the content values its
/// argument - the image has no head cue this scan can name.
///
/// REF: FUN_8004FCC8, FUN_8004FE5C (the two dispatchers the sites call)
pub fn module_head_cue(image: &[u8]) -> Option<ModuleHeadCue> {
    head_cue_site(image, None).map(|(cue, _)| cue)
}

/// [`module_head_cue`] for one body of a multi-spell module: each body is a
/// whole choreography with its own call, so the scan stays inside the framed
/// function at `body_va`. A single body that forks inside its arm (PROT 0946's
/// Call Wave `0x150` against Big Wave `0x151`) is [`forked_head_cue`]'s.
pub fn body_head_cue(image: &[u8], body_va: u32) -> Option<ModuleHeadCue> {
    head_cue_site(image, Some(body_va)).map(|(cue, _)| cue)
}

/// PROT 0953 Terio Punch's head cue off the caster's latch
/// `0x801C8FE0 + (seat + 1) * 4` as arm 0 reads it, before the arm writes it
/// (`0x801F6AFC..0x801F6B04`). Clear takes the charge, whose call is `0x159`
/// (`0x801F6E5C`); set, the punch, `0x15A` (`0x801F6CE0`), the call a scan reads
/// first. `None` for any other module.
pub fn latch_forked_head_cue(entry: u32, latch: i32) -> Option<u16> {
    (entry == 953).then_some(if latch == 0 { 0x159 } else { 0x15A })
}

/// Big Wave's action id, the one PROT 0946 runs on its latch-set branch.
const BIG_WAVE: u8 = 0x56;

/// A head cue that forks on who is casting or what is queued, ahead of the call:
/// a scan reads only the `li a0` nearest the call. `acting` is the acting seat's
/// monster id `DAT_8007BD0C[seat - 3]`, `head` the formation's first, `DAT_8007BD0C[0]`.
/// The outer `None` is no fork; `Some(None)` a caster the fork gives no line.
/// PROT 0944's fork is on its latched roll against the victim's HP, which the cast
/// band holds (`World::guilty_cross_line`).
///
/// - PROT 0940 Glare, `0x801F6ACC..0x801F6AEC`: Zora's line against everyone's.
/// - PROT 0946, `0x801F6AD8..0x801F6B00`: arm 0 forks on the caster's latch
///   `0x801C8FE0 + (seat + 1) * 4`. Clear, it is Call Wave's branch, which sets
///   the latch and calls `0x150` (`0x801F6CF4`); set, Big Wave's, which clears it
///   and calls `0x151` (`0x801F6C2C`), the call a scan reads first. The port keys
///   the branch on the action, as the module's camera does (`capture`: the
///   `zeto_call_wave_mid_cast` recording plays `XA11` channel `0`, `0x150`).
/// - PROT 0947, `0x801F6AD8..0x801F6AEC`: Neo Windhash against V Windhash.
/// - PROT 0955 Terror Scream, `0x801F7758..0x801F77B8`: three monster-id bands.
/// - PROT 0961 and 0962, `0x801F6AC0..0x801F6AD4` and `0x801F6A7C..0x801F6A90`.
/// - PROT 0964's Rogue strikes, `0x801F6BCC..0x801F6C1C`: `0x801F8FE4` holds the
///   strike's element (`capture`, caller log: `0xB0`, `0xB1`, `0xB2`).
pub fn forked_head_cue(entry: u32, action: u8, acting: u16, head: u16) -> Option<Option<u16>> {
    let line = match (entry, action) {
        (940, 0x3C) => Some(if acting == 0xA9 { 0x1B7 } else { 0x192 }),
        (946, _) => Some(if action == BIG_WAVE { 0x151 } else { 0x150 }),
        (947, _) => Some(if action == 0xA7 { 0x19E } else { 0x19B }),
        (955, 0x70) => match acting {
            0x7D..=0x7F => Some(0x190),
            0x83..=0x85 => Some(0x191),
            0x28..=0x2A => Some(0x197),
            _ => None,
        },
        (961, _) => Some(if head == 0xB5 { 0x1C1 } else { 0x18F }),
        (962, _) => Some(if head == 0xB5 { 0x1C0 } else { 0x18E }),
        (964, 0xB0) => Some(0x1C5),
        (964, 0xB1) => Some(0x1C6),
        (964, 0xB2) => Some(0x1C4),
        _ => return None,
    };
    Some(line)
}

/// The arm of the module's phase byte `ctx[+0x279]` whose pass makes the head
/// cue's call. The arm the cue sits in decides when a cast is heard: PROT 0904
/// reaches its call on arm `3`, `119` vsyncs after the arm-2 stream wait begins,
/// PROT 0927 on arm `2` (`capture`, caller log, Theeder and Juggernaut).
///
/// The tick dispatches on the phase with a compare tree or a jump table, so
/// each phase value is walked through the dispatch code from the first
/// `lbu rt,0x279(rs)` of the function that holds the call, to the first
/// instruction that is not dispatch. The cue's arm is the one whose body starts
/// nearest ahead of the call. `None` when the call is outside any such
/// function: the caller then has no arm to wait for.
///
/// REF: FUN_8004FCC8 (the call this places)
pub fn module_head_cue_arm(image: &[u8]) -> Option<u8> {
    head_cue_arm(image, None)
}

/// [`module_head_cue_arm`] for one body of a multi-spell module.
pub fn body_head_cue_arm(image: &[u8], body_va: u32) -> Option<u8> {
    head_cue_arm(image, Some(body_va))
}

fn head_cue_arm(image: &[u8], body_va: Option<u32>) -> Option<u8> {
    let (_, site) = head_cue_site(image, body_va)?;
    site_arm(image, site)
}

/// `CdSync(1)`, the non-blocking poll of the drive's last command.
const JAL_DRIVE_POLL: u32 = jal_word(0x8003_F2B8);

/// The drive wait ahead of a summon module's voice: the arm that seeks the
/// drive to its clip file (`FUN_8003EAE4(0, slot)`), the arm that polls the seek
/// (`FUN_8003F2B8(1)`) and returns busy until it is done, and the seek table
/// index it parks on. Orb's PROT 0911 seeks `0xC` on arm 1 (`0x801F6B98`) and
/// polls on arm 2 (`0x801F6BB8`), where the head cue follows the poll. `None`
/// when the image's content has no literal seek, or no poll in the seek's own
/// function after it.
///
/// REF: FUN_8003EAE4, FUN_8003F2B8
pub fn module_drive_arms(image: &[u8]) -> Option<ModuleDriveArms> {
    let end = match legaia_overlay_images::slot_b_module::content_end(
        image,
        legaia_overlay_images::slot_b_module::SLOT_B_LINK_BASE,
    ) {
        0 => image.len(),
        n => n.min(image.len()),
    };
    let words: Vec<u32> = image[..end & !3]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let (seek_site, slot) = words.iter().enumerate().find_map(|(site, &w)| {
        (w == JAL_DRIVE_SEEK)
            .then(|| match resolve_arg(&words, site, 5) {
                ArgSource::Literal(slot) => Some((site, slot)),
                _ => None,
            })
            .flatten()
    })?;
    let func = legaia_overlay_images::slot_b_module::framed_functions(image)
        .into_iter()
        .find(|f| f.start / 4 <= seek_site && seek_site < f.end / 4)?;
    let poll_site = (seek_site + 1..(func.end / 4).min(words.len())).find(|&site| {
        words[site] == JAL_DRIVE_POLL
            && matches!(resolve_arg(&words, site, 4), ArgSource::Literal(1))
    })?;
    Some(ModuleDriveArms {
        seek_arm: site_arm(image, seek_site)?,
        poll_arm: site_arm(image, poll_site)?,
        slot: u32::from(slot),
    })
}

/// [`module_drive_arms`]'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleDriveArms {
    /// The arm whose pass issues the seek.
    pub seek_arm: u8,
    /// The arm that returns busy until the seek completes.
    pub poll_arm: u8,
    /// The seek table index the seek parks on (`XA<slot + 1>`).
    pub slot: u32,
}

/// The phase arm of `image`'s tick whose body holds word `site`.
fn site_arm(image: &[u8], site: usize) -> Option<u8> {
    let func = legaia_overlay_images::slot_b_module::framed_functions(image)
        .into_iter()
        .find(|f| f.start / 4 <= site && site < f.end / 4)?;
    let words: Vec<u32> = image
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let lo = func.start / 4;
    let head = lo..site.min(lo + DISPATCH_HEAD_WORDS);
    // PROT 0908 reads the byte ahead of its dispatch for another test, and
    // reloads it through a saved `addiu s5, v1, 0x279`: the dispatch is the
    // load whose walk reaches the most arm bodies.
    let pointers: Vec<u32> = words[head.clone()]
        .iter()
        .filter(|&&w| (w >> 26) == 0x09 && w & 0xFFFF == 0x279)
        .map(|&w| (w >> 16) & 0x1F)
        .collect();
    let bodies = head
        .filter(|&i| {
            let w = words[i];
            (w >> 26) == 0x24
                && (w & 0xFFFF == 0x279
                    || (w & 0xFFFF == 0 && pointers.contains(&((w >> 21) & 0x1F))))
        })
        .map(|load| dispatch_bodies(&words, lo, load))
        .max_by_key(|bodies| {
            let mut seen: Vec<usize> = bodies.iter().flatten().copied().collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len()
        })?;
    let mut best: Option<(usize, u8)> = None;
    for (phase, body) in bodies.into_iter().enumerate() {
        let Some(body) = body else { continue };
        if body <= site && best.is_none_or(|(at, _)| body > at) {
            best = Some((body, phase as u8));
        }
    }
    best.map(|(_, phase)| phase)
}

/// The clip file the module's head seek parks the drive on: the slot of the
/// first `FUN_8003EAE4` call in the image's own content that forms it as a
/// literal. PROT 0904's is `7` (`XA8`) and its voice plays from `6` (`XA7`), so
/// that clip starts with a seek; PROT 0927 seeks and plays `0xE`.
///
/// REF: FUN_8003EAE4
pub fn module_head_seek(image: &[u8]) -> Option<u32> {
    let end = match legaia_overlay_images::slot_b_module::content_end(
        image,
        legaia_overlay_images::slot_b_module::SLOT_B_LINK_BASE,
    ) {
        0 => image.len(),
        n => n.min(image.len()),
    };
    let words: Vec<u32> = image[..end & !3]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    words.iter().enumerate().find_map(|(site, &w)| {
        (w == JAL_DRIVE_SEEK)
            .then(|| match resolve_arg(&words, site, 5) {
                ArgSource::Literal(slot) => Some(u32::from(slot)),
                _ => None,
            })
            .flatten()
    })
}

/// The arm body each phase reaches from the phase load at word `load`.
fn dispatch_bodies(words: &[u32], lo: usize, load: usize) -> Vec<Option<usize>> {
    let phase_reg = ((words[load] >> 16) & 0x1F) as usize;
    let mut constants = [None; 32];
    for &w in &words[lo..load] {
        // The prologue's `li`s: PROT 0904's tree compares against one.
        let (op, rs, rt) = (w >> 26, (w >> 21) & 0x1F, ((w >> 16) & 0x1F) as usize);
        if op == 0x09 && rs == 0 {
            constants[rt] = Some(w as u16 as i16 as i32 as u32);
        } else if (0x20..=0x26).contains(&op) {
            constants[rt] = None;
        }
    }
    match table_dispatch(words, load, phase_reg as u32) {
        Some(table) => table.into_iter().map(Some).collect(),
        None => (0..=MAX_MODULE_PHASE)
            .map(|phase| {
                let mut walk = TreeWalk {
                    words,
                    regs: constants,
                    phase_reg,
                    in_head: true,
                };
                walk.set(phase_reg, Some(u32::from(phase)));
                walk.run(load + 1)
            })
            .collect(),
    }
}

const STACK_POINTER: usize = 29;
/// How far into the tick its dispatch's phase load can sit.
const DISPATCH_HEAD_WORDS: usize = 96;
/// The highest phase [`module_head_cue_arm`] tries.
const MAX_MODULE_PHASE: u8 = 63;
/// How many instructions a phase's way through the dispatch may take.
const DISPATCH_WALK_LIMIT: usize = 96;
/// How far past the phase load a jump-table lookup's `jr` can sit.
const TABLE_LOOKUP_SPAN: usize = 12;

/// The tick's jump table, when it dispatches through one: `sltiu` bound,
/// `sll idx, phase, 2`, base into a register, `addu`, `lw`, `jr`. The body
/// address of each phase in bounds.
fn table_dispatch(words: &[u32], load: usize, phase_reg: u32) -> Option<Vec<usize>> {
    let base_va = legaia_overlay_images::slot_b_module::SLOT_B_LINK_BASE;
    let mut known = [None::<u32>; 32];
    let mut index_reg = None;
    let mut slot_reg: Option<(u32, u32)> = None;
    let mut word_reg = None;
    let mut bound = None;
    for &w in words.get(load + 1..load + 1 + TABLE_LOOKUP_SPAN)? {
        let (op, rs, rt, rd) = (
            w >> 26,
            (w >> 21) & 0x1F,
            (w >> 16) & 0x1F,
            (w >> 11) & 0x1F,
        );
        let simm = w as u16 as i16 as i32 as u32;
        match op {
            0x0B if rs == phase_reg => bound = Some(w & 0xFFFF),
            0x0F => known[rt as usize] = Some((w & 0xFFFF) << 16),
            0x09 => known[rt as usize] = known[rs as usize].map(|v: u32| v.wrapping_add(simm)),
            0 if w & 0x3F == 0 && rt == phase_reg && (w >> 6) & 0x1F == 2 => index_reg = Some(rd),
            0 if w & 0x3F == 0x21 => {
                let base = match index_reg {
                    Some(i) if i == rs => known[rt as usize],
                    Some(i) if i == rt => known[rs as usize],
                    _ => None,
                };
                if let Some(b) = base {
                    slot_reg = Some((rd, b));
                }
            }
            0x23 => {
                if let Some((r, b)) = slot_reg
                    && r == rs
                {
                    word_reg = Some((rt, b.wrapping_add(simm)));
                }
            }
            0 if w & 0x3F == 0x08 => {
                let (r, table) = word_reg?;
                if r != rs {
                    return None;
                }
                let at = table.wrapping_sub(base_va) as usize / 4;
                return (0..bound? as usize)
                    .map(|p| {
                        let va = *words.get(at + p)?;
                        Some(va.wrapping_sub(base_va) as usize / 4)
                    })
                    .collect();
            }
            _ => {}
        }
    }
    None
}

/// A phase's way through a compare-tree dispatch: `li`, `slti` / `sltiu` of the
/// phase, `beq` / `bne`, `j`. Anything else is where an arm's body begins: a
/// body opens on anything, and walked on as dispatch it would run away.
struct TreeWalk<'a> {
    words: &'a [u32],
    regs: [Option<u32>; 32],
    phase_reg: usize,
    /// Still ahead of the tree's first branch, where the tick's head loads
    /// its pointers between the phase load and the first compare.
    in_head: bool,
}

impl TreeWalk<'_> {
    fn get(&self, r: usize) -> Option<u32> {
        if r == 0 { Some(0) } else { self.regs[r] }
    }

    fn set(&mut self, r: usize, v: Option<u32>) {
        if r != 0 {
            self.regs[r] = v;
        }
    }

    fn step(&mut self, w: u32) -> bool {
        let (op, rs, rt) = (
            w >> 26,
            ((w >> 21) & 0x1F) as usize,
            ((w >> 16) & 0x1F) as usize,
        );
        let simm = w as u16 as i16 as i32 as u32;
        let a = self.get(rs);
        match op {
            0x09 if rs == 0 => self.set(rt, Some(simm)),
            0x0A if a.is_some() => self.set(rt, a.map(|v| u32::from((v as i32) < simm as i32))),
            0x0B if a.is_some() => self.set(rt, a.map(|v| u32::from(v < simm))),
            0x23 | 0x09 if self.in_head => self.set(rt, None),
            // PROT 0916's tree spills a constant and reloads it mid-way.
            0x23 if rs == STACK_POINTER => self.set(rt, None),
            _ => return false,
        }
        true
    }

    /// Walk from word `pc` to the arm body this phase reaches.
    fn run(&mut self, mut pc: usize) -> Option<usize> {
        let base = legaia_overlay_images::slot_b_module::SLOT_B_LINK_BASE;
        for _ in 0..DISPATCH_WALK_LIMIT {
            let w = *self.words.get(pc)?;
            let (op, rs, rt) = (
                w >> 26,
                ((w >> 21) & 0x1F) as usize,
                ((w >> 16) & 0x1F) as usize,
            );
            let branch_to = (pc as i64 + 1 + i64::from(w as u16 as i16)) as usize;
            let (a, b) = (self.get(rs), self.get(rt));
            let next: Option<usize> = match op {
                0x02 => {
                    Some(((w & 0x03FF_FFFF) << 2).wrapping_sub(base & 0x0FFF_FFFF) as usize / 4)
                }
                // A spilled constant only ever separates arm `1`, past every cue.
                0x04 if rs == self.phase_reg && b.is_none() => Some(pc + 2),
                0x04 => a
                    .zip(b)
                    .map(|(x, y)| if x == y { branch_to } else { pc + 2 }),
                0x05 => a
                    .zip(b)
                    .map(|(x, y)| if x != y { branch_to } else { pc + 2 }),
                _ => {
                    if w == 0 || self.step(w) {
                        pc += 1;
                        continue;
                    }
                    return Some(pc);
                }
            };
            let Some(next) = next else {
                return Some(pc);
            };
            // The delay slot runs either way; an arm's own instruction there
            // changes nothing the tree reads.
            let delay = *self.words.get(pc + 1)?;
            let _ = self.step(delay);
            self.in_head = false;
            pc = next;
        }
        None
    }
}

/// [`module_head_cue`] with the word index of the call it found.
fn head_cue_site(image: &[u8], body_va: Option<u32>) -> Option<(ModuleHeadCue, usize)> {
    // A zero bound is "no layout recognised" (a fixture too small to carry a
    // head table), not "no content": scan the whole image then. Every real
    // band image has a non-zero code end.
    let end = match legaia_overlay_images::slot_b_module::content_end(
        image,
        legaia_overlay_images::slot_b_module::SLOT_B_LINK_BASE,
    ) {
        0 => image.len(),
        n => n.min(image.len()),
    };
    let words: Vec<u32> = image[..end & !3]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    // The body's framed function, when the caller names one.
    let span = body_va.and_then(|va| {
        let at = va.wrapping_sub(legaia_overlay_images::slot_b_module::SLOT_B_LINK_BASE) as usize;
        legaia_overlay_images::slot_b_module::framed_functions(image)
            .into_iter()
            .find(|f| f.start == at)
            .map(|f| f.start / 4..f.end / 4)
    });
    for (site, &w) in words.iter().enumerate() {
        if w != JAL_CUE_DISPATCH && w != JAL_SFX_FUNNEL {
            continue;
        }
        if span.as_ref().is_some_and(|s| !s.contains(&site)) {
            continue;
        }
        if w == JAL_SFX_FUNNEL
            && !matches!(resolve_arg(&words, site, 5), ArgSource::Literal(cat) if cat < 3)
        {
            continue;
        }
        match resolve_arg(&words, site, 4) {
            ArgSource::Literal(id) => return Some((ModuleHeadCue::Literal(id), site)),
            ArgSource::RandPlus(base) => {
                return Some((
                    ModuleHeadCue::Random {
                        base,
                        span: rand_span_before(&words, site),
                    },
                    site,
                ));
            }
            ArgSource::Unknown => continue,
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The dispatcher's CD-XA arm: two gates, then the clip triple
// ---------------------------------------------------------------------------

/// Cue ids at or above this take the dispatcher's CD-XA arm
/// (`sltiu v0,s0,0x100` at `0x8004FCD4`).
pub const XA_CUE_BASE: u16 = 0x100;

/// The two cells the CD-XA arm tests before it starts a clip
/// (`0x8004FCE0..0x8004FD00`; the funnel's voice leg runs the same pair at
/// `0x8004FE84..0x8004FEA4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoiceCueGates {
    /// `ctx[+0x276]`, the battle context's **side-band applier stage** - the
    /// per-turn `summon.dat` / `readef.DAT` streaming machine's phase byte
    /// (`FUN_801DABA4` seeds it `1` each turn, `FUN_801F12D0` steps it and
    /// zeroes it when the acting entity's archive slots are resident;
    /// `docs/formats/summon-readef.md`). Non-zero declines every voice cue.
    /// It is **not** a tutorial flag: no tutorial routine writes it.
    ///
    /// A summon module polls this byte itself before installing its actor
    /// record and only then raises its head cue (PROT 0903: `lbu 0x276`
    /// at `0x801F6CC0`, the cue at `0x801F6E50`), so for the cast's own
    /// voice the gate is structurally open by the time it is tested. The
    /// port's side-band is resident, not streamed, so its window is zero
    /// frames wide and a host passes `0`.
    pub side_band_stage: u8,
    /// `FUN_8003DE7C(1)`'s countdown `gp+0x91C`: vsyncs left in the read span
    /// of the clip the drive last started (`dur` from the starter, stepped
    /// down by the frame-speed byte each poll). Non-zero declines the cue -
    /// a cast while the previous voice is still inside its span plays
    /// nothing, and that is the one decline the port reproduces
    /// (`AudioState::battle_xa_busy_frames`).
    pub clip_span_left: u16,
}

/// The starter's three arguments, `FUN_8003D53C(clip_slot, channel, dur)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceClipRequest {
    /// Clip slot - `XA<slot + 1>.XA` in the boot-built table at `0x801C6ED8`.
    pub clip_slot: u32,
    /// CD-XA channel inside that file's interleave.
    pub channel: u32,
    /// Read span in vsyncs, `(raw * 60 + 99) / 100`.
    pub duration_sectors: u32,
}

/// What the CD-XA arm did with a cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCueVerdict {
    /// `id < 0x100`: the SFX-ring path, not this arm.
    NotVoice,
    /// Declined at `ctx[+0x276] != 0`.
    SideBandStreaming,
    /// Declined at `FUN_8003DE7C(1) != 0`.
    ClipStillSounding,
    /// The span table has no entry for the cue (a disc-free build), or the
    /// entry is zero - the starter would read two sectors of nothing.
    NoSpan,
    /// Start the clip.
    Play(VoiceClipRequest),
}

/// The dispatcher's CD-XA arm (`0x8004FCE0..0x8004FD78`): test the two
/// gates in retail's order, then form the starter's triple - slot
/// `(id - 0x100) >> 3` with the `1 -> 0x1A`, `3 -> 0x1B`, `5 -> 0x1C`
/// remaps applied in sequence off that one value, channel `(id - 0x100) & 7`,
/// span `(raw * 60 + 99) / 100` where `raw` is the caller's read of
/// `DAT_800788B8[id - 0x100]` (`legaia_game_tables::xa_cue_table`).
///
/// PORT: FUN_8004FCC8
pub fn admit_voice_cue(id: u16, gates: VoiceCueGates, raw_span: Option<u16>) -> VoiceCueVerdict {
    if id < XA_CUE_BASE {
        return VoiceCueVerdict::NotVoice;
    }
    if gates.side_band_stage != 0 {
        return VoiceCueVerdict::SideBandStreaming;
    }
    if gates.clip_span_left != 0 {
        return VoiceCueVerdict::ClipStillSounding;
    }
    let Some(raw) = raw_span.filter(|&r| r != 0) else {
        return VoiceCueVerdict::NoSpan;
    };
    let v = u32::from(id - XA_CUE_BASE);
    let clip_slot = match v >> 3 {
        1 => 0x1A,
        3 => 0x1B,
        5 => 0x1C,
        other => other,
    };
    VoiceCueVerdict::Play(VoiceClipRequest {
        clip_slot,
        channel: v & 7,
        duration_sectors: (u32::from(raw) * 60).div_ceil(100),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caller logs: Rogue's three strikes, Terror Scream by a Terror Face.
    /// A monster outside Terror Scream's bands has no line.
    #[test]
    fn a_forked_head_cue_follows_the_caster_and_the_action() {
        assert_eq!(forked_head_cue(964, 0xB0, 1, 1), Some(Some(0x1C5)));
        assert_eq!(forked_head_cue(964, 0xB2, 1, 1), Some(Some(0x1C4)));
        assert_eq!(forked_head_cue(964, 0xAF, 1, 1), None);
        assert_eq!(forked_head_cue(955, 0x70, 0x7E, 1), Some(Some(0x190)));
        assert_eq!(forked_head_cue(955, 0x70, 0x50, 1), Some(None));
        assert_eq!(forked_head_cue(947, 0x57, 1, 1), Some(Some(0x19B)));
        assert_eq!(forked_head_cue(946, 0x55, 1, 1), Some(Some(0x150)));
        assert_eq!(latch_forked_head_cue(953, 0), Some(0x159));
        assert_eq!(latch_forked_head_cue(953, 1), Some(0x15A));
        assert_eq!(latch_forked_head_cue(946, 0), None);
        assert_eq!(forked_head_cue(946, BIG_WAVE, 1, 1), Some(Some(0x151)));
        assert_eq!(forked_head_cue(940, 0x3C, 0xA9, 1), Some(Some(0x1B7)));
        assert_eq!(forked_head_cue(940, 0x3C, 0x10, 1), Some(Some(0x192)));
    }

    #[test]
    fn enemy_leg_band() {
        for class in 0..=2 {
            assert_eq!(
                cast_audio_cue(RetailSlot::new(3), 0, class, 0, 0),
                CastCueOutcome::Sfx(0x20C)
            );
        }
        assert_eq!(
            cast_audio_cue(RetailSlot::new(3), 0, 3, 0, 0),
            CastCueOutcome::Sfx(0x20D)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(3), 0, 8, 0, 0),
            CastCueOutcome::Sfx(0x20D)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(3), 0, 4, 0, 0),
            CastCueOutcome::Sfx(0x20E)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(3), 0, 5, 0, 0),
            CastCueOutcome::None
        );
        // char-kind 4 routes to the enemy leg even on a player slot.
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 4, 0, 0, 0),
            CastCueOutcome::Sfx(0x20C)
        );
    }

    #[test]
    fn player_leg_per_character_band() {
        // char_kind 2 (Noa in the 0x8007BD10 kind space): base 0x20.
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 0, 0, 0),
            CastCueOutcome::Sfx(0x118)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 1, 0, 0),
            CastCueOutcome::Sfx(0x118)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 2, 0, 0),
            CastCueOutcome::Sfx(0x119)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 3, 0, 0),
            CastCueOutcome::Sfx(0x11A)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 8, 0, 0),
            CastCueOutcome::Sfx(0x11A)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 4, 0, 0),
            CastCueOutcome::Sfx(0x11B)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 5, 0, 0),
            CastCueOutcome::Sfx(0x11C)
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 6, 0, 0),
            CastCueOutcome::None
        );
    }

    #[test]
    fn class_7_gates_on_sub_class() {
        for sub in 1..=4u8 {
            assert_eq!(
                cast_audio_cue(RetailSlot::new(0), 1, 7, 0, sub),
                CastCueOutcome::Sfx(0x10C)
            );
        }
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 1, 7, 0, 0),
            CastCueOutcome::None
        );
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 1, 7, 0, 5),
            CastCueOutcome::None
        );
    }

    #[test]
    fn item_give_special_precedes_class_dispatch() {
        assert_eq!(
            cast_audio_cue(RetailSlot::new(0), 2, 0, 0xFE, 0),
            CastCueOutcome::ItemGive { voice_arg: 0x1B }
        );
        // But not on the enemy leg.
        assert_eq!(
            cast_audio_cue(RetailSlot::new(3), 0, 0, 0xFE, 0),
            CastCueOutcome::Sfx(0x20C)
        );
    }

    // --- the module head cue ------------------------------------------------

    fn le(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// `li a0, imm` = `addiu a0, zero, imm`.
    fn li_a0(imm: u16) -> u32 {
        0x2404_0000 | u32::from(imm)
    }

    #[test]
    fn head_cue_reads_the_literal_in_the_delay_slot() {
        // nop; jal FUN_8004FCC8; _li a0,0x134; nop
        let img = le(&[0, JAL_CUE_DISPATCH, li_a0(0x134), 0]);
        assert_eq!(module_head_cue(&img), Some(ModuleHeadCue::Literal(0x134)));
    }

    #[test]
    fn head_cue_reads_the_literal_formed_before_the_call() {
        // li a0,0x161; li a1,1; jal FUN_8004FCC8; _nop
        let img = le(&[li_a0(0x161), 0x2405_0001, JAL_CUE_DISPATCH, 0]);
        assert_eq!(module_head_cue(&img), Some(ModuleHeadCue::Literal(0x161)));
        // `ori a0, zero, imm` is the same literal.
        let img = le(&[0x3404_0155, JAL_CUE_DISPATCH, 0]);
        assert_eq!(module_head_cue(&img), Some(ModuleHeadCue::Literal(0x155)));
    }

    #[test]
    fn head_cue_names_the_coin_flip_of_the_two_random_modules() {
        // jal rand; _nop; li v1,2; div v0,v1; mfhi v0; addiu a0,v0,0x1b0;
        // jal FUN_8004FCC8; _nop
        let img = le(&[
            JAL_BIOS_RAND,
            0,
            0x2403_0002,
            0x0043_001A,
            0x0000_2010,
            0x2444_01B0,
            JAL_CUE_DISPATCH,
            0,
        ]);
        let head = module_head_cue(&img).unwrap();
        assert_eq!(
            head,
            ModuleHeadCue::Random {
                base: 0x1B0,
                span: 2
            }
        );
        assert_eq!(head.resolve(|| 7), 0x1B1);
        assert_eq!(head.resolve(|| 8), 0x1B0);
        // `addiu a0, v0, imm` with no `rand` in the window is not a coin
        // flip - it is a value the scan cannot name.
        let img = le(&[0x2444_01B0, JAL_CUE_DISPATCH, 0]);
        assert_eq!(module_head_cue(&img), None);
    }

    #[test]
    fn head_cue_skips_a_funnel_site_outside_the_voice_leg() {
        // The funnel with category 3 (a monster seat) takes the ring path:
        // not a voice, so the scan moves on to the next site.
        let img = le(&[
            li_a0(0x155),
            0x2405_0003,
            JAL_SFX_FUNNEL,
            0,
            0,
            JAL_CUE_DISPATCH,
            li_a0(0x1A8),
        ]);
        assert_eq!(module_head_cue(&img), Some(ModuleHeadCue::Literal(0x1A8)));
        // Category 1: the voice leg, and the head.
        let img = le(&[li_a0(0x155), 0x2405_0001, JAL_SFX_FUNNEL, 0]);
        assert_eq!(module_head_cue(&img), Some(ModuleHeadCue::Literal(0x155)));
    }

    #[test]
    fn head_cue_never_guesses_a_register_it_cannot_value() {
        // move a0, s0 (addu a0, zero, s0): unknown, and there is no other site.
        let img = le(&[0x0010_2021, JAL_CUE_DISPATCH, 0]);
        assert_eq!(module_head_cue(&img), None);
        // A stale literal behind an unknown write is not read past the write.
        let img = le(&[li_a0(0x134), 0x0010_2021, JAL_CUE_DISPATCH, 0]);
        assert_eq!(module_head_cue(&img), None);
        assert_eq!(module_head_cue(&[]), None);
    }

    // --- the CD-XA arm's gates ------------------------------------------------

    #[test]
    fn voice_arm_tests_the_two_gates_in_retail_order() {
        let open = VoiceCueGates::default();
        assert_eq!(
            admit_voice_cue(0x21, open, Some(9)),
            VoiceCueVerdict::NotVoice
        );
        assert_eq!(
            admit_voice_cue(
                0x134,
                VoiceCueGates {
                    side_band_stage: 3,
                    clip_span_left: 5
                },
                Some(1143)
            ),
            VoiceCueVerdict::SideBandStreaming,
            "ctx[+0x276] is tested first"
        );
        assert_eq!(
            admit_voice_cue(
                0x134,
                VoiceCueGates {
                    side_band_stage: 0,
                    clip_span_left: 5
                },
                Some(1143)
            ),
            VoiceCueVerdict::ClipStillSounding
        );
        assert_eq!(admit_voice_cue(0x134, open, None), VoiceCueVerdict::NoSpan);
        assert_eq!(
            admit_voice_cue(0x134, open, Some(0)),
            VoiceCueVerdict::NoSpan
        );
    }

    #[test]
    fn voice_arm_forms_the_two_captured_starter_triples() {
        // PROT 0903 -> cue 0x134 -> FUN_8003D53C(6, 4, 686): raw 1143.
        assert_eq!(
            admit_voice_cue(0x134, VoiceCueGates::default(), Some(1143)),
            VoiceCueVerdict::Play(VoiceClipRequest {
                clip_slot: 6,
                channel: 4,
                duration_sectors: 686
            })
        );
        // PROT 0905 -> cue 0x131 -> (6, 1, 568): raw 946.
        assert_eq!(
            admit_voice_cue(0x131, VoiceCueGates::default(), Some(946)),
            VoiceCueVerdict::Play(VoiceClipRequest {
                clip_slot: 6,
                channel: 1,
                duration_sectors: 568
            })
        );
        // The three low-slot remaps, applied in sequence off one value.
        let slot = |id: u16| match admit_voice_cue(id, VoiceCueGates::default(), Some(100)) {
            VoiceCueVerdict::Play(r) => r.clip_slot,
            other => panic!("{other:?}"),
        };
        assert_eq!(slot(0x108), 0x1A);
        assert_eq!(slot(0x118), 0x1B);
        assert_eq!(slot(0x128), 0x1C);
        assert_eq!(slot(0x120), 4);
    }
}
