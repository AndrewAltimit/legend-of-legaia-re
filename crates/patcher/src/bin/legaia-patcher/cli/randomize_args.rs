//! The `randomize` flag set and its value enums.

use crate::flag_values::{
    parse_arts_ap_cost, parse_arts_ap_grant, parse_arts_power, parse_attack_count_scale,
    parse_delilas_party, parse_exp_scale, parse_item_spec, parse_location_rename,
    parse_prize_price, parse_seru_catch_rate, parse_stat_scale, parse_super_art_power,
};
use clap::{Parser, ValueEnum};
use legaia_patcher::apply;
use legaia_patcher::drops::DropMode;
use std::path::PathBuf;

#[derive(Parser)]
pub(crate) struct RandomizeArgs {
    /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
    /// is accepted and resolved to the `.bin` it references).
    #[arg(long)]
    pub(crate) input: PathBuf,
    /// Seed for reproducibility. Either a number (decimal or `0x`-hex) or any
    /// string (hashed to a number). The resolved numeric seed is always
    /// printed so a run can be reproduced exactly. If omitted, one is drawn
    /// from the system clock.
    #[arg(long)]
    pub(crate) seed: Option<String>,
    /// How monster item drops are reassigned. Off by default, like every
    /// other randomizer feature: a run that only asks for a manual edit or a
    /// community mod must not silently reshuffle the drop tables (it once
    /// did - a pack-only PPF came out at 9 MB).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) drops: DropArg,
    /// Inject an *additional* low-chance equipment drop into the battle-end
    /// reward routine (a same-size `SCUS_942.54` code hook). On a per-battle
    /// roll it grants one extra random weapon / armor / accessory **on top of**
    /// the normal drop - the regular drop table (vanilla or `--drops`) is never
    /// disturbed.
    #[arg(long, default_value_t = false)]
    pub(crate) equipment_drops: bool,
    /// Per-battle chance (percent) for the `--equipment-drops` bonus drop.
    #[arg(long, default_value_t = legaia_code_hooks::bonus_drop::DEFAULT_CHANCE_PCT)]
    pub(crate) equipment_drop_chance: u8,
    /// Bank a slice of the formation's experience into the party whenever they
    /// **successfully run away** (a same-size code hook into the battle-action
    /// escape teardown). Vanilla awards nothing for fleeing; this credits
    /// `--flee-exp-pct`% of the fled fight's EXP to every party member.
    #[arg(long, default_value_t = false)]
    pub(crate) flee_exp: bool,
    /// Percentage of the formation's experience banked on a successful escape
    /// (only with `--flee-exp`).
    #[arg(long, default_value_t = legaia_code_hooks::flee_exp::DEFAULT_PCT)]
    pub(crate) flee_exp_pct: u8,
    /// With `--enemy-ally-pct`% probability per battle, **charm a random enemy**
    /// onto the party's side as an uncontrolled ally (a same-size code hook into
    /// battle setup that sets the AI-delegated bits on the frontmost enemy, plus a
    /// one-word widen of the victory check so the ally isn't an enemy you must
    /// defeat). Fires only in **multi-enemy** fights - single-enemy fights (every
    /// input-gated tutorial and solo boss) are skipped, since charming the lone
    /// enemy of a scripted fight softlocks it.
    #[arg(long, default_value_t = false)]
    pub(crate) enemy_ally: bool,
    /// Per-battle percentage chance an enemy is charmed (only with `--enemy-ally`).
    #[arg(long, default_value_t = legaia_patcher::enemy_ally::DEFAULT_PCT)]
    pub(crate) enemy_ally_pct: u8,
    /// Draw a **red HP gauge over every enemy** during battle: the roster
    /// panel's `HP` label and retail's AP-meter primitive recoloured red with
    /// the remaining percentage (no frame), fed by the displayed-HP mirror so
    /// it steps per hit, seated over each living monster's screen X every
    /// frame. A two-word detour at the head
    /// of the damage-number popup renderer plus the routine laid over four
    /// routines retail never references - no injected-code arena bytes, so it
    /// composes with every other hook.
    #[arg(long, default_value_t = false)]
    pub(crate) enemy_hp_bar: bool,
    /// With `--shiny-pct`% probability per battle, the frontmost **capturable**
    /// enemy spawns as a rare **shiny** variant: +35% stats (translucent), and the
    /// Seru you capture from it deals +35% damage forever, with a translucent
    /// summon + a "+35% DMG!" cast caption (a same-size code hook into battle
    /// setup + the capture/damage/draw paths; the persistent shiny flag is a
    /// parallel per-spell byte at `record+0x1C0`, and every injected routine lives
    /// in verified-dead SCUS space outside all live tables).
    #[arg(long, default_value_t = false)]
    pub(crate) shiny_seru: bool,
    /// **Delilas Challenge**: a fourth option on the Muscle Dome enrollment
    /// menu - a brand-new 2-round arena course: Che & Lu Delilas together
    /// (1v2), then Gi (1v1). The double-team fits the battle heap by
    /// streaming slim clones (their generic castable spells trimmed) from
    /// two unreachable archive slots - the real 163/164 records are never
    /// modified, and the fight keeps the signature specials, the
    /// attack-attack-special AI, and the real names. A full clear pays 5000
    /// coins. Unlocks after the Koru event in Nivora Ravine (the `nilboa2`
    /// story flag). Losing a round returns to the Sol venue by the dome's
    /// own design - no game over. A `koin1` script edit plus a small
    /// arena/SCUS code injection. Mutually exclusive with `--delilas-party`.
    #[arg(long, default_value_t = false)]
    pub(crate) delilas_challenge: bool,
    /// **Custom items**: inject three brand-new items into cut item slots -
    /// Nature's Elixir (full HP+MP restore), the Ra-Seru Tear (a free cast
    /// of the user's own Ra-Seru summon), and the Fury Bloom (party-wide
    /// Fury Boost). Standalone: with a `random` drop / chest / steal mode
    /// they join the fill pool, and with `--delilas-challenge` they replace
    /// the Honey as the course's full-clear reward.
    #[arg(long, default_value_t = false)]
    pub(crate) custom_items: bool,
    /// **Delilas party swap**: play as the Delilas siblings while the story's
    /// ravine duels (and the Muscle Dome Master legs) field Vahn / Noa / Gala
    /// instead. A battle-model + name identity swap: each character keeps
    /// their own animations, arts, stats and story, but wears the mapped
    /// sibling's model; each sibling's monster block wears the mapped
    /// character's battle model under the retail Delilas movesets, renamed to
    /// match. The value maps the three siblings onto Vahn, Noa, Gala in
    /// order: any permutation of `gi`, `lu`, `che` (e.g. `--delilas-party
    /// lu,gi,che` puts Lu on Vahn). New-game party names follow the mapping
    /// (existing saves keep their stored names). The field walking models
    /// rebuild from the same sibling models, and the party's battle grunt
    /// voices resample from the siblings' own voice banks; what the
    /// Tactical Arts shout banks carry is picked by
    /// `--delilas-arts-voice`. Mutually exclusive with `--delilas-challenge`.
    #[arg(long, value_name = "V,N,G", value_parser = parse_delilas_party)]
    pub(crate) delilas_party: Option<legaia_delilas_party::delilas_party::PartyMapping>,
    /// With `--delilas-party`: keep Che's welded giant hammer on his
    /// swapped mesh instead of replacing it with a mirrored fist and
    /// fusing the host's own weapon into the hand. Comparison build:
    /// clips that assume a hand-sized part swing the hammer wide, and
    /// Che fights with the hammer regardless of his equipped weapon.
    #[arg(long)]
    pub(crate) delilas_che_hammer: bool,
    /// With `--delilas-party`: what the Tactical Arts shout banks
    /// (XA2/XA4/XA6) carry. `original` (default) keeps the retail
    /// Vahn / Noa / Gala shouts; `adjusted` re-voices them toward each
    /// mapped sibling with the tuned pitch/formant map; `removed`
    /// silences arts shouts (attack grunts remain).
    #[arg(long, value_name = "MODE", default_value = "original")]
    pub(crate) delilas_arts_voice: legaia_delilas_party::delilas_voice_fx::ArtsVoiceMode,
    /// With `--delilas-party`: how much of the hero's Tactical Arts kit
    /// becomes the sibling's. `hybrid` (default) keeps every host art's
    /// own animation and only reskins one Hyper into the sibling's
    /// signature special; `delilas` rebuilds the whole art-stream
    /// archive from the sibling's own motions, renames each surviving
    /// art after the clip it plays, and blanks every art the Super and
    /// Miracle Arts do not need out of the matcher.
    #[arg(long, value_name = "MODE", default_value = "hybrid")]
    pub(crate) delilas_moves: legaia_delilas_party::delilas_party::DelilasMoveMode,
    /// **Show Super Arts on the in-battle move list**: list a character's
    /// Super Arts on the Tactical-Arts list the Triangle button opens in
    /// battle, which retail never draws at all. A Super Art appears once you
    /// have **performed** it (retail records nothing about a Super Art ever
    /// firing, so the patch keeps a per-character flag byte in a spare slot of
    /// the character record - it saves with the rest, an older save starts
    /// with none), sits among the regular arts **in AP order**, and its row
    /// shows the Super Art's name, the chain's summed AP cost (a Super Art
    /// charges nothing of its own) and the arrows you actually type - the
    /// seven-to-nine inputs the game's own arts recogniser resolves to the
    /// trigger (Tri-Somersault: up down up up up down up), blue with each
    /// sub-art's last arrow yellow and the Super's final arrow orange; the pause
    /// menu's Status screen (Left = Moves) lists them the same way, with a
    /// description naming the chain. Two same-size
    /// detours into the SCUS list renderer plus their routines and tables in
    /// verified-dead SCUS regions, an in-place replacement of the list pager
    /// in PROT 0898 whose tail records a performed Super Art, and a detour
    /// from the Super applier into it. The Triangle caption's own page
    /// thresholds stay retail, so on a later page it can still read "View
    /// Hyper Arts list". **Mutually exclusive with `--shiny-seru`,
    /// `--arts-ap-grant` / `--arts-ap-cost` and `--delilas-challenge`** (same
    /// dead-space bytes).
    #[arg(long, default_value_t = false)]
    pub(crate) show_super_arts: bool,
    /// **Super Arts Pack, by ZetaPhoenix**: fifteen extra Super Arts - five
    /// per character on top of the retail five - each with its own name (Ultra
    /// Elbow, Somersault Duo, Double Lizard, Ground Pound, ...), hit count and
    /// animation, triggered by their own arts chains. ZetaPhoenix's 3764-byte
    /// block is installed unmodified: it is parked in the `DMY.DAT` annex and
    /// streamed to `0x801FD000` at battle load by an injected stub, and ten
    /// same-size word edits point the retail Super-Art applier, the arts queue
    /// builder and the two banner routines at it. The retail five per character
    /// keep their triggers and results - the pack carries them verbatim as its
    /// first five rows - though its animation hook also runs for the seventeen
    /// art constants it lists, three of which retail Super Arts share.
    /// **Mutually exclusive with `--shiny-seru`, `--show-super-arts`,
    /// `--arts-ap-grant` / `--arts-ap-cost` and `--delilas-challenge`** (same
    /// dead-space bytes).
    #[arg(long, default_value_t = false)]
    pub(crate) super_arts_pack: bool,
    /// Per-battle percentage chance a capturable enemy is shiny (only with
    /// `--shiny-seru`).
    #[arg(long, default_value_t = legaia_code_hooks::shiny_seru::DEFAULT_PCT)]
    pub(crate) shiny_pct: u8,
    /// **Jewel fix**: make the boss cinematic casts respect elemental guards.
    /// In retail, Xain's Bloody Horns / Terio Punch (+ Bull Charge), Cort's
    /// Guilty Cross, and the Delilas trio's Blazing Slash /
    /// Megaton Press / Plasma Strike call the resist-ladder-bypassing damage
    /// wrapper (`FUN_801DD6B4`, finisher `param_5 = 1`), so Earth/other
    /// Jewels, elemental guards and All Guard never apply to them despite the
    /// caster's element being read by the affinity scale. This retargets the
    /// thirteen damage `jal`s in the six streamed cast modules to the
    /// guard-respecting wrapper `FUN_801DD4B0` (same-size word edits in PROT
    /// entries 944 / 952 / 953 / 958 / 959 / 960); casts that already respect
    /// guards (incl. Neo Star Slash, which shares Plasma Strike's module) are
    /// untouched.
    #[arg(long, default_value_t = false)]
    pub(crate) jewel_fix: bool,
    /// Fix the **attack-approach softlock** (the "endless camera orbit"): a
    /// monster approaching an out-of-reach target waits in a range poll with
    /// no timeout while its approach animation drives the movement - and
    /// when that animation dies mid-approach (a summon immediately before
    /// the melee; caught live and reproduced on the Gaza rematch) nothing
    /// re-stages it and the battle waits forever. A nine-word rewrite of the
    /// poll's redundant facing recompute re-stages the dead animation
    /// through the game's own staging state, so the monster simply resumes
    /// walking. Healthy approaches, party attacks, and in-range attacks
    /// behave byte-for-byte like retail.
    #[arg(long, default_value_t = false)]
    pub(crate) approach_softlock_fix: bool,
    /// Fix **no random encounters in Jette's Absolute Fortress**. The escape
    /// clock in Zora's Floating Castle switches battles off for its last
    /// minute by raising a story flag that nothing ever lowers, and both
    /// Fortress scenes open their encounter tables with a copy of the
    /// castle's "this flag means no battles" group - so a save that left the
    /// castle with under a minute showing finds the whole Fortress silent,
    /// with the Door of Light greyed out. This rewrites that leading group in
    /// the `deroa` and `chitei2` scene MANs (14 bytes each, same size) into a
    /// duplicate of the scene's own post-Jette group, so the Fortress never
    /// reads the stale flag. It heals a save that is already affected; a
    /// healthy save and the castle itself play exactly as retail.
    #[arg(long, default_value_t = false)]
    pub(crate) fortress_encounter_fix: bool,
    /// Fix the **Conkram mirror freeze**: on an emulator that runs the CPU
    /// faster than a console (PCSX-ReARMed's default for this game, or a
    /// SwanStation / DuckStation overclock), the Room of Mirrors scene in
    /// present-day Conkram runs the heap dry, and the effect code copies an
    /// animation through the failed allocation's NULL pointer over the CPU's
    /// exception vector - the game locks up. This makes both of the effect
    /// parts' allocations safe (four same-size edits in `SCUS_942.54`): a
    /// failed animation block drops the part, a failed render node gets a
    /// shared stand-in, nothing is written through NULL, and the failure no
    /// longer leaves the `PRG ERR` readout on screen. Every allocation that
    /// succeeds runs exactly as retail, so play at console speed is
    /// unchanged.
    #[arg(long, default_value_t = false)]
    pub(crate) conkram_freeze_fix: bool,
    /// Set how much **AP the Spirit command charges** into the battle AP
    /// gauge (retail 32). `0` makes Spirit a pure defensive stance (guard
    /// boost, no AP); `100` fills the whole gauge in one press; a **negative**
    /// value makes Spirit *drain* the gauge instead, floored at zero.
    /// Rewrites the battle engine's per-action AP accrual for the Spirit
    /// category plus the three gauge-widget ramp targets that mirror it -
    /// four same-size word edits in the battle overlay (PROT 898). A negative
    /// value additionally makes the accrual read signed and neutralizes the
    /// AP-Boost accessory arms (they read the accrual unsigned), so an
    /// AP-Boost accessory is inert while the setting is negative.
    #[arg(long, value_name = "AP", allow_negative_numbers = true,
          value_parser = clap::value_parser!(i16).range(-100..=100))]
    pub(crate) spirit_ap: Option<i16>,
    /// Set how much **AP taking damage grants**, as AP per 100% of max HP
    /// lost (retail 100: a hit that takes your whole HP bar fills the
    /// 100-point gauge). `0` means damage never feeds the gauge; `200`
    /// doubles the fill rate; a **negative** value makes being hit *drain*
    /// the gauge instead, floored at zero. Rewrites the battle damage
    /// finisher's scale in the battle overlay (PROT 898); as with
    /// `--spirit-ap`, a negative value neutralizes the AP-Boost accessory
    /// arms.
    #[arg(long, value_name = "AP", allow_negative_numbers = true,
          value_parser = clap::value_parser!(i16).range(-200..=200))]
    pub(crate) damage_ap: Option<i16>,
    /// Set the **fishing-exchange price** of one or more prizes. Comma- or
    /// repeat-separated `ITEM=POINTS` entries (`--fishing-price 0x6F=500` sets
    /// the Water Egg to 500 fishing points; ids in decimal or `0xHH`). The
    /// price is both the point cost and the "only appears once you can afford
    /// it" gate, so lowering it also makes the prize show up sooner. Applies to
    /// every venue (Buma / Vidna) row granting that item. `legaia-patcher
    /// fishing` lists the current prizes and prices.
    #[arg(long, value_name = "ITEM=POINTS", value_delimiter = ',', value_parser = parse_prize_price)]
    pub(crate) fishing_price: Vec<(u8, u32)>,
    /// Set the **Earth Egg coin threshold** - the casino-coin count the Sol
    /// Tower "Prize Counter" requires before it offers to exchange coins for the
    /// Earth Ra-Seru Egg (retail 100000). This is a bespoke scripted exchange,
    /// *not* a row in the casino prize table, so the shop / casino editors can't
    /// reach it. VALUE is the coins required; the game debits exactly that many
    /// on purchase (gate = value - 1, debit = value, matching retail). Range
    /// 1..=8388608. `legaia-patcher earth-egg` shows the current value.
    #[arg(long, value_name = "VALUE")]
    pub(crate) earth_egg_price: Option<u32>,
    /// **Rebalance a Tactical Art's damage** ("arts power-down"). Comma- or
    /// repeat-separated `COMBO=VALUE` entries, targeting an art by its input
    /// combo (`L/R/D/U`, e.g. `--arts-power RDLDL=0x16`). `VALUE` is a
    /// power-encoding byte: `0x0C..=0x1F` (a defence facet + one of the
    /// multipliers 12/18/20/22/28; higher = stronger, so a *lower* value powers
    /// the art down), or `0` to disable that art's hits. Every active per-strike
    /// power byte of the matched art is set to `VALUE` (hit count preserved).
    /// `legaia-patcher arts` lists every art's combo and current power tiers.
    #[arg(long, value_name = "COMBO=VALUE", value_delimiter = ',', value_parser = parse_arts_power)]
    pub(crate) arts_power: Vec<(Vec<legaia_art::queue::Command>, u8)>,
    /// **Rebalance a Super Art's damage.** Comma- or repeat-separated
    /// `NAME=VALUE` entries, targeting one of the fifteen Super Arts by name
    /// (case / spacing / punctuation insensitive, e.g.
    /// `--super-art-power "Tri-Somersault"=0x1A`). `VALUE` is the same
    /// power-encoding byte `--arts-power` takes (`0x0C..=0x1F`, or `0` to
    /// disable the hits), applied to every active per-strike byte of the Super
    /// Art's own `record0` art record (hit count preserved). A Super Art has no
    /// input combo and no arts-name-table row - and costs no AP of its own, the
    /// chain arts pay - so power is the knob it has; `--arts-power` and
    /// `--arts-ap-grant` structurally cannot reach it.
    /// `legaia-patcher arts` lists each Super Art with its trigger chain.
    #[arg(long, value_name = "NAME=VALUE", value_delimiter = ',', value_parser = parse_super_art_power)]
    pub(crate) super_art_power: Vec<(&'static legaia_art::SuperArt, u8)>,
    /// **Make a Tactical Art grant AP instead of costing it** ("arts AP-grant").
    /// Comma- or repeat-separated `[CHARACTER:]COMBO=AMOUNT` entries, targeting
    /// an art by its input combo (`L/R/D/U`, e.g. `--arts-ap-grant RDLDL=10` or
    /// `--arts-ap-grant Vahn:RDLDL=10`). `AMOUNT` is the AP (Spirit) granted per
    /// use (1..=100); the art becomes castable at any AP level and *adds* that
    /// much (clamped at 100) rather than paying a cost. A same-size code hook
    /// into the party arts queue-builder (PROT 0898) plus routines + a
    /// per-(character, row) config table in verified-dead SCUS regions. Without
    /// a `CHARACTER:` prefix every character holding that combo is targeted -
    /// each in its own cell, so nothing spills onto another character's art.
    /// The art's menu AP number is rewritten to match (a grant shows `0`).
    /// **Mutually exclusive with `--shiny-seru`** (same arena bytes).
    #[arg(long, value_name = "COMBO=AMOUNT", value_delimiter = ',', value_parser = parse_arts_ap_grant)]
    pub(crate) arts_ap_grant: Vec<legaia_arts_patch::arts_ap_grant::ArtApSpec>,
    /// **Set what a Tactical Art costs in AP** ("arts AP-cost"). Same
    /// `[CHARACTER:]COMBO=AMOUNT` syntax as `--arts-ap-grant`; `AMOUNT` is the
    /// flat AP (Spirit) the art charges per use (1..=100), replacing retail's
    /// computed `multiplier x command_count`. Retail stores no per-art cost, so
    /// this rides the same code hook as `--arts-ap-grant`; both the
    /// affordability gate and the charged amount follow the setting, and the
    /// art's menu AP number is rewritten to match. `0` is not available - it is
    /// the config table's "leave at retail" value, so the cheapest art is 1 AP.
    /// **Mutually exclusive with `--shiny-seru`** (same arena bytes).
    #[arg(long, value_name = "COMBO=AMOUNT", value_delimiter = ',', value_parser = parse_arts_ap_cost)]
    pub(crate) arts_ap_cost: Vec<legaia_arts_patch::arts_ap_grant::ArtApSpec>,
    /// **Oscillating AP costs**: every battle, each Tactical Art is dealt at
    /// random onto the *cost* side (retail: pays its AP, full damage) or the
    /// *grant* side (castable at any AP level, *adds* the AP it would have
    /// cost, deals `DAMAGE_PCT` percent of its damage). Re-rolled per art per
    /// battle, so a fight is a mix of both sides and the next fight a
    /// different one. `DAMAGE_PCT` is `0..=100` (default 20 when the flag is
    /// given bare). Enemies are untouched. The in-battle Tactical-Arts list
    /// (Triangle) shows a grant-side art as `0` AP for that battle. Code hooks
    /// into the arts queue-builder, the strike-damage kernel (PROT 0898), the
    /// battle loader and the arts-list renderer, with the routines in the
    /// verified-dead SCUS regions.
    /// **Mutually exclusive with `--shiny-seru`, `--arts-ap-grant` /
    /// `--arts-ap-cost`, `--show-super-arts`, `--super-arts-pack` and
    /// `--delilas-challenge`** (same regions).
    #[arg(long, value_name = "DAMAGE_PCT", num_args = 0..=1,
          default_missing_value = "20",
          value_parser = clap::value_parser!(u8).range(0..=100))]
    pub(crate) oscillating_ap: Option<u8>,
    /// **Rename a place everywhere the game shows it**: the quick-travel /
    /// Door-of-Wind list, the label drawn over the world map at its map
    /// position, and the banner shown on entering the scene (which is also the
    /// save-screen location row). Repeatable `TARGET=NAME` entries, where
    /// `TARGET` is either a landmark cell index or the place's current name
    /// (`legaia-patcher locations` lists both - the 14 places with a world-map
    /// label but no quick-travel cell, e.g. "Hunter's Spring" or "Sol Tower",
    /// are addressable only by name). Matching is exact, so renaming "Conkram"
    /// leaves "Conkram (Past)" alone. The new name is ASCII, up to 23
    /// characters. E.g. `--rename-location "3=Ancient Fire Cave"`.
    #[arg(long, value_name = "TARGET=NAME", value_parser = parse_location_rename)]
    pub(crate) rename_location: Vec<(legaia_patcher::apply::RenameTarget, String)>,
    /// Add the in-shop **Seru trading** vendor: every merchant grows a fourth
    /// Trade row opening a screen that swaps a party member's learned
    /// Seru-magic for a different one (offer reseeds on a play-time bucket,
    /// deterministic from the run's seed). Runs on real hardware - the whole
    /// screen is hand-assembled MIPS hosted in the menu overlay - and the same
    /// seed is embedded in `SCUS_942.54` for the engine's trade UI.
    #[arg(long, default_value_t = false)]
    pub(crate) seru_trade: bool,
    /// Maximum trades a single vendor offers at once (only with `--seru-trade`).
    #[arg(long, default_value_t = legaia_game_tables::seru_trade::DEFAULT_MAX_OFFERS)]
    pub(crate) seru_trade_offers: u8,
    /// How random-encounter formations are reassigned. The pool each scene draws
    /// from is set by `--encounter-scope`.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) encounters: DropArg,
    /// Pool an encounter randomization draws from: `scene` (each scene's own
    /// monsters - the default, every monster stays a local resident),
    /// `kingdom` (any monster from the same kingdom: Drake / Sebucus / Karisto),
    /// or `world` (any monster on the disc, so late-game monsters can appear at
    /// the start). Only applies when `--encounters` is set. `kingdom` needs the
    /// disc's CDNAME.TXT; the wider pools rely on the battle loader streaming a
    /// monster by id, so an out-of-area enemy still loads and renders.
    #[arg(long, value_enum, default_value_t = ScopeArg::Scene)]
    pub(crate) encounter_scope: ScopeArg,
    /// Opt out of the solo-strong pass. It is on by default whenever
    /// `--encounters` is set: a randomized fight that would pit the party against
    /// a monster much stronger than the area's natives is forced to just that one
    /// monster instead of a pack of 2+ (cut-off `--solo-strong-threshold`). Pass
    /// this to keep the over-strong packs (vanilla behaviour for the formation
    /// counts).
    #[arg(long)]
    pub(crate) no_solo_strong_encounters: bool,
    /// "Strong fight" cut-off for the solo-strong pass, as a percent of the area's
    /// native average monster power (default 200 = twice as strong). A random
    /// formation whose strongest monster clears this bar is forced solo. Ignored
    /// with `--no-solo-strong-encounters`.
    #[arg(long, default_value_t = apply::DEFAULT_SOLO_STRONG_THRESHOLD_PCT)]
    pub(crate) solo_strong_threshold: u16,
    /// How treasure-chest contents are reassigned (global; `random` draws from
    /// the valid item pool, `shuffle` redistributes the existing chest items).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) chests: DropArg,
    /// How town-merchant shops are reassigned - what stores sell (global;
    /// `shuffle` redistributes the existing shop-item multiset across all towns,
    /// `random` draws each slot from the valid item pool). The town shop stock is
    /// inline in each scene's field-VM script (op `0x49`).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) shops: DropArg,
    /// How the casino prize-exchange is reassigned (`shuffle` redistributes the
    /// existing prizes, `random` draws from the existing prize pool; each prize
    /// keeps its coin price + progression gate). Distinct from `--shops`: the
    /// casino spends coins, not gold.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) casino: DropArg,
    /// How monster combat stats are reassigned (HP / MP / ATK / UDF / LDF /
    /// INT / SPD from the `battle_data` archive). `shuffle` permutes each stat
    /// column across the roster (each stat's multiset preserved, so the overall
    /// difficulty budget is kept); `random` draws each stat from that column's
    /// pool. AGL is left untouched - it gates the enemy AI's action economy
    /// rather than player-facing difficulty. `legaia-patcher monster-stats` lists
    /// the current stats.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) monster_stats: DropArg,
    /// **Scale every enemy's combat stats** by a difficulty multiplier
    /// (`0.1x..=5x` per stat, retail `1`). Three spellings, which nest: one
    /// number scales everything - `--enemy-stat-scale 2` doubles every
    /// monster's HP / MP / ATK / UDF / LDF / INT / SPD, `0.5` halves them - or
    /// a per-stat list scales only what it names, leaving the rest at retail:
    /// `--enemy-stat-scale hp=3` for spongy-but-not-lethal enemies,
    /// `--enemy-stat-scale attack=2,defense=0.5` for glass cannons. Stat keys:
    /// `hp`, `mp`, `attack`, `defense` (both halves) or `defense_high` /
    /// `defense_low` individually, `intelligence`, `speed`.
    /// Third, a `|`-separated **per-group split** gives random encounters and
    /// boss fights their own scale, each spelled either of the above ways:
    /// `--enemy-stat-scale 'regular:0.75|boss:2'` for a breezier road and
    /// harder set-pieces, `--enemy-stat-scale 'boss:hp=2'` to make only the
    /// bosses spongy. Groups are `regular` / `boss` / `all`; an unnamed group
    /// falls back to `all`, or to retail. Which monsters count as bosses is read
    /// off the disc's own encounter tables (scripted-only fights), not a list.
    /// Unlike `--monster-stats` this moves nothing between monsters - each
    /// keeps its own profile and each group shifts together, **story bosses
    /// included** (only the unwinnable-by-design Rim Elm sparring partner is
    /// pinned, since a weakened one can soft-lock the tutorial).
    /// AGL is left alone, as it gates the AI's action economy rather than
    /// difficulty, and EXP / gold / drops never move - a 5x run is harder, not
    /// richer. Seedless, and applied *after* `--monster-stats`, so the two
    /// compose. `legaia-patcher monster-stats` lists the current stats.
    #[arg(long, value_name = "MULT|STAT=MULT,...|GROUP:SCALE|...", value_parser = parse_stat_scale)]
    pub(crate) enemy_stat_scale: Option<legaia_patcher::monster_stats::ScaleProfile>,
    /// **Scale every battle's EXP payout** by a multiplier (`0.1x..=5x`,
    /// retail `1`): `--exp-scale 2` doubles every monster's base EXP reward,
    /// `--exp-scale 0.5` halves it. Edits the base-EXP halfword in each
    /// monster's record, so the victory spoils, the post-battle split among
    /// living party members, and the `--flee-exp` grant all scale together.
    /// Gold, drops and everything else stay retail. A scaled reward never
    /// drops to zero (floors at 1 EXP) and saturates at 65535. Seedless.
    #[arg(long, value_name = "MULT", value_parser = parse_exp_scale)]
    pub(crate) exp_scale: Option<legaia_patcher::monster_stats::ScalePermille>,
    /// **Override every capturable Seru's catch rate** with one flat percent
    /// (`0..=100`): the chance that a killing blow on a Seru monster absorbs
    /// its magic. Retail rates run from 80% (an early Gimard) down to 1% (the
    /// rarest late-game Seru); `--seru-catch-rate 100` makes every eligible
    /// kill absorb, `--seru-catch-rate 0` disables absorption entirely. Only
    /// the 63 capturable records are touched - the override never makes a
    /// non-Seru monster capturable, and the Ivory Book's +30-point bonus
    /// still applies on top (capped by the roll's own d100).
    #[arg(long, value_name = "PCT", value_parser = parse_seru_catch_rate)]
    pub(crate) seru_catch_rate: Option<u8>,
    /// **Scale how many hits enemies land with their standard attacks** by a
    /// multiplier (`0.1x..=5x`, retail `1`): `--enemy-attack-count 2` roughly
    /// doubles every enemy's physical strikes per turn, `0.5` halves them.
    /// Retail prices each attack in AGL and lets the per-round AGL gauge
    /// afford as many strikes as fit, so this divides each attack entry's
    /// AGL-cost byte by the multiplier while leaving AGL itself alone -
    /// composing cleanly with `--enemy-stat-scale`, which never touches AGL.
    /// Scaled costs round half up and clamp so an enemy that attacks in
    /// retail always lands **at least one** hit per attack turn (never zero),
    /// and the engine's own 15-action queue bounds the top end. Unavailable
    /// (`0xFF`) and deliberately overpriced entries are left alone, so
    /// movesets never change - only counts. Spell casts are untouched. Only
    /// the unwinnable-by-design Rim Elm sparring partner is pinned. Seedless.
    #[arg(long, value_name = "MULT", value_parser = parse_attack_count_scale)]
    pub(crate) enemy_attack_count: Option<legaia_patcher::monster_stats::ScalePermille>,
    /// How special-attack power is reassigned (the battle-action move-power
    /// table - enemy specials + Seru-magic, NOT party Tactical Arts). `shuffle`
    /// permutes the 44 power values (multiset preserved); `random` draws each
    /// from that pool. Only the power changes - each move keeps its own
    /// animation, effects, and sound. `legaia-patcher move-powers` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) move_power: DropArg,
    /// How the element-affinity matrix is reassigned (which element beats which:
    /// the 8×8 damage-scale grid). `shuffle` permutes the 64 cells (the same
    /// number of weaknesses / resistances exists, between different pairs);
    /// `random` draws each cell from that pool. Per-character element assignment
    /// is left untouched. `legaia-patcher affinity` shows the current grid.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) element_affinity: DropArg,
    /// How spell MP costs are reassigned (the SCUS spell table). `shuffle`
    /// permutes the MP costs of the named, costed spells (the cost multiset is
    /// preserved); `random` draws each from that pool. Free / internal-tier
    /// spells never gain a cost. `legaia-patcher spell-costs` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) spell_cost: DropArg,
    /// How equipment passive stat bonuses are reassigned (the SCUS bonus table).
    /// `shuffle` permutes each slot category's stat tuples (`INT/ATK/UDF/LDF/SPD`)
    /// among that category's gear (so weapon power lands on another weapon, armor
    /// on armor; the per-category budget is kept); `random` draws each from that
    /// category's pool. The equip-character mask, accessory passive, and slot type
    /// never move. `legaia-patcher equip-bonuses` lists the current table.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) equip_bonus: DropArg,
    /// How the equip-character mask is reassigned (who can wear each piece of
    /// gear - the `+6` byte of the SCUS bonus table). `shuffle` permutes the
    /// masks within each slot category (each character keeps the same count of
    /// equippable weapons / body / head / footwear, just on different items);
    /// `random` draws each row's mask from its category pool. Stat bonuses and
    /// slot type never move, so this composes with `--equip-bonus`.
    /// `legaia-patcher equip-bonuses` lists the current masks.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) equip_mask: DropArg,
    /// Reassign which weapon class each character specializes in (Vahn blades,
    /// Noa claws, Gala clubs/axes by default). Permutes the three favored
    /// families among the characters and rewrites the per-(character, weapon)
    /// arm-cost byte in the player battle files, so an off-class weapon widens
    /// the Arms command in an arts combo. The Astral Sword stays always-wide.
    /// `legaia-patcher weapon-specialty` shows the current favored class per char.
    #[arg(long, default_value_t = false)]
    pub(crate) weapon_specialty: bool,
    /// Set the Arts-gauge swing cost of one weapon for one character:
    /// `CHAR:ITEM=COST` (e.g. `Vahn:0xBA=30` makes the Astral Sword a
    /// favored-width swing). `COST` is the AP a press charges and the pennant
    /// width plus 6; retail tiers are 30 / 42 / 54, minimum 7. The byte lives
    /// in that character's player battle file, so the same weapon can be priced
    /// differently per character. `legaia-patcher equipment` lists every
    /// current value. Repeatable / comma-separated; applied after
    /// `--weapon-specialty`.
    #[arg(
        long = "swing-cost",
        value_name = "CHAR:ITEM=COST",
        value_delimiter = ','
    )]
    pub(crate) swing_cost: Vec<String>,
    /// Set who can equip an item: `ITEM=OWNERS` where `OWNERS` is any of the
    /// letters `V` `N` `G` (Vahn / Noa / Gala), `any`, or `none` (e.g.
    /// `0xBA=VNG`). Rewrites the item's equip-character mask in the SCUS
    /// equipment table; items sharing a bonus row move together (the report
    /// says which). A weapon a character's player file has no record for gets
    /// its model carried over from the file that has it (the three player
    /// files are re-packed and their boundaries moved to make room; past
    /// that the rebuilt file's records go to the DMY.DAT annex - same-size
    /// image, still a PPF). Body, head, footwear and Ra-Seru forms cannot
    /// be carried and fall through to the default look. Repeatable /
    /// comma-separated.
    #[arg(
        long = "equip-owner",
        value_name = "ITEM=OWNERS",
        value_delimiter = ','
    )]
    pub(crate) equip_owner: Vec<String>,
    /// With `--equip-owner`: do not carry weapon models over; a new owner
    /// swings with the default look (and the default record's cost).
    #[arg(long)]
    pub(crate) no_model_transplant: bool,
    /// With `--equip-owner`: when the carried-over weapon models fit neither
    /// the three player files nor the DMY.DAT annex, grow them with a
    /// whole-disc relayout. The image gets longer, so this needs `--output`
    /// and writes no PPF.
    #[arg(long)]
    pub(crate) allow_relayout: bool,
    /// How per-monster steal items are reassigned (the Evil God Icon table;
    /// `shuffle` redistributes the existing steal items, `random` draws from the
    /// valid item pool - the steal *chance* is always preserved).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) steals: DropArg,
    /// How each Tactical Art's button combo is reassigned. `shuffle` permutes a
    /// character's own combos among its arts; `random` draws each art a combo
    /// from the global pool of every regular art's combo. Either way every art
    /// keeps a combo that's unique within its character, and the Miracle Art is
    /// left untouched. `legaia-patcher arts` lists current combos.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) arts: DropArg,
    /// How scene-transition doors/exits are reassigned (one-way / decoupled:
    /// each door's whole destination - scene + entry tile + facing - is
    /// reassigned globally; `shuffle` permutes the existing destinations across
    /// all doors, `random` draws each from the global pool). Going back through
    /// the destination's own doors is not guaranteed to return you.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) doors: DropArg,
    /// Whether door randomization is bidirectional (`coupled`: re-pair doors so
    /// you can return the way you came) or one-way (`decoupled`: each door's
    /// destination is independent, so going back leads elsewhere). Only applies
    /// when `--doors` is not `none`.
    #[arg(long, value_enum, default_value_t = CouplingArg::Coupled)]
    pub(crate) door_coupling: CouplingArg,
    /// How intra-town (house / interior) doors are reassigned. Only `shuffle`
    /// is meaningful; `random` is treated as `none`. Covers both intra-town
    /// door classes: the scripted door warps (a per-scene, class-preserving
    /// shuffle of the player door-warp target tiles: interior landings permute
    /// among house entries, exterior doorsteps among exits) and the `.MAP`
    /// kind-0 intra-scene teleports (most house exits; a per-scene shuffle
    /// accepted only when the scene's walk-component reachability is
    /// preserved, so no rewire can strand the player). `legaia-patcher
    /// house-doors` / `map-doors` list the two populations.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) house_doors: DropArg,
    /// Number of random starting items the new game begins with (`0` = leave the
    /// vanilla Healing Leaf ×5 untouched). Each is a distinct random consumable
    /// with a small random count. The random fill shares the seed's capacity
    /// (7 slots, or 5 with `--all-warps`) with the convenience-item toggles, and
    /// takes whatever they leave - so it adds on top of them rather than being
    /// crowded out. `legaia-patcher starting-items` shows the current contents.
    #[arg(long, default_value_t = 0)]
    pub(crate) starting_items: usize,
    /// Seed Door of Wind (the warp consumable) into the new game's starting bag.
    /// Pass `--door-of-wind` for the default stack (10) or `--door-of-wind N` for
    /// N (1..=99). Additive to a normal new game (the Healing Leaf is kept)
    /// unless `--starting-items` also rerolls the bag. Pairs with `--all-warps`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "10",
        value_name = "COUNT"
    )]
    pub(crate) door_of_wind: Option<u8>,
    /// Seed Incense (the encounter-rate consumable) into the new game's starting
    /// bag. Pass `--incense` for the default stack (10) or `--incense N` for N
    /// (1..=99). Additive to a normal new game (the Healing Leaf is kept) unless
    /// `--starting-items` also rerolls the bag.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "10",
        value_name = "COUNT"
    )]
    pub(crate) incense: Option<u8>,
    /// Seed the Speed Chain accessory (always act first in battle) into the new
    /// game's starting bag. Pass `--speed-chain` for the default (1) or
    /// `--speed-chain N` for N (1..=99). Additive like `--door-of-wind`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    pub(crate) speed_chain: Option<u8>,
    /// Seed the Chicken Heart accessory (increases the successful-escape rate)
    /// into the starting bag. `--chicken-heart` for the default (1) or
    /// `--chicken-heart N`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    pub(crate) chicken_heart: Option<u8>,
    /// Seed the Good Luck Bell accessory (Low Encounter - halves the encounter
    /// rate) into the starting bag. It only puts the item in the bag; equipping
    /// it is what applies the passive.
    /// `--good-luck-bell` for the default (1) or `--good-luck-bell N`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    pub(crate) good_luck_bell: Option<u8>,
    /// Seed explicit item(s) into the new game's starting bag, on top of the
    /// convenience toggles and the Healing Leaf base. Comma-separated
    /// `id[:count]` entries, id in decimal or `0xHH`, count defaulting to 1
    /// (e.g. `--start-with 0x89:10,0xd1,154:3`). The id space is the full item
    /// table - any consumable, weapon, armor, or accessory id works. Items
    /// beyond the 7-slot direct seed (5 with `--all-warps`) are granted via the
    /// opening scene like the random fill. `legaia-patcher starting-items` shows
    /// the resulting bag.
    #[arg(long, value_name = "ID[:COUNT]", value_delimiter = ',', value_parser = parse_item_spec)]
    pub(crate) start_with: Vec<(u8, u8)>,
    /// Unlock every Door-of-Wind warp destination from the start (preset the
    /// "visited towns" story-flag bitmask). Lets Door of Wind teleport to any
    /// town immediately. It claims the warp-preset region that otherwise carries
    /// the last two starting-item slots, so the bag is capped at 5 items with it
    /// on (7 without).
    #[arg(long, default_value_t = false)]
    pub(crate) all_warps: bool,
    /// Start the new game at this character level instead of 1 (`0` or `1` =
    /// vanilla level 1). Seeds the lead character's cumulative XP and recomputes
    /// the starting stats to the level from the disc's own growth curves. Range
    /// 2..=14 (the XP seed is a single 16-bit immediate). `legaia-patcher
    /// starting-items` shows the current starting level.
    #[arg(long, default_value_t = 0)]
    pub(crate) starting_level: u8,
    /// Re-introduce unused enemies (the standalone "Comm" record, id 78, and
    /// the Evil Bat duplicates 176..=178 - ids no formation references) into
    /// the random-encounter pool. Only takes effect with
    /// `--encounters random` (a `shuffle` can't introduce a new monster).
    #[arg(long, default_value_t = false)]
    pub(crate) unused_enemies: bool,
    /// Re-introduce unused items (the "Something Good" sell item and the unnamed
    /// Seru accessory) into the valid item pool, so a `random` drop / chest /
    /// steal fill can hand them out. Only affects the `random` modes.
    #[arg(long, default_value_t = false)]
    pub(crate) unused_items: bool,
    /// Comma-separated item ids (decimal or `0xHH`) to keep in their original
    /// chests, never randomized - and dropped from the random-fill pool so they
    /// can't be duplicated elsewhere. Defaults to the disc's full quest / key /
    /// story item set (every unsellable item except the chest-found equipment,
    /// so no door key, garden tool, letter, book, or one-off story item is ever
    /// moved or randomly placed). `legaia-patcher chests` lists current contents
    /// to audit. Pass an empty value (`--keep-static-items ""`) to randomize
    /// everything.
    #[arg(long, value_delimiter = ',')]
    pub(crate) keep_static_items: Option<Vec<String>>,
    /// Write the portable PPF 3.0 patch here (defaults to `<input>.ppf`).
    #[arg(long)]
    pub(crate) patch: Option<PathBuf>,
    /// Also write a full patched disc-image copy here (contains Sony bytes -
    /// for local play only, never redistribute). A matching `.cue` is written
    /// beside it (single-track Mode 2/2352) so emulators that won't load a bare
    /// BIN - e.g. mednafen rejects a >64 MiB BIN - can open the image directly.
    #[arg(long)]
    pub(crate) output: Option<PathBuf>,
    /// Write a reproducibility manifest (seed + options + change summary) here.
    /// Safe to share alongside the PPF - it embeds no game bytes.
    #[arg(long)]
    pub(crate) manifest: Option<PathBuf>,
    /// Plan and report the run but write no files (patch / output / manifest).
    #[arg(long, default_value_t = false)]
    pub(crate) dry_run: bool,
    /// Proceed even when the disc is not the USA build (SCUS-94254). The
    /// randomizer's offsets and code hooks target the USA disc; running it
    /// against a PAL disc (SCES_019.44/.45/.46) produces a corrupt hybrid.
    #[arg(long, default_value_t = false)]
    pub(crate) allow_region_mismatch: bool,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum CouplingArg {
    /// Bidirectional - you can return the way you came.
    Coupled,
    /// One-way - going back leads somewhere else.
    Decoupled,
}

