//! Signature arts: every slot gets its sibling's art, fights in its element,
//! and the effect script sits in the bank the runtime walks.

use super::cast_bed::stream_speed;
use super::load_disc;
use super::move_modes::{SLOTS, run_mapped};
use legaia_delilas_party::delilas_party::{
    CastRoutePolicy, DelilasMoveMode as MoveMode, PartyMapping, Sibling, apply_delilas_party,
};
use legaia_delilas_party::delilas_voice_fx::ArtsVoiceMode;
use legaia_disc_patch::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// Every hero slot gets its mapped sibling's signature special: renamed,
/// re-combo'd, and driven by that sibling's own choreography instead of
/// the host art's.
///
/// The assertions are chosen to fail on retail. A shape check would not
/// be: the host streams are already 15-part, so "15 parts" passes on an
/// unpatched disc and proves nothing. What cannot happen by accident is
/// the frame count moving from the host's to the sibling clip's, and the
/// effect script losing the host's hand-authored flame.
#[test]
fn every_slot_gets_its_siblings_signature_art() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_actor_formats::battle_char_assembly as bca;
    use legaia_delilas_party::delilas_party::READEF_ENTRY;
    use legaia_party_swap::winpose;

    /// One hero slot's expected reskin. Combo glyphs: 1 L, 2 R, 3 D, 4 U,
    /// so the three retail combos read R D L D L / L L R L R / R R L L L.
    struct Expect {
        slot: usize,
        /// The host art's retail name, which must lose exactly one site.
        host_name: &'static str,
        /// The signature name replacing it, which must gain exactly one.
        sig_name: &'static str,
        monster_id: u16,
        /// Monster-archive entry indices of the sibling's signature
        /// chain, in play order; the last is the payoff stage.
        chain: &'static [usize],
        /// The host art's retail combo - how its bank record is addressed.
        host_combo: &'static [u8],
        /// The art's action constant - the attack-camera table row.
        action_constant: u8,
    }
    let expect = [
        Expect {
            slot: 0,
            host_name: "Burning Flare",
            sig_name: "Plasma Strike",
            monster_id: 164,
            chain: &[14, 12, 13],
            host_combo: &[2, 3, 1, 3, 1],
            action_constant: 0x1C,
        },
        Expect {
            slot: 1,
            host_name: "Vulture Blade",
            sig_name: "Blazing Slash",
            monster_id: 162,
            chain: &[10, 11, 12],
            host_combo: &[1, 1, 2, 1, 2],
            action_constant: 0x1F,
        },
        Expect {
            slot: 2,
            host_name: "Explosive Fist",
            sig_name: "Megaton Press",
            monster_id: 163,
            chain: &[10, 11],
            host_combo: &[2, 2, 1, 1, 1],
            action_constant: 0x1C,
        },
    ];

    let mapping = PartyMapping::parse("lu,gi,che").expect("parse mapping");
    let retail = DiscPatcher::open(original.clone()).expect("open disc");
    let retail_scus = retail
        .read_named_file("SCUS_942.54")
        .expect("read retail SCUS");
    let retail_readef = retail
        .read_entry_footprint(READEF_ENTRY)
        .expect("read retail readef");
    let archive = retail
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("read archive");

    let mut patcher = DiscPatcher::open(original).expect("open disc");
    apply_delilas_party(
        &mut patcher,
        &mapping,
        ArtsVoiceMode::default(),
        MoveMode::default(),
        CastRoutePolicy::Install,
    )
    .expect("apply");
    let patched = DiscPatcher::open(patcher.into_image()).expect("re-open");
    let scus = patched.read_named_file("SCUS_942.54").expect("read SCUS");
    let readef = patched
        .read_entry_footprint(READEF_ENTRY)
        .expect("read readef");

    let occurrences = |hay: &[u8], needle: &str| -> usize {
        hay.windows(needle.len())
            .filter(|w| *w == needle.as_bytes())
            .count()
    };

    for Expect {
        slot,
        host_name,
        sig_name,
        monster_id,
        chain,
        host_combo,
        action_constant,
    } in expect
    {
        // The arts-table record gives up the host name to carry the
        // signature, and the sibling's spell row KEEPS the signature
        // (the enemy-side rename is retired: the mirrored hero's
        // signature is a physical attack, so the row's only reader is
        // the player-side state-0x28 cast banner, which must show the
        // special's own name). Net: the signature name gains exactly
        // the arts-record site, and the host name loses its only one.
        // Pinning counts still catches collateral text-replace damage -
        // these strings nest (searching `Hurricane` finds `Hurricane
        // Kick`), so a rename that went through the image as text
        // rather than through each table's own pointer drifts a count.
        assert_eq!(
            occurrences(&scus, sig_name),
            occurrences(&retail_scus, sig_name) + 1,
            "slot {slot}: {sig_name} should gain exactly the arts-record site"
        );
        assert_eq!(
            occurrences(&scus, host_name),
            occurrences(&retail_scus, host_name) - 1,
            "slot {slot}: {host_name} should lose its arts-record site"
        );

        // The host art's stream now carries the sibling's clip at the
        // clip's OWN length - not the host stream's retail length.
        let host_rec = {
            let entry = retail
                .read_entry(863 + slot)
                .expect("read retail player file");
            let rec0 = bca::decode_record0(&entry).expect("decode record0");
            let bank = bca::art_animation_bank(&rec0).expect("art bank");
            // Addressed by the retail combo, never by the inline name -
            // that field is a dev label ("Fiery Miyawaki"), not the art's
            // display name, so a name match silently finds nothing.
            let matches: Vec<_> = bank
                .iter()
                .filter(|r| !r.uses_base_archive() && r.combo == host_combo)
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "slot {slot}: {host_name}'s combo should address exactly one bank record"
            );
            matches[0].clone()
        };
        // The new combo must not occur INSIDE any of that character's
        // other arts, at any offset. Equality is the wrong test: the
        // retail matcher walks the scan start index downward, so a rival
        // art matching at offset >= 1 wins outright, and an ordinary art
        // does not consume its run - which is how `L R L R D` used to
        // fire Gala's Battering Ram (`L R D`, offset 2) plus Back Punch
        // instead of his signature.
        {
            let entry = patched
                .read_entry(863 + slot)
                .expect("read patched player file");
            let rec0 = bca::decode_record0(&entry).expect("decode patched record0");
            let bank = bca::art_animation_bank(&rec0).expect("patched art bank");
            // Glyphs 1 L, 2 R, 3 D, 4 U.
            const SIGNATURE_COMBO: [u8; 5] = [1, 2, 4, 4, 3];
            assert!(
                bank.iter()
                    .any(|r| !r.uses_base_archive() && r.combo == SIGNATURE_COMBO),
                "slot {slot}: no bank record answers to the signature combo"
            );
            assert!(
                !bank.iter().any(|r| r.combo == host_combo),
                "slot {slot}: the retail combo {host_combo:?} still matches something"
            );
            // Art rows start at bank record 11 - where the retail matcher
            // starts its own scan (`li s3,0xb` in `FUN_801EED1C`) - and an
            // art input is at least three directions (the shortest on the
            // disc are `RRL` / `LLD` / `UDL` / `RLD` / `LUU`, one per
            // character). Shorter rows are starters and basic swings;
            // including them flags every combo that contains any single
            // direction, i.e. all of them.
            for other in bank
                .iter()
                .skip(11)
                .filter(|r| !r.uses_base_archive() && r.combo.len() >= 3)
            {
                if other.combo == SIGNATURE_COMBO {
                    continue;
                }
                assert!(
                    !SIGNATURE_COMBO
                        .windows(other.combo.len())
                        .any(|w| w == other.combo.as_slice()),
                    "slot {slot}: {:?} occurs inside the signature combo - it would \
                     match first, and an ordinary art does not consume its run",
                    other.combo
                );
            }
        }

        let anims = legaia_actor_formats::monster_archive::animations(&archive, monster_id)
            .expect("animations")
            .expect("archive slot");
        let stages: Vec<&legaia_actor_formats::monster_archive::MonsterAnimation> = chain
            .iter()
            .map(|&i| anims.get(i).expect("chain stage"))
            .collect();
        assert!(
            !stages.is_empty(),
            "slot {slot}: chain {chain:?} has no stages"
        );

        let off = bca::art_me_slot(slot, false) * winpose::READEF_SLOT;
        let src = host_rec.stream_source as usize;
        let before = winpose::art_entry_shape(&retail_readef[off..off + winpose::READEF_SLOT], src)
            .expect("retail shape");
        let after = winpose::art_entry_shape(&readef[off..off + winpose::READEF_SLOT], src)
            .expect("patched shape");
        assert_eq!(before.0, after.0, "slot {slot}: part count must not move");
        assert_ne!(
            after.1, before.1,
            "slot {slot}: frame count identical to retail - the retarget did not land"
        );
        // The patched art record: the rate the stream is written at, and
        // the hit frames scheduled against it.
        let patched_rec = {
            let entry = patched
                .read_entry(863 + slot)
                .expect("read patched player file");
            let rec0 = bca::decode_record0(&entry).expect("decode patched record0");
            let bank = bca::art_animation_bank(&rec0).expect("patched art bank");
            bank.into_iter()
                .find(|r| !r.uses_base_archive() && r.combo == [1, 2, 4, 4, 3])
                .expect("the signature record")
        };

        // The whole chain, not just the payoff, measured as DURATION
        // rather than frame count. A clip runs `frames * 8 / rate` ticks,
        // so the rebuild is free to trade keyframe density for room (it
        // halves the rate and the count together when a slot is tight)
        // and a raw frame-count test would read that as a lost stage.
        // Duration is what actually has to survive: every stage's
        // authored playing time, still there.
        let want: f64 = stages
            .iter()
            .map(|c| c.frame_count as f64 / c.rate.max(1) as f64)
            .sum();
        let got = after.1 as f64 / patched_rec.rate.max(1) as f64;
        assert!(
            (got - want).abs() / want < 0.10,
            "slot {slot}: stream runs {got:.1} units against the chain's authored {want:.1} \
             - the wind-up stages {chain:?} did not land"
        );

        // Where the payoff stage begins in the rebuilt stream, derived
        // the same way the rebuild derives it: each stage stretched from
        // its own rate to the stream's.
        let rate = patched_rec.rate.max(1) as usize;
        let payoff_start: usize = stages[..stages.len() - 1]
            .iter()
            .map(|c| {
                (c.frame_count * rate)
                    .div_ceil(c.rate.max(1) as usize)
                    .max(1)
            })
            .sum();

        // Every hit lands on the strike, not in the wind-up, and the
        // number of them is untouched - the count is how many times the
        // action applies damage, so moving it would change the move.
        let hits = |r: &bca::ArtAnimRecord| -> Vec<u8> {
            (0..4)
                .filter_map(|i| r.effect_script.get(0x10 + i).copied())
                .filter(|&f| f != 0)
                .collect()
        };
        let (was, now) = (hits(&host_rec), hits(&patched_rec));
        assert_eq!(
            was.len(),
            now.len(),
            "slot {slot}: hit count moved {was:?} -> {now:?}"
        );

        // The host's impact-effect class is off. Entry `+0x7A` drives two
        // renderers that live outside the 8-record effect script - the
        // swing-path element spark (`FUN_8004998C`) and the character-
        // tinted afterimages (`FUN_80049348`) - so a non-zero value here
        // paints the sibling's move in the HOST's element no matter what
        // the effect script says. Vahn's Burning Flare ships `1`; that is
        // what put fire on Lu.
        assert_eq!(
            patched_rec.impact_class, 0,
            "slot {slot}: the host art's impact-effect class survived \
             (retail {}) - the sibling wears the host's element sparks \
             and afterimage tint",
            host_rec.impact_class
        );
        if slot == 0 {
            // Keeps the check above honest: Vahn's Burning Flare is the
            // one host art of the three that sets a class at all, so if
            // retail ever read 0 here the zeroing would assert nothing.
            assert_ne!(
                host_rec.impact_class, 0,
                "retail Burning Flare no longer sets an impact-effect \
                 class - the assertion above has gone vacuous"
            );
        }
        // The hits sit on the frames the rebuilt stream actually
        // CONNECTS on, measured off that stream rather than off the
        // patcher's own chain arithmetic. Anchoring them on the payoff
        // stage's start - the assertion this replaces - was satisfied by
        // exactly the frames that were too late: the payoff stage opens
        // with its own approach, so the first application landed
        // 1.1-3.0 s after the body connected in every pairing.
        //
        // A connect is where the whole body arrests: the per-frame mean
        // part translation delta falls. So every hit must be a frame the
        // stream decelerates INTO, and the first one must land in the
        // stream's first two thirds (the "all four crammed on the tail"
        // signature the old anchor produced).
        {
            let speed = stream_speed(&readef, slot, src);
            assert!(
                speed.len() + 1 >= after.1,
                "slot {slot}: decoded {} speed samples for a {}-frame stream",
                speed.len(),
                after.1
            );
            let mean = speed.iter().sum::<f64>() / speed.len().max(1) as f64;
            let first = *now.first().expect("a first hit") as usize;
            assert!(
                first * 3 < after.1 * 2,
                "slot {slot}: first damage at frame {first} of {} - the whole \
                 pattern sits on the tail. hits {now:?}",
                after.1
            );
            // What a hit must sit on depends on what the chain knows
            // about itself. A stage in the damaging tag band carries its
            // own contact beats on disc (Lu's strike stages do), and those
            // are authority - they need not be deceleration frames, since
            // a lunge connects at speed. A stage retail gave no beats to
            // (Gi's and Che's signature stages are tag 0x23, damaged by
            // their cast modules instead) has only its motion to go on, so
            // there the hit must be a frame the body arrests into.
            let rate = patched_rec.rate.max(1) as usize;
            let mut authored: Vec<usize> = Vec::new();
            let mut start = 0usize;
            for st in &stages {
                let len = (st.frame_count * rate)
                    .div_ceil(st.rate.max(1) as usize)
                    .max(1);
                if (0x0C..=0x1F).contains(&st.action_id) {
                    for i in 0..4 {
                        let f = st.effect_script.get(0x10 + i).copied().unwrap_or(0);
                        if f != 0 {
                            authored
                                .push(start + (f as usize * len).div_ceil(st.frame_count.max(1)));
                        }
                    }
                }
                start += len;
            }
            let mut on_impact = 0;
            for (i, h) in now.iter().enumerate() {
                let h = *h as usize;
                assert!(
                    h >= 1 && h < after.1,
                    "slot {slot}: hit at frame {h} is outside the {}-frame stream",
                    after.1
                );
                let rides_previous = i > 0 && h == now[i - 1] as usize + 1;
                let lands = if authored.is_empty() {
                    h >= 2 && speed[h - 1] < speed[h - 2]
                } else {
                    authored.contains(&h)
                };
                if lands {
                    on_impact += 1;
                }
                assert!(
                    lands || rides_previous,
                    "slot {slot}: hit {i} at frame {h} is not on a connect - \
                     {}. hits {now:?}, stream mean speed {mean:.1}",
                    if authored.is_empty() {
                        format!(
                            "the body is still accelerating there ({:.1} -> {:.1})",
                            speed[h.saturating_sub(2)],
                            speed[h - 1]
                        )
                    } else {
                        format!("the chain's authored beats are {authored:?}")
                    }
                );
            }
            assert!(
                on_impact > 0,
                "slot {slot}: no hit lands on a deceleration at all. hits {now:?}"
            );
        }

        // The sibling's spell row keeps its retail name. No enemy ever
        // casts it (the AI picker's cast arm is rewritten into a
        // physical attack), and the player-side cast banner - the
        // state-0x28 label the un-gate opened to Magic casts - reads
        // exactly this row when the converted signature fires, so it
        // must announce the special itself, not the host art.
        {
            let table = legaia_game_tables::spell_names::SpellNameTable::from_scus(&scus)
                .expect("patched spell table");
            let id = (monster_id - 0x29) as u8;
            assert_eq!(
                table.name(id),
                Some(sig_name),
                "slot {slot}: the cast banner row {id:#04X} should announce the special"
            );
        }

        // The battle idle carries the sibling's own combat stance, at
        // exactly its retail byte length (the stream is inline in a
        // record[0] whose every later offset would otherwise move), and
        // anchored back onto the host's rest by one rigid whole-body
        // translation - every clip the swap does NOT rebuild (walk,
        // flinch, block, get-up) still starts from the host's rest, so an
        // un-anchored idle pops the whole body at every transition.
        //
        // The anchor's two references are what this checks (see
        // `winpose::idle_anchor`): the torso's x/z land exactly on the
        // host rest, and the deepest ankle over the cycle lands exactly
        // on the host idle's, so the character stands on the ground. The
        // torso's HEIGHT is deliberately not the host's - that difference
        // is the stance (Lu stands taller than Vahn, Che crouches).
        {
            let retail_idle = bca::idle_battle_animation(
                &retail.read_entry(863 + slot).expect("retail player file"),
            )
            .expect("retail idle")
            .expect("retail has an idle");
            let new_idle = bca::idle_battle_animation(
                &patched.read_entry(863 + slot).expect("patched player file"),
            )
            .expect("patched idle")
            .expect("patched has an idle");
            assert_eq!(
                (retail_idle.part_count, retail_idle.frame_count),
                (new_idle.part_count, new_idle.frame_count),
                "slot {slot}: idle shape moved - the stream is inline, so its length is fixed"
            );
            assert_ne!(
                retail_idle.frames, new_idle.frames,
                "slot {slot}: idle is byte-identical to retail - the sibling stance did not land"
            );
            let (was, now) = (retail_idle.frames[0][0], new_idle.frames[0][0]);
            assert_eq!(
                (was.tx, was.tz),
                (now.tx, now.tz),
                "slot {slot}: idle frame 0 moved the torso off the host rest ground spot"
            );
            let rig = if slot == 1 {
                &legaia_party_swap::RIG_NOA
            } else {
                &legaia_party_swap::RIG_VAHN_GALA
            };
            let feet = [
                rig.channel_for_canonical[11] as usize,
                rig.channel_for_canonical[14] as usize,
            ];
            // GTE space is y-down, so the deepest contact is the LARGEST y.
            let floor = |a: &legaia_actor_formats::monster_archive::MonsterAnimation| {
                a.frames
                    .iter()
                    .map(|f| feet.iter().map(|&c| f[c].ty).max().unwrap())
                    .max()
                    .unwrap()
            };
            assert_eq!(
                floor(&retail_idle),
                floor(&new_idle),
                "slot {slot}: the swapped idle does not plant its feet on the host's floor"
            );
        }

        // The attack camera is re-timed to the swing it now films: the
        // arm this art dispatches to must be reachable from exactly one
        // live table slot (or re-timing it would mistune another art),
        // and its last cursor threshold must sit on the payoff frame.
        {
            const TABLES: [(usize, usize); 3] = [(0x270, 17), (0x2B8, 20), (0x308, 17)];
            const BASE: u32 = 0x801C_E818;
            const EPILOGUE: u32 = 0x801D_828C;
            let ov = patched.read_entry(898).expect("battle overlay");
            let word = |o: usize| u32::from_le_bytes(ov[o..o + 4].try_into().unwrap());
            let (tb, tn) = TABLES[slot];
            let row = action_constant as usize - 0x1A;
            assert!(
                row < tn,
                "slot {slot}: art constant {action_constant:#04X} is past the table"
            );
            let arm = word(tb + row * 4);
            assert!(
                arm != 0 && arm != EPILOGUE,
                "slot {slot}: art has no camera arm"
            );
            let uses = TABLES
                .iter()
                .flat_map(|&(b, n)| (0..n).map(move |r| b + r * 4))
                .filter(|&o| word(o) == arm)
                .count();
            assert_eq!(
                uses, 1,
                "slot {slot}: arm {arm:#010X} is reached from {uses} table slots - \
                 re-timing it would mistune another art"
            );
            let mut ends: Vec<u32> = TABLES
                .iter()
                .flat_map(|&(b, n)| (0..n).map(move |r| b + r * 4))
                .map(word)
                .filter(|&a| a != 0 && a != EPILOGUE)
                .collect();
            ends.sort_unstable();
            let end = ends.iter().copied().find(|&a| a > arm).unwrap_or(EPILOGUE);
            let mut cursor = [false; 32];
            let mut shots = Vec::new();
            for o in ((arm - BASE) as usize..(end - BASE) as usize).step_by(4) {
                let w = word(o);
                let (op, rs, rt, imm) = (w >> 26, (w >> 21) & 0x1F, (w >> 16) & 0x1F, w & 0xFFFF);
                match op {
                    0x21 | 0x25 if imm == 0x68 => cursor[rt as usize] = true,
                    0x0A if cursor[rs as usize] && (0x10..=0x4000).contains(&imm) => {
                        shots.push(imm)
                    }
                    _ => {}
                }
            }
            let last = *shots.iter().max().expect("a cursor-gated shot change");
            // Sixteenths of a keyframe; one keyframe of slack for the
            // integer rounding the scale factor carries.
            let want = (payoff_start * 16) as i64;
            assert!(
                (last as i64 - want).abs() <= 16,
                "slot {slot}: arm {arm:#010X} last shot change at cursor {last} \
                 (kf {}), payoff starts at kf {payoff_start} - the camera finishes \
                 its choreography before the strike. shots {shots:?}",
                last / 16
            );
        }
    }
}

