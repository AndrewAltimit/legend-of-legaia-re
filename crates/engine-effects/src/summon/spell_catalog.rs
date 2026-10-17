//! Which spell ids summon what: the id ranges, stager PROT entries, creature ids, the big-summon table and display names.

/// Per-frame opcode budget for one part's move-VM tick (defensive cap; retail
/// has no explicit limit but breaks on WAIT / HALT / end-of-buffer).
pub const SUMMON_PART_BUDGET: usize = 256;

/// Player Seru-magic spell-id range that resolves to a per-summon overlay at
/// the battle-action cast band (`FUN_801E295C` state `0x29`: `actor[+0x1DF] >=
/// 0x81`). Gimard *Burning Attack* = `0x81`; the SCUS spell-name table
/// (`DAT_800754D0 + id*12`) calls that id plainly `Gimard`, and `Tail Fire` is
/// spell `0x27` - the enemy Gimard's move, a different id on a different,
/// non-stager path (this tree's long-standing nickname for it is *Fire Tail*,
/// disclosed in `docs/subsystems/battle-action.md`). The `0x82..=0x8B` legs each carry a committed
/// regression oracle (mid-cast loader-B id + slot-B-resident stager;
/// disc+library-gated `summon_binding_base_high`); `0x81` is PCSX-side.
pub const SERU_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x81..=0x8B;

/// Evolved-Seru player cast block (`0x8C..=0x95`), the contiguous continuation
/// of [`SERU_SUMMON_IDS`] under the *same* `(id - 0x81) + 903` arithmetic
/// (`0x8C → 914 .. 0x95 → 923`, `summon_overlay::EVOLVED_SUMMON_STAGER_PROT`).
/// Every entry is statically confirmed stager-shaped, and the arithmetic is
/// capture-pinned on **both** sides of the gap (`0x8B → 913`, `0x99 → 927`).
/// Every leg is now *individually* capture-pinned too (loader-B id read
/// mid-cast + the stager byte-resident at slot B; disc+library-gated
/// `evolved_summon_binding`): the `<seru>_summon_mid_cast` mednafen states
/// cover `0x8C..=0x8F` / `0x92..=0x95`, and the two injected-cast PCSX states
/// `evolved_0x90_midcast` / `evolved_0x91_midcast` close Kemaro `0x90 → 918`
/// and Spoon `0x91 → 919`. Two of these stagers carry `0x4000` render-mode
/// nodes (`0x8E → 916`, `0x93 → 921`).
pub const EVOLVED_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x8C..=0x95;

/// Whether `spell_id` is a player Seru cast whose damage rolls through a
/// `battle_data` summon body: the base block [`SERU_SUMMON_IDS`] or the
/// evolved block [`EVOLVED_SUMMON_IDS`]. Both resolve their creature through
/// [`summon_creature_id`] and hand the shared kernel's summon branch
/// (`FUN_801DD0AC`, attacker slot `7`) the same summon-body stats.
pub fn has_summon_body(spell_id: u8) -> bool {
    SERU_SUMMON_IDS.contains(&spell_id) || EVOLVED_SUMMON_IDS.contains(&spell_id)
}

/// Whether the cast module for a player Seru `spell_id` calls the Seru
/// side-effect stager `FUN_801F3D3C` - the routine that stages the debuff
/// percent, or a light-row cure tier, into `0x801F6960`.
///
/// Retail calls it from inside the module, not from the cast band: the
/// `jal 0x801f3d3c` word `0x0C07CF4F` sits in every image `0903..=0923`
/// except **0907** (Nighto, `0x85`) and **0916** (Aluru, `0x8E`), and in no
/// image above (a byte scan of the entries at their slot-B base; Spoon's is
/// `0x801F7028`). So the whole base and evolved blocks stage, minus those two -
/// exactly the ids `FUN_801F3C34`'s "No effect." pass early-outs on
/// (`0x85` / `0x8E` / `>= 0x96`).
///
/// REF: FUN_801F3D3C (its callers, by image)
pub fn module_stages_side_effect(spell_id: u8) -> bool {
    has_summon_body(spell_id) && spell_id != 0x85 && spell_id != 0x8E
}