impl CouplingArg {
    pub(crate) fn coupling(self) -> apply::DoorCoupling {
        match self {
            CouplingArg::Coupled => apply::DoorCoupling::Coupled,
            CouplingArg::Decoupled => apply::DoorCoupling::Decoupled,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum DropArg {
    /// Redistribute the existing values (drops / encounter ids).
    Shuffle,
    /// Draw each value uniformly from the valid pool.
    Random,
    /// Leave untouched.
    None,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum ScopeArg {
    /// Each scene draws only from its own monsters (the classic behaviour).
    Scene,
    /// Each scene draws from any monster in its kingdom (Drake/Sebucus/Karisto).
    Kingdom,
    /// Each scene draws from any monster on the disc (regions fully mixed).
    World,
}

impl ScopeArg {
    pub(crate) fn scope(self) -> apply::EncounterScope {
        match self {
            ScopeArg::Scene => apply::EncounterScope::Scene,
            ScopeArg::Kingdom => apply::EncounterScope::Kingdom,
            ScopeArg::World => apply::EncounterScope::World,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ScopeArg::Scene => "scene",
            ScopeArg::Kingdom => "kingdom",
            ScopeArg::World => "world",
        }
    }
}

/// Lowercase name of a mode for the manifest (valid-TOML string value).
pub(crate) fn mode_str(mode: DropMode) -> &'static str {
    match mode {
        DropMode::Shuffle => "shuffle",
        DropMode::Random => "random",
    }
}

impl DropArg {
    pub(crate) fn mode(self) -> Option<DropMode> {
        match self {
            DropArg::Shuffle => Some(DropMode::Shuffle),
            DropArg::Random => Some(DropMode::Random),
            DropArg::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_str_is_lowercase() {
        assert_eq!(mode_str(DropMode::Shuffle), "shuffle");
        assert_eq!(mode_str(DropMode::Random), "random");
    }
}