/// Each hero slot fights in the **mapped sibling's** element, not the
/// host character's.
///
/// The battle overlay's per-character element table (`0x801F5480`) is the
/// only per-character element on the disc and the one both affinity
/// readers index (`FUN_801DD864` at `0x801dd8ac`/`0x801dd900`,
/// `FUN_801EC3E4` at `0x801ecf38`/`0x801ecf94`), so until it moves a
/// swapped party deals and takes the *host's* element - Lu's Plasma
/// Strike landing as fire out of Vahn's slot, Che's Megaton Press as
/// thunder out of Gala's.
///
/// Proved by contrast against retail rather than against a literal table:
/// every slot must read the sibling's own `+0x1D` byte, at least one must
/// have MOVED off retail (the assertion is vacuous if the two agree
/// everywhere), and the five characters the swap does not touch must be
/// byte-identical to retail.
///
/// Run on a rearranged mapping as well as the default: the table is
/// indexed by slot and the value comes from the sibling, so a pass that
/// wrote the right element to the wrong slot would still satisfy the
/// default if the default happened to be the identity permutation.
#[test]
fn every_slot_fights_in_its_siblings_element() {
    use legaia_game_tables::element_affinity as ea;
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let retail_ov = retail.read_entry(898).expect("retail battle overlay");
    let archive = retail
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    let sibling_element = |s: Sibling| -> u8 {
        legaia_actor_formats::monster_archive::record(&archive, s.monster_id())
            .expect("record")
            .expect("populated")
            .element
    };

    for mapping in [
        PartyMapping::default(),
        PartyMapping {
            vahn: Sibling::Lu,
            noa: Sibling::Gi,
            gala: Sibling::Che,
        },
    ] {
        let (patched, notes) = run_mapped(&original, MoveMode::Hybrid, mapping);
        let ov = patched.read_entry(898).expect("patched battle overlay");
        let want = [mapping.vahn, mapping.noa, mapping.gala];
        let mut moved = 0;
        for (slot, sibling) in want.iter().copied().enumerate() {
            let at = ea::CHARACTER_ELEMENTS_FILE_OFFSET + slot;
            let got = ov[at];
            let expect = sibling_element(sibling);
            assert_eq!(
                got,
                expect,
                "slot {slot}: element {got} ({:?}), expected {}'s own {expect} ({:?})",
                ea::Element::from_id(got).map(|e| e.name()),
                sibling.display_name(),
                ea::Element::from_id(expect).map(|e| e.name()),
            );
            if got != retail_ov[at] {
                moved += 1;
            }
        }
        assert!(
            moved > 0,
            "no slot's element moved off retail - the assertion is vacuous"
        );
        // The characters outside the party swap keep retail's elements:
        // the table has eight rows and the pass owns exactly three.
        for i in 3..ea::CHARACTER_ELEMENTS_LEN {
            let at = ea::CHARACTER_ELEMENTS_FILE_OFFSET + i;
            assert_eq!(
                ov[at],
                retail_ov[at],
                "character {} element moved, and the swap owns only slots 0..3",
                i + 1
            );
        }
        // The affinity matrix itself is untouched - this pass moves who
        // is what element, never what an element does.
        let m = ea::AFFINITY_MATRIX_FILE_OFFSET;
        let n = ea::ELEMENT_COUNT * ea::ELEMENT_COUNT;
        assert_eq!(
            &ov[m..m + n],
            &retail_ov[m..m + n],
            "the element-affinity matrix moved"
        );
        assert_eq!(
            notes.iter().filter(|n| n.contains(" element: ")).count(),
            3,
            "the report should name one element per slot: {notes:?}"
        );
    }
}