/// Rare-Seru **flute** summon block (`0x96..=0x98`), the contiguous
/// continuation of [`EVOLVED_SUMMON_IDS`] under the same arithmetic
/// (`0x96 → 924`, `0x97 → 925`, `0x98 → 926`). SummonFlute items (item-effect
/// classes 126/127) enqueue the spell id directly, so the flutes ride the same
/// stager mechanism as Seru magic rather than a separate path.
///
/// Capture-pinned on both live legs (`flute_lippian_midcast` /
/// `flute_spikefish_midcast`, probe `autorun_flute_cast.lua`): loader-B
/// `0x1D`/`0x1E` mid-cast with the slot-B head byte-identical to the disc
/// entry. **Lippian** `0x96` heads with the ASCII title "Ultimate Rave" (the
/// failed-kill banner; the landed 1/128 kill shows "Ultimate Death"), and
/// **Spikefish** `0x97` heads with a pre-linked slot-B pointer table. `0x98`
/// is unused in retail: entry 0926's own content is a single sector opening on
/// `jr ra`, and everything from file `+0x800` is 0927 bleeding through the
/// extraction over-read (byte-identical for 0x8800 bytes).
pub const FLUTE_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x96..=0x98;

/// High summon block: Evil Seru Magic (`0x99` - the creature resolves
/// per-cast, e.g. Juggernaut), the Sim-Seru summons Palma / Mule / Horn /
/// Jedo (`0x9A..=0x9D`), and the Ra-Seru summons Meta / Terra / Ozma
/// (`0x9E..=0xA0`). All eight legs each carry a committed regression oracle
/// (mid-cast loader-B id + slot-B-resident stager; disc+library-gated
/// `summon_binding_base_high`).
pub const HIGH_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x99..=0xA0;

/// Whether a party member's cast of `spell_id` trains the spell (credits the
/// record's `+0x8` XP word and runs the `FUN_801E70BC` level check).
///
/// The finisher's XP tail (`FUN_801DDB30`, `0x801DE440`) keys on attacker
/// slot 7 with no spell-id test, and every module of the base, evolved and
/// high blocks strikes through `FUN_801DD0AC(0x12, 7, seat)` - the high block
/// included (Juggernaut `0x801F7E0C`, Palma `0x801F8114`, Mule `0x801F7E4C`,
/// Jedo's scripted arm `0x801F8438`, Meta `0x801F7BA0`, Terra `0x801F7CCC`,
/// Ozma `0x801F8E04`, each with `li a1,0x7`). The two that strike nothing
/// credit inline instead: Horn `0x9C` per party seat and Jedo `0x9D` per
/// living monster outside a scripted fight
/// ([`crate::magic_xp::horn_seat_xp_gain`],
/// [`crate::magic_xp::JEDO_XP_PER_LIVING_MONSTER`]). `FUN_801E70BC`'s own
/// tripled-threshold set names `0x99` / `0x9B` / `0xA0`. The flute ids
/// `0x96..=0x98` are item casts the caster's spell list never carries, so
/// they are left out.
///
/// REF: FUN_801DDB30 (`0x801DE440` slot-7 gate), FUN_801E70BC
pub fn module_trains_spell_xp(spell_id: u8) -> bool {
    has_summon_body(spell_id) || HIGH_SUMMON_IDS.contains(&spell_id)
}

/// The whole player-summon spell-id span, base through high block. Every id in
/// it resolves through the same linear stager arithmetic - see
/// [`summon_stager_prot_entry`].
pub const PLAYER_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x81..=0xA0;

/// PROT entry holding the per-summon stager overlay for a Seru-magic `spell_id`,
/// or `None` if `spell_id` is not a summon. Retail: `FUN_8003EC70(id - 0x79)`
/// loads raw-TOC index `(id - 0x79) + 0x381` - the in-RAM TOC at `0x801C70F0`
/// is raw `PROT.DAT` from byte 0 (header included), so the extraction entry
/// sits 2 below the raw index: `0x81..=0x8B → 903..=913` and
/// `0x99..=0xA0 → 927..=934` (see `docs/formats/prot.md` § index spaces).
///
/// Capture-pinned for **every id in the base and high blocks**: one mid-cast
/// save state per spell holds the battle overlay's loader-B current-id
/// (`0x8007BC4C`) at exactly `spell_id - 0x79` with the predicted entry
/// byte-resident at the slot-B link base (the `<seru>_summon_mid_cast` +
/// `gimard_summon_*` scenarios in `scripts/scenarios.toml`). Titled entries
/// head with the ASCII display name of the summon's attack (0907 Nighto
/// "Hell's Music", 0927 Juggernaut "Dark Eclipse"); the high-block entries
/// otherwise head with a pre-linked slot-B pointer table. The
/// [`EVOLVED_SUMMON_IDS`] block bridging the two is capture-pinned on every
/// leg as well, and so is the [`FLUTE_SUMMON_IDS`] block that used to read as a
/// gap: `0x96`/`0x97` are the rare-Seru flute summons Lippian / Spikefish
/// (capture-pinned mid-cast on the *player* path), and `0x98 → 926` is retail's
/// unused slot. So the whole span `0x81..=0xA0` is **one unbroken linear run**
/// - `extraction = spell_id - 0x79 + 895` - with no hole to special-case.
pub fn summon_stager_prot_entry(spell_id: u8) -> Option<u32> {
    if !PLAYER_SUMMON_IDS.contains(&spell_id) {
        return None;
    }
    // One contiguous run: 0x81 → 903 .. 0xA0 → 934. The loader arithmetic is
    // `FUN_8003EC70(id - 0x79)` against the raw TOC, and the extraction index
    // sits 2 below the raw one, so `903 + (id - 0x81)` states the same thing.
    // REF: FUN_8003EC70 (the slot-B overlay loader this mirrors)
    Some(903 + (spell_id - 0x81) as u32)
}

/// `battle_data` (PROT 867) creature id whose mesh + per-object animation the
/// player Seru-magic summon `spell_id` reuses, or `None` if not a summon / not
/// found. **The player summon spawns the namesake creature** (the Gimard spell
/// summons the Gimard creature, Theeder→Theeder, …), so the faithful render is
/// the ordinary battle TRS-keyframe path applied to that creature's
/// monster-archive block (mesh via `legaia_actor_formats::monster_archive::battle_render_mesh`,
/// animation via [`legaia_actor_formats::monster_archive::idle_animation`] →
/// [`crate::battle_anim::MonsterAnimPlayer`] → `tmd_to_vram_mesh_posed_rot`) -
/// **not** the move-VM scene-graph the [`SummonScene`] stand-in drives.
///
/// Resolved from the disc-pinned base + evolved map
/// ([`legaia_actor_formats::summon_creatures`]): every summon group's `summon.dat`
/// actor-record TMD is byte-identical to its mapped `battle_data` creature mesh,
/// covering `0x81..=0x95` (the base block plus the evolved-Seru block, including
/// the two evolved legs Kemaro `0x90` / Spoon `0x91`, which were disc-pinned by
/// mesh identity first and have since gained injected-cast capture states too).
/// The high block `0x99..=0xA0` carries a bespoke mesh (not an archive creature)
/// and is intentionally unmapped - those summons return `None` here for now.
/// The map id is validated as live in this archive; when it is not, the map
/// entry's creature name is looked up among the archive's records instead.
pub fn summon_creature_id(spell_id: u8, battle_data_entry: &[u8]) -> Option<u16> {
    // Disc-pinned map: the creature id is exact (byte-validated mesh identity),
    // so prefer it. Validate it resolves in this archive before returning.
    let mapped = legaia_actor_formats::summon_creatures::creature_for_spell(spell_id).filter(|c| {
        legaia_actor_formats::monster_archive::record(battle_data_entry, c.creature_id)
            .ok()
            .flatten()
            .is_some()
    });
    if let Some(c) = mapped {
        return Some(c.creature_id);
    }
    // Fallback: the mapped creature's name, resolved among this archive's
    // records. The battle spell mirror's summon names (`0x81..=0x8B`) are the
    // map's own names for those ids, so the map is the one name source.
    let name = legaia_actor_formats::summon_creatures::creature_for_spell(spell_id)?.name;
    let slots = legaia_actor_formats::monster_archive::slot_count(battle_data_entry) as u16;
    (1..=slots).find(|&id| {
        legaia_actor_formats::monster_archive::record(battle_data_entry, id)
            .ok()
            .flatten()
            .is_some_and(|r| r.name == name)
    })
}