/// The runtime reads the signature art's effect script out of exactly the
/// bytes the reskin edits.
///
/// The chain is `DAT_801C9360[char]` -> the decoded `record[0]` image ->
/// `record0[+0x58]` -> `bank + 4 + row*0xD0` -> `+0x24` -> `+0x14`
/// (`FUN_8004AD80` at `0x8004b708`/`0x8004bc84`, handed to `FUN_801DEA50`
/// as `node[+0x4C]` by `FUN_80047430`). This asserts the *disc* half of
/// that chain: the whole art bank the runtime walks is a verbatim image of
/// the player file's `record[0]`, so a same-size edit inside a bank record
/// is an edit the runtime sees.
///
/// Cross-checked against a live mid-battle RAM capture (mednafen scenario
/// `party_battle_gobu_gobu`): every one of Vahn's 33 bank rows in RAM
/// matches this decode byte for byte, effect scripts included.
#[test]
fn the_signature_effect_script_lives_in_the_bank_the_runtime_walks() {
    use legaia_actor_formats::battle_char_assembly as bca;
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let (patched, _) = run_mapped(&original, MoveMode::Hybrid, PartyMapping::default());
    // Glyphs 1 L, 2 R, 3 D, 4 U - the combo the reskin writes.
    const SIGNATURE_COMBO: [u8; 5] = [1, 2, 4, 4, 3];
    for (slot, ch) in SLOTS {
        let entry = patched
            .read_entry(legaia_arts_patch::arts::player_entry_index(ch))
            .expect("player file");
        let rec0 = bca::decode_record0(&entry).expect("record0");
        let bank = bca::art_animation_bank(&rec0).expect("art bank");
        let row = bank
            .iter()
            .find(|r| !r.uses_base_archive() && r.combo == SIGNATURE_COMBO)
            .expect("the signature row");
        // The bank record is addressed off record[0] by the same
        // expression the runtime uses, so read the script straight out of
        // the image and require the parser's view to agree with it.
        let at = row.entry_offset + 0x14;
        let live: Vec<u8> = rec0[at..at + 8 * 8].to_vec();
        assert_eq!(
            &live[..],
            &row.effect_script[0x14..0x14 + 8 * 8],
            "slot {slot}: the parsed script is not the bytes at record0 +{at:#x}"
        );
        // Exactly one record spawns, and it is not a retail id: the host
        // scripts are 8 x 0x96 (Vahn), 2 x 0x93 (Noa), 4 x 0x93 (Gala).
        let spawns: Vec<u8> = (0..8)
            .map(|i| live[i * 8 + 1])
            .filter(|&id| id != 0 && id & 0x7F != 0x7F)
            .collect();
        assert_eq!(
            spawns.len(),
            1,
            "slot {slot}: {} spawning record(s), expected the sibling's single burst",
            spawns.len()
        );
        assert!(
            !matches!(spawns[0], 0x96 | 0x93),
            "slot {slot}: the burst is still a host-element id {:#04X}",
            spawns[0]
        );
        // And the retail row it replaced really did carry those ids, so
        // the check above cannot go vacuous on a future disc.
        let retail_entry = retail
            .read_entry(legaia_arts_patch::arts::player_entry_index(ch))
            .expect("retail player file");
        let retail_rec0 = bca::decode_record0(&retail_entry).expect("retail record0");
        let retail_bank = bca::art_animation_bank(&retail_rec0).expect("retail art bank");
        let retail_row = retail_bank
            .iter()
            .find(|r| r.entry_offset == row.entry_offset)
            .expect("the retail row at the same offset");
        let retail_spawns: Vec<u8> = (0..8)
            .map(|i| retail_row.effect_script[0x14 + i * 8 + 1])
            .filter(|&id| id != 0 && id & 0x7F != 0x7F)
            .collect();
        assert!(
            retail_spawns.iter().all(|&id| matches!(id, 0x96 | 0x93)),
            "slot {slot}: retail host script is {retail_spawns:02X?}, not the \
             hand-authored flame/spark this test contrasts against"
        );
    }
}