/// What a host seats as the summoned creature: the mesh (TMD + texture pool,
/// monster-shaped so `MonsterMesh::battle_render_mesh` relocates it), the
/// clip the seat idles on, and the record-order clip set the stager's staged
/// ids resolve against.
#[derive(Debug, Clone)]
pub struct SummonSpawnAsset {
    /// The archive creature id for a reused enemy body, `None` for a body
    /// that comes out of the cast's own `summon.dat` group.
    pub creature_id: Option<u16>,
    pub mesh: legaia_actor_formats::monster_archive::MonsterMesh,
    pub idle: Option<legaia_actor_formats::monster_archive::MonsterAnimation>,
    pub clips: Vec<legaia_actor_formats::monster_archive::MonsterAnimation>,
}

/// Resolve the body a player summon `spell_id` seats as slot 7.
///
/// Retail's seat arm `FUN_801F19EC` installs the cast's **own** streamed
/// `summon.dat` actor record (TMD + texture pool, through the monster mesh
/// installer `FUN_80055468`) for every id. For the base and evolved blocks
/// (`0x81..=0x95`) that record's TMD and texture pool are byte-identical to
/// the mapped `battle_data` creature's (the disc-gated
/// `summon_creature_tmd_map_real` oracle pins both), so the archive record is
/// the same body - but not the same clips: those come out of the cast's own
/// record whenever `summon_dat` is given. Every other cast id
/// (the high block `0x99..=0xA0` and the flutes) has no archive twin and
/// resolves from `summon_dat` (extraction PROT 893) through
/// [`legaia_actor_formats::summon_readef::parse_cast`], idling on the record's clip 0.
/// `None` when neither source resolves.
///
/// REF: FUN_801F19EC, FUN_80055468
pub fn summon_spawn_asset(
    spell_id: u8,
    battle_data_entry: &[u8],
    summon_dat: Option<&[u8]>,
) -> Option<SummonSpawnAsset> {
    use legaia_actor_formats::monster_archive as ma;
    if let Some(creature) = summon_creature_id(spell_id, battle_data_entry)
        && let Ok(Some(mesh)) = ma::mesh(battle_data_entry, creature)
    {
        let archive_clips = ma::animations(battle_data_entry, creature)
            .ok()
            .flatten()
            .unwrap_or_default();
        // The clips are the record's own, not the archive twin's: the
        // installer hands the streamed record's part table (`+0x4C`) to the
        // node (`sw v1,0x4c(v0)` at `0x801F1DCC`) and scans its tags
        // (`FUN_80054CB0(record, 4)` at `0x801F1D1C`). Only the TMD and
        // texture pool are byte-identical to the archive's; the clip set
        // differs in count, length, rate byte and root speed (Gimard's walk
        // carries `56` in its record against the archive's `28`). A record
        // whose clips do not pose this mesh's parts keeps the archive's.
        let part_count = archive_clips.first().map(|c| c.part_count);
        let own = summon_dat
            .and_then(|s| legaia_actor_formats::summon_readef::parse_cast(s, spell_id).ok())
            .map(|c| c.clips)
            .filter(|c| {
                !c.is_empty() && part_count.is_none_or(|n| c.iter().all(|a| a.part_count == n))
            });
        if let Some(clips) = own {
            return Some(SummonSpawnAsset {
                creature_id: Some(creature),
                mesh,
                idle: clips.first().cloned(),
                clips,
            });
        }
        return Some(SummonSpawnAsset {
            creature_id: Some(creature),
            mesh,
            idle: ma::idle_animation(battle_data_entry, creature)
                .ok()
                .flatten(),
            clips: archive_clips,
        });
    }
    let cast = legaia_actor_formats::summon_readef::parse_cast(summon_dat?, spell_id).ok()?;
    Some(SummonSpawnAsset {
        creature_id: None,
        idle: cast.clips.first().cloned(),
        mesh: cast.mesh,
        clips: cast.clips,
    })
}

/// One entry of the four-slot **big-summon** band `0x9A..=0xA0` - the Sim-Seru
/// summons (Palma / Mule / Horn / Jedo) and the three Ra-Seru summons
/// (Meta / Terra / Ozma). These are the summons whose creature body is
/// **bespoke**: unlike `0x81..=0x95`, their mesh byte-matches no `battle_data`
/// archive record, so [`summon_creature_id`] returns `None` for them and the
/// mesh has to come out of the cast's own `summon.dat` group
/// ([`legaia_actor_formats::summon_readef::parse_cast`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigSummon {
    /// Spell / action id (`actor[+0x1DF]`).
    pub spell_id: u8,
    /// The summon's own name - what the player selects in the magic menu.
    pub name: &'static str,
    /// The cast's attack name. This is the string carried **on the disc** at
    /// the group's actor-record `rec[0]` offset, which is what pins each
    /// name to its id (see [`BIG_SUMMONS`]).
    pub attack: &'static str,
    /// Element id the actor record's `+0x1D` byte carries for this cast
    /// (`0`=earth `1`=water `2`=fire `3`=wind `4`=thunder `5`=light `6`=dark
    /// `7`=none) - the value the damage pipeline attributes to the cast.
    pub element: u8,
}

/// The big-summon band, id-ascending.
///
/// Every row is pinned to its id by **two independent disc reads**, not by
/// ordering assumption:
///
/// 1. the `summon.dat` group's actor record carries the cast's attack name as
///    an inline ASCII string at `rec[0]`, and it equals [`BigSummon::attack`];
/// 2. the same record's `+0x1D` element byte equals [`BigSummon::element`].
///
/// Both are asserted against the real disc by the disc-gated
/// `summon_cast_real` / `summon_view_real` oracles. `0x99` (Evil Seru Magic,
/// "Dark Eclipse") shares the band's spell range but is a three-slot group
/// with a per-cast creature, so it is not a member here.
pub const BIG_SUMMONS: &[BigSummon] = &[
    BigSummon {
        spell_id: 0x9A,
        name: "Palma",
        attack: "Meteor Cluster",
        element: 0,
    },
    BigSummon {
        spell_id: 0x9B,
        name: "Mule",
        attack: "Deep Avalanche",
        element: 1,
    },
    BigSummon {
        spell_id: 0x9C,
        name: "Horn",
        attack: "Resurrector",
        element: 5,
    },
    BigSummon {
        spell_id: 0x9D,
        name: "Jedo",
        attack: "Deadly Promise",
        element: 6,
    },
    BigSummon {
        spell_id: 0x9E,
        name: "Meta",
        attack: "Inferno",
        element: 2,
    },
    BigSummon {
        spell_id: 0x9F,
        name: "Terra",
        attack: "Queen Twister",
        element: 3,
    },
    BigSummon {
        spell_id: 0xA0,
        name: "Ozma",
        attack: "Voltagor",
        element: 4,
    },
];

/// The [`BigSummon`] row for `spell_id`, or `None` outside `0x9A..=0xA0`.
pub fn big_summon(spell_id: u8) -> Option<&'static BigSummon> {
    BIG_SUMMONS.iter().find(|s| s.spell_id == spell_id)
}

/// The [`BigSummon`] row named `name` (case-insensitive), or `None`.
pub fn big_summon_by_name(name: &str) -> Option<&'static BigSummon> {
    BIG_SUMMONS
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
}

/// The two live **rare-Seru flute** summons, whose ids sit between the evolved
/// block and the high block and so are covered by neither
/// [`legaia_actor_formats::summon_creatures`] nor [`BIG_SUMMONS`]. Both are
/// capture-pinned mid-cast on the player path (see [`FLUTE_SUMMON_IDS`]);
/// `0x98` is retail's unused third slot and stays unnamed.
const FLUTE_SUMMON_NAMES: &[(u8, &str)] = &[(0x96, "Lippian"), (0x97, "Spikefish")];

/// Display name of the creature a player summon spell brings out, across the
/// whole `0x81..=0xA0` span: the namesake `battle_data` creature for the base
/// and evolved blocks ([`legaia_actor_formats::summon_creatures`]), the summon's own
/// name for the big-summon band ([`BIG_SUMMONS`]) and the two flute summons.
/// `None` for any other id - the unused `0x98` slot, and ids outside the
/// player-summon span. (The battle spell mirror names only `0x81..=0x8B`, which
/// the creature map already covers with the same names.)
pub fn summon_display_name(spell_id: u8) -> Option<&'static str> {
    if let Some(c) = legaia_actor_formats::summon_creatures::creature_for_spell(spell_id) {
        return Some(c.name);
    }
    if let Some(b) = big_summon(spell_id) {
        return Some(b.name);
    }
    if let Some((_, n)) = FLUTE_SUMMON_NAMES.iter().find(|(id, _)| *id == spell_id) {
        return Some(n);
    }
    None
}
