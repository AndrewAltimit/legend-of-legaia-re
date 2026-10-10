# Randomizer: Delilas Challenge, custom items and party swap

Three [`legaia-patcher`](randomizer.md) features built around the Delilas
siblings (Gi, Lu and Che), the three recurring boss duellists:

| Feature | Flag | What the player gets |
|---|---|---|
| [Delilas Challenge](#delilas-challenge) | `--delilas-challenge` | a fourth Muscle Dome enrollment option: a new 2-round arena course (Che and Lu together, then Gi) paying 5000 coins and a reward item |
| [Custom items](#completion-reward---a-honey-or-three-custom-items) | `--custom-items` | three new items (Nature's Elixir, Ra-Seru Tear, Fury Bloom) in cut item slots; standalone, and the course's reward when both are set |
| [Delilas party swap](#delilas-party-swap) | `--delilas-party V,N,G` | play as the siblings: the party wears their battle models, names and elements, and the story duels field Vahn, Noa and Gala instead |

```bash
legaia-patcher randomize --input DISC.bin --delilas-challenge --custom-items
legaia-patcher randomize --input DISC.bin --delilas-party gi,lu,che --delilas-moves delilas
```

These are the largest features the patcher ships. Unlike the table randomizers
they combine script edits, model and animation rebuilds, audio re-voicing and
code injection. This page is their full reference. The flag summary is on
[`randomizer.md`](randomizer.md#added-mechanics); the shared write path and the
injected-code arenas are on [`randomizer-internals.md`](randomizer-internals.md).

## Delilas Challenge

`--delilas-challenge` adds a fourth option to the Muscle Dome enrollment
clerk's "who will be entering" menu (`delilas_challenge` module): the
**Delilas Challenge** - a brand-new 2-round Muscle Dome *course* - **Che &
Lu Delilas together (1v2), then Gi (1v1)** - with **5000 coins** paid into
the dome winnings counter for a full clear. The gauntlet exists nowhere in
retail (the Nivora Ravine confrontations are one-off solo duels spread
across the story, and no retail formation anywhere fields two distinct
Delilas). It is delivered as a dome course because a normal battle staged
from the town scene `koin1` freezes on any spell (the scene never installs
battle-effect / summon / player-magic asset residency - which is exactly why
the Muscle Dome disables magic).

The double-team round is the interesting engineering: two full Delilas
blocks (163-166 KB of pre-texture bytes) overshoot the battle heap's ~145 KB
distinct-monster budget, and the failing malloc returns NULL that the loader
uses unchecked - a naive 1v2 froze at the round-1 load (the byte-level
mechanism is the battle-heap-budget section of
[`battle.md`](../subsystems/battle.md#the-battle-heap-budget---why-a-formation-of-large-distinct-bosses-cannot-load)).
The shipped fix streams **slim clones** without touching the originals:

- `legaia_asset::monster_archive::slim_castables` rebuilds Che's and Lu's
  blocks minus most castable spell entries, under two probe-traced safety
  constraints. First, exactly **one** rollable castable survives (Che's
  entry 6, Lu's entry 7): the AI's cast pick is `rand % castable_count` and
  an empty menu executes the div-guard `break 0x1C00` the BIOS parks on
  (the [AI-picker
  section](../subsystems/battle.md#monster-ai-fun_801e9fd4-action-picker--fun_801e7320-target-resolver)
  has the mechanism). Second, each sibling's **streamed signature special
  stages block entries by raw index** (Lu's Plasma Strike, action `0x7B`,
  stages `14 -> 12 -> 13`; Che's, action `0x7A`, stages `10 -> 11`), and an
  aliased stand-in never satisfies the module's completion wait - the
  caster loops its approach run forever - so those entries survive
  verbatim (`delilas_dome::slim_policy`); Lu instead force-drops her
  unstaged `0x23` special at entry 11 to pay for them. Choreography entries
  are not standalone casts: promoting Lu's never-rolled entry 12 to the
  rollable menu was tried and wedges her first generic cast. Mesh, stats,
  name, reactions survive byte-identical; the entry count and index space
  are preserved - kept entries keep their retail indices and a dropped slot
  aliases the basic-attack entry (safe for every consumer except the
  index-staging modules above, hence the protection). The slim pair costs
  ~137 KB - under budget with ~8 KB of combat headroom.
- The clones are written to archive slots 190/191, two ids no formation,
  encounter, or dome roster on the disc ever references (full-disc sweep;
  also outside the `--unused-enemies` pool). The real 163/164 slots are
  never modified, so the ravine duels and the Master course keep every move.
- The formation seats the **real ids** 163/164 - the bespoke
  attack-attack-special AI, the names, and every id-keyed table stay
  genuine. Two 10-word cave routines hooked at the loader's two
  id-to-slot-offset sites (`0x8005451C` in the streamer, and one
  instruction *before* the first-enemy pre-streamer's conversion at
  `0x80054B70` - the conversion's own delay slot clobbers the id register,
  so the hook cannot sit on it) add 27 to the id **for the archive fetch
  only**, and only while the course word reads `0x131` (the Delilas course,
  round 0) - every other battle streams the untouched originals.
- Seru magic is locked out through **retail's own reject path**: the
  battle round driver's two Magic-command input arms reject the selection
  when `_DAT_8007BAC0 & 0x200` - the Master course's lockout bit
  (`overlay_0898_801d0748`; Beginner/Expert seeds lack it and genuinely
  allow magic). The patch widens each test's mask to `0x300` (two
  same-size words in the battle-action overlay, PROT 0898), so the reject
  fires on the dome-contest marker `0x100` every contest seed carries -
  including the course's own `0x131`. The lockout matters because the
  arena never installs the summon / player-magic sound+art residency: a
  live test cast Meta in the course and got audio-state corruption (the
  koin1 magic-freeze class). Retail Beginner/Expert legs lose their latent
  magic access too - the same corruption waits there.
- **The special cadence is retimed, per round.** The bespoke Delilas AI
  arm (`case 0xa2..0xa4` in the monster picker `FUN_801E9FD4`, battle
  overlay) queues the signature special whenever the shared battle turn
  counter `ctx[0x28A]` hits `% 3 == 2` - attack, attack, special, with
  both siblings synchronized in a 1v2. The course reroutes the arm: its
  ctx reload (`0x801EB7C4`) jumps to a course-gated block that resumes the
  stock arm register-exactly for every non-course context (ravine duels,
  Master rounds), and in the Che & Lu round (course word `0x131` exactly)
  fires a sibling's special on `(counter + offset) % 4 == 3`, Lu offset 2
  / Che offset 0 - one special every four turns, staggered two turns
  apart. The block overwrites the SCUS passive-name draw `FUN_80035274`,
  a 48-instruction function with zero references of any form in any image
  (the five-form address-word scan; `port-catalog-ignore.toml`
  `[unreferenced]`) - the only always-resident home left once the shared
  rodata gap filled and the battle overlay's image proved packed
  (`static-overlays.toml`: all `.text+.rodata` RAM-matched live).
- **Gi borrows two one-shot casts in his round, and Divide retires
  Blazing Slash.** In the course's Gi round (word `0x132` exactly, seat 0
  only - a Divide clone never re-casts) a second cave arm (over the
  unreferenced SCUS angle tween `FUN_80050d40`, reached by `j` from the
  main block) queues **Divide** (`0x50`, Green Slime's split) on Gi's
  first pick with HP strictly below half of max (`+0x14C < +0x14E >> 1`)
  and **Spore Gas** (`0x4F`, Berserker's status cloud) on his first pick
  with the shared counter at or past 3, by writing the actor's cast queue
  (`+0x1DE = 2`, `+0x1DF` = the spell id) - the exact mechanism the
  signature specials ride: both are capture-class spells whose modules
  stream from PROT 0940 / 0939 at cast time
  ([`spell-table.md`](../formats/spell-table.md)). Each is one-shot via a
  two-bit flags word in the display cave's data tail, zeroed by the
  Che & Lu arm on every round-0 pick (a course always re-enters through
  round 0, so re-enrollment re-arms both). Before Divide, non-cast picks
  resume the stock arm and Blazing Slash keeps its retail `% 3 == 2`
  cadence; after Divide, they jump straight to the arm join so the
  generic attack-picker runs and Blazing Slash can never be queued again
  (the clone always takes that path - it melees only). Gi's monster block
  is byte-untouched.
- **The dev reporter's `PRG ERR%d` paint is gated off.** The 1v2's tight
  heap fails transient effect-instance allocs in bursts (retail tolerates
  the skipped spawns), but every failure bumps the malloc accumulator
  `gp+0x510` that the on-screen dev error reporter (`0x80016444`) prints
  from. The `beqz` guarding that one print arm becomes an unconditional
  branch; the WORK/READ/CD error arms and the accumulator itself are
  untouched. (The spawner's own `ori 0x4000` failure flag at `0x800211A0`
  is *not* the mechanism - nothing on the disc references the word it
  writes.)

The option is gated on story flag `0x378` - the flag the Koru death event
latches and the world map reads to flip the ravine entrance from `nilboa`
to `nilboa2` - so until that event the clerk brushes the player off. On by
default in the web patcher's Balanced and Full Chaos presets.

### Completion reward - a Honey, or three custom items

A winning course settle grants a reward alongside the 5000 coins, via a
two-word detour at the settle's post-payout `s0` staging (`0x801D114C`)
into a grant routine in the AI-block cave's tail (`0x800352EC`), gated on
the settling course being 3. Which reward depends on `--custom-items`
(the web patcher's "Custom items" toggle, on in the Balanced and Full
Chaos presets):

- **Off** (the default): the grant gives one retail **Honey** (`0x65`,
  the permanent all-stats-+4 consumable) per clear
  (`custom_items::plan_grant` over `[HONEY_ITEM_ID]`). Only the grant
  cave and the arena settle hook are written - item records, effect
  descriptors, jump tables, and the battle-overlay hooks all stay retail.
- **On**: the grant gives **three brand-new items**, described below.

The venue's award ceremony announces what was actually won. Retail's
post-contest narration in `koin1` reads "That was a good fight. / Well
done. / We hope you enter again." and then "Contestant {name} is awarded /
{n} tokens!" - but the Delilas course sits outside the token payout table
that counter substitutes from, so a course win read "0 tokens" and never
mentioned the items. The koin1 script edit splices four SYSTEM-flag tests
at the narration record's own branch point - right after its `76 CB` /
`75 38` tests, *before* the message run, because the actor-dialog SM
consumes a `[text][24][text][48]` flow as one contiguous run and control
ops spliced inside it cut the ceremony short (live-tested). Any
course-unlock flag set (`0x536`/`0x537`/`0x538` - a retail arm always sets
its own, the Delilas arm clears all three) keeps the retail flow, and
otherwise the settlement's contest-won flag (`0x50A`, set only on the
cleared-course path and untouched anywhere in `koin1`) routes to a
complete parallel flow appended past the record: the shared "good fight"
box copied verbatim (LZS folds the repeat), then "You won 1 Ra-Seru Tear,
/ 1 Nature's Elixir and / 1 Fury Bloom!" (or "You won 1 Honey!" on the
fallback), then the verbatim close-and-settle ops and a jump to the same
park loop both retail arms converge on. A Delilas loss falls through into
the retail box (`delilas_challenge::reward_box_lines`,
`CONTEST_WON_FLAG`).

`--custom-items` is a standalone feature, not a Delilas sub-option: the
item set (`CustomItemsInjection::plan_item_set` - records, descriptors,
jump-table arms, item-machinery caves, battle-overlay hooks, and **no**
arena writes; applier `apply::inject_custom_item_set`) installs without
the challenge, and a `random` drop / chest / steal mode then adds the
three ids to the fill pool (`custom_items::CUSTOM_ITEM_IDS`, the
`--unused-items` `extend_pool` shape) so they turn up as loot. With
neither a `random` mode nor the challenge, the items exist but nothing
hands them out - both CLIs print a note. The Delilas grant is the
second, independent half; `inject_custom_items` composes the two and
each half is idempotent on its own key (applier jump-table word / arena
hook word), so the item set landing first and the challenge arriving on
a re-run still completes the grant.

The items are claimed from the item table's only
free slots - the executable's three empty-name records (`0xB9` the cut
Ra-Seru-egg item, `0x12` / `0x1A` the cut top-tier Ra-Seru weapon slots;
a dual census - the curated gamedata cross-reference and the patcher's
own drops / chests / steals / shops / casino / fishing / starting-item
sweeps - shows every other named id is reachable in retail):

- **Nature's Elixir** - restores an ally's HP *and* MP to full, in the
  field menu or in battle. Its effect class is a new applier jump-table
  arm (`0x48`) that fills MP (with the retail `-amount` mirror write) and
  tail-jumps into the retail tier-2 HP-restore arm, so the popup and
  displayed-HP accounting are retail's own.
- **Ra-Seru Tear** - battle-only; using it turns the committed item
  action into a **real Ra-Seru summon cast of the user's own Ra-Seru**
  (Vahn Meta, Noa Terra, Gala Ozma) with the 240-MP cost skipped - a
  free summon any character can shed. The conversion hooks the
  action-seed category dispatch (`0x801E2D60`): the item is already
  deducted at menu commit, the spell id is `0x9D +` the roster char id,
  and a one-shot flag makes the summon leg's unconditional MP deduct
  (`0x801E4584` - it underflows a u16 when unchecked) write a zero-cost
  mirror instead.
  Caster-matched is a mechanism constraint, not flavour: the big summons
  stream the caster's own choreography, and a forced mismatched pair
  (Gala casting Meta) parks the battle forever in the summon driver's
  completion poll while the matched pair completes and lands damage.
- **Fury Bloom** - battle-only; one use sets the Fury Boost
  action-gauge extension (`actor+0x1F9`, exactly the retail class-5
  arm's write) on **every living party member**, with the retail Fury
  cue on the user. Party-wide application needs a custom arm because
  group targeting lives inside each retail class arm, not the
  dispatcher. (An earlier "Delilas Tear" that would cast a sibling
  signature attack parks the battle at the capture band's completion
  wait. The reason is **not** "a party actor has no monster block" - see
  [Casting a sibling signature attack from a party
  slot](#casting-a-sibling-signature-attack-from-a-party-slot) for what
  actually blocks it, and for the seat-0 hazard that makes it unsafe to
  ship even where it runs.)

All three items play the heal-item chime (`FUN_8004FCC8` cue `0x20C`)
from their own arms: the cast-audio dispatcher `FUN_801F3990` maps item
classes to cues through a 9-entry table, so custom classes are silent
unless the arm plays the cue itself.

Either grant rides the settle's own latch-gated winning arm (the code
path that pays the coins and, on the Master course, the retail War God
Icon): the detour runs one `FUN_800421D4(id, 1)` give per reward item
when the settling course is 3, then replays the displaced pair
(`custom_items::assemble_grant_routine_for`). New code follows the same
unreferenced-function cave discipline as the course itself - the
class-14 Point Card arm (reachable code with no reachable data), three
more zero-reference SCUS functions, and the tails of the course's own
two caves - plus one rule the course never needed: **a claimed cave
must also survive a cold boot**. The zero-reference libapi VBlank-tier
slot `FUN_800605C8` passes every static scan and every save-state
probe, yet the kernel invokes it during boot init and a disc that
overwrites it parks at the PS1 logo
([`re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md)); the
cold-boot watcher `autorun_boot_watch.lua` is now part of the cave
verification standard. The menu validator's class table points both
new classes at its existing always-usable arm.

The feature is two coordinated halves that ship together
(`apply_delilas_challenge` installs both):

- **The koin1 menu + warp** (`delilas_challenge` module, script + data). The
  3-option `0x28` who-enrolls picker grows to the 4-option `0x29` form (the
  picker arity ceiling), with the new arm appended at the record's end, and
  the quick-path skip before it is retargeted at never-set flags - retail
  permanently hides the who-menu once Noa or Gala has refused enrollment
  (flags `0x559`/`0x558`) and auto-registers Vahn, which would strand the
  new option on most saves. The new arm shows a confirm picker, then mirrors
  one of retail's own difficulty arms exactly: gate on `0x378`, set the
  dome-active flag `0x509`, clear the three course-unlock flags
  `0x536`/`0x537`/`0x538`, set the course-3 request flag `0x539`, then the
  verbatim BGM + wait ops and the verbatim `3E 69` arena warp. Losing a
  round returns to the Sol venue by the dome's own design (no game over).
- **The arena course** (`delilas_dome` module, a code injection). The arena
  overlay (PROT 0977) has no course beyond Master, so course 3 is added with
  six same-size edits: a seed detour at the course-select init
  (`FUN_801CEA6C` @ `0x801CEBCC`) that decodes flag `0x539` into the packed
  course/round word as course 3 - guarded on the retail flag seed having
  left the word at its no-course default `1`, which only the Delilas arm
  (it clears all three course-unlock flags) produces, and clearing `0x539`
  whether or not it seeded, so a stale request flag (a save state frozen
  during the warp flourish) is scrubbed instead of hijacking a
  Beginner/Expert/Master enrollment into the Delilas course; a `{round_count=3, roster_ptr}` descriptor
  at the course-3 slot `0x801D1A08 + 3*8`; the hub actor template that
  occupied that slot relocated into a `SCUS_942.54` routine cave (its one
  `lui`/`addiu` reference repointed); the Delilas roster (Gi/Che/Lu, the
  dome's own name strings) in the cave; the seed routine; and a **reward
  detour** at the settlement's payout-table load (`FUN_801D0F60` @
  `0x801D1118`) that returns 5000 for course 3 instead of the out-of-table
  read (courses 0-2 pay from `0x801D1860 + course*0x40 + (round-1)*4`;
  course 3 indexed past it into bytes that read 0 - the "0 tokens" an
  unhooked clear paid). The payout load sits behind the settlement's cleared
  latch (`DAT_801D1ADC`, raised only on *course exhausted and survived*), so
  a loss pays nothing - and retail's halve-the-winnings-on-a-loss behavior
  is untouched. All four descriptor readers compute `base + course*8`, so
  one descriptor write makes course 3 work everywhere. One more cell
  matters: the arena hub's round-end routing (`0x801CEE44`) reads the koin1
  menu-selection residue at `0x80084448` and treats the value **4** as "the
  player chose the quit option" - clearing the win latch and zeroing the
  winnings. The Delilas enrollment rides the who-menu's 4th slot, so its
  residue is exactly 4; the seat routine clears the cell at round-0 install
  (the intermission menu overwrites it with its own small pick afterward,
  so one clear suffices). Leaving the course mid-way settles as a loss
  (halve), not a quit (zero). The results screen reads the same payout
  table again on its own (`FUN_801D1184` into the winnings-display
  variable `DAT_801D1AAC`), so a second, display-only detour at its final
  address add (`0x801D125C`) shows the same 5000 - the override lives over
  `FUN_800260DC`, the SCUS per-mode camera preset, the second
  zero-reference function claimed after the Gi arm filled `FUN_80035274`.

The `koin1` MAN is sector-aligned with zero compressed slack, so the grown
script only fits back into its footprint through the optimal LZS packer -
which is why every MAN re-pack site in the patcher now falls back to
`compress_optimal` when greedy misses (`compress_within`). If another edit
(e.g. a grown language pack) has already consumed the headroom, the koin1
half skips with a note instead of failing the run. Seedless; re-application
is a no-op (the arena injection is idempotent on the seed-hook detour). The
dome fields whichever fighter the arena normally seats; routing a chosen
party member into the arena's fighter slot is an open RE thread.

**Shares SCUS arena bytes with shiny-Seru and the arts AP override.** The dome
course's seed routine + template cave lives in the same verified-dead SCUS
region (`0x8007AE00`) that [shiny Seru](randomizer-internals.md#shiny-seru) and the arts AP override
use, and that region is full - the surrounding zero-runs are live-table
padding, not dead space. So the three cannot coexist. The Delilas Challenge
takes precedence over shiny-Seru (it is the headline feature the presets
carry): when both are requested, shiny-Seru yields with a note and the
challenge is installed. The arts AP override, which is manual-only, is a hard
conflict with either - enabling it alongside the challenge (or shiny-Seru) is
refused up front. This is why the Balanced/Full Chaos presets, which enable
both the challenge and shiny-Seru, install the challenge and skip shiny-Seru.

Two byte-level details are load-bearing and both were caught by live play,
not by the static suite:

- **The seed word must carry bit `0x100`** (`0x131`, not a bare course/round
  encoding): the battle-exit selector `FUN_80046A20` tests
  `_DAT_8007BAC0 & 0x100` to route a leg's end back to arena mode `0x18`.
  Without the bit a lost leg fell into the ordinary game-over gate and a won
  leg would have exited to the field instead of the between-leg hub. Every
  retail seed (`0x101`/`0x111`/`0x321`) carries it.
- **The who-menu skip tests are retargeted, never NOPed**: `0x21` is the
  field VM's frame-yield/stop opcode
  ([script-vm.md](../subsystems/script-vm.md#per-frame-scheduling)), not a
  no-op - an earlier build's eight-byte `0x21` fill broke the clerk dialog
  mid-interaction (the "re-talk the NPC several times" bug). The tests keep
  their exact retail op shape with the flag id swapped to never-set flags.

> Verified by the `delilas_challenge_real` and `delilas_dome_real` disc
> oracles: the retail image locates as unpatched and every hooked address
> matches the known US build; the patched koin1 MAN re-parses with the
> 4-option picker targeting the new branch (gate, confirm picker,
> dome-active/course-unlock flags, course-3 request, arena warp all asserted
> at decode level, and no scripted-battle op); the seed hook becomes a `j`
> into the cave, the course-3 descriptor and roster land, the edit is
> byte-deterministic and idempotent, composes with the Earth Egg price edit
> in either order, and every touched sector stays EDC/ECC-valid.
>
> **Live-verified** (the 3x 1v1 course shape): the clerk dialog flows
> cleanly to the option, the confirm shows, the warp lands in the arena, the
> course runs its roster one round at a time, a lost leg routes through the
> dome's own loss scenes back to the venue (no game over), and settlement is
> benign. The double-team revision was live-falsified (freeze at the round-1
> load - two distinct Delilas blocks miss the battle heap budget; the byte
> arithmetic is in `battle.md`'s heap-budget section) and reverted. The
> 5000-coin payout hook passes the static + disc oracles; its live
> confirmation and the custom-item prize are the follow-ups.

## Delilas party swap

`--delilas-party gi,lu,che` (any permutation of the three siblings, in
Vahn, Noa, Gala order) swaps the playable party's battle identity with the
Delilas siblings: each character keeps their own animations, arts, magic,
stats and story, but wears the mapped sibling's battle model and fights
under their name, while each sibling's monster block is rebuilt around the
mapped character's battle model - so the three Nivora Ravine duels and the
Muscle Dome Master legs field Vahn, Noa and Gala, performing the retail
Delilas movesets (the streams pose parts by index, so a same-part-count
model inherits every move - the same law `monster-model` replacement rides).

Mechanics of the swap (`legaia_patcher::party_swap`): both pose systems use
flat per-part rigid transforms addressed by part index, and both texture
systems are 4bpp indices + 16-colour CLUTs, so the swap is an anatomy
permutation (players order bones `torso, pelvis, head, arms, legs`; every
Delilas mesh orders parts `head, torso, pelvis, arms, legs`; Noa's extra
hair bone merges into the head part), a **pivot-anchored rest-pose
bake** per part (`v' = R_t^T S(R_align (R_s v))` - each part anchors at
its rest **pivot**, the joint the engine rotates it about; `R_align`
turns the whole rig onto the host's facing and then swings each part
minimally onto its own host bone, pinning the twist; the part scales
axially onto the target's joint-to-joint span and radially by the
uniform height ratio, so the chains stay closed under every clip while
the sibling's shapes survive), and a bit-exact texel re-layout (islands
shelf-pack between the monster page and the party band's five section
tiles, palettes carried over - union-merged on the player side into the
CLUT columns `record[0]` does not claim). On the player side **every
equipment-section record** of the `PLAYERn` file is rewritten (the swap
must survive any equipment) with the swing action records and the
character's own `record[0]` animation source preserved; the descriptor
chain repacks with sector-aligned slots inside the retail entry footprint.
New-game template names follow the mapping ("Gi" / "Lu" / "Che"; existing
saves keep their stored names).

Two of the sibling "fists" are really **welded weapons** - Che's armB
hammer-fist and Gi's armA blade-fist, each authored several times the
radius of a real hand - and a welded weapon posed by a host clip whose
hand rotations assume a hand-sized part sweeps across the body (Gi's
blade under Noa's Spirit charge was the catalogued streak). The swap
replaces each with the sibling's other fist, mirrored across the source
body's sagittal plane (`playerize::WELDED_WEAPON_FISTS`), and puts the
**host's own equipped weapon** in the hand instead: per held-item section
record, the same curated item-alone cut the equipment viewer uses
(`equip_isolate`) recovers that record's welded weapon geometry as the
retail prims verbatim - shape, UVs, colour words and ABR bits untouched
- and the result merges into the baked hand at record-rewrite time,
after the hand's centroid seat + FK inset, so the blade never drags the
fist's placement (`party_swap::weapon_fuse`). The weapon keeps its
**real texture**, riding three measured invariants: every fusable
weapon's UVs stay inside its own section's band tile, every weapon uses
at most two CLUT columns, and a held-section record's pool upload is a
fixed-size tile whatever it contains. So the band relayout keeps the
sibling's islands out of the weapon section's tile, each weapon-section
record keeps its **own retail tile pixels** (equipping the weapon
repaints the tile with exactly the texels its prims sample - per-record
texture at zero extra pixel cost), and the weapon's palettes ride the
record's own CLUT run at two reserved band columns (the highest ones
record[0] does not claim); only the prims' CBA column is remapped.
Ra-Seru records (item ids `0x01..=0x1A`) keep their bare arm - dressing
the Ra-Seru arm is a separate cut.

One reservation is about a *writer*, not a reader: the per-frame facial
animator (`FUN_8004C7B4`) MoveImages the character's eye and mouth
frames onto fixed rows of **section 1's** band tile every battle frame -
the neutral face is re-stamped even when no track record is active, and
the frame strips ride `RECORD0_TEXTURE_RECTS[0]`'s tile, which the swap
keeps retail. Any sibling texels the relayout parks in that destination
window are overwritten with the host's face the moment a battle starts -
on-disc renders look clean while the live game shows the host's face
palette-mismatched through the sibling's CLUT (a saturated confetti
patch on whatever body part UV'd there; Gi's chest was the reported
case). The relayout therefore splits section 1's region around the
per-character eye+mouth destination union
(`playerize::FACE_STAMP_WINDOWS`, mirrored from the static SCUS
face-geometry tables and disc-asserted by
`party_swap_face_window_real`), leaving the window zero and
unreferenced; the emitted pool still covers the section's full rect.

A second reader nobody sees offline is the Spirit charge's streamer
effect: it samples authored **vertex indices** of specific assembled
objects (the host's hair part and the `0xFE` weapon extra carry 49 / 56
vertices on Noa's retail assembly), and a swap slot whose geometry is
deliberately dropped - the hair channel a sibling has no part for, or
the discarded weapon extra - answers those reads with whatever bytes sit
past its empty pool. The effect's trail fan then stretches
mesh-textured quads from the actor to the same garbage point every run:
the "Spirit streak", visible for the first ~20 ticks after the trail
spawns and invariant to every anim-data edit (measured by pinning the
fan packets in the live display list - position, colour and UV garbage
together, i.e. a struct read past a pool, not a pose defect). The
rewrite therefore never emits a truly empty object where retail carried
geometry: such slots get a **prim-less stub** - the retail slot's
vertex count of origin-pinned vertices, so a sampled streamer anchors on
the part's own socket and nothing is ever drawn - and the first
variant-carrying section re-emits one stub `0xFE` extra (tag `100`,
ordinal 1, inside the two-pair variant snapshot) so tag-100 lookups
resolve too. Two sibling hardenings ride along, both measured live: the
charge-loop record `0x11`'s window (`+0x85/+0x86`) is clamped to the
frame count of the **main-archive** stream its `stream_source` aliases
at commit (the side-band buffer routinely holds the main archive when
the base-archive clip materializes - on retail too - and the scratch
past the decoded stream is unwritten zeros), and `delilas-verify`'s
`0xFE` check now passes exactly one prim-less tag-100 anchor and fails
anything that draws.
One
more per-sibling shape aid: Che's round torso over a narrow loincloth
opens sky wedges at the waist under a low battle camera, so his pelvis
top ring cones up into the torso interior as a sight-line curtain
(`playerize::WAIST_CURTAIN_IDS`). each
character's PROT 0874 field mesh + atlas window rebuilds from the
sibling's **own field NPC mesh** (the nilboa duel-scene pack, PROT 0639,
with rest poses + head TIMs from PROT 0638) - retail-authored chibi
geometry that fits the pack budget at full detail. Each NPC part bakes
into the party field bone's local frame through the same pivot-anchored
bake as the battle side (the locomotion clips rotate each bone about its
pivot, so pivot anchoring + axial length matching is what keeps the
walking chains and the neck closed). NPC meshes are authored for
fixed-camera scenes and are not watertight from free angles - the
renderer's winding cull opens them up - so the rebuilt head and torso
carry a winding-reversed twin per prim (a size ladder narrows the scope
until the container fits), and the head's genuine openings (the hair
shell's underside, which the fixed scene camera never exposed) seal
with flat fan fills in the part's fill colour before the relayout. The
head re-lays into the character's atlas window,
bodies stay flat vertex colours - the retail field style. The
battle-model conversion survives as a fallback when the NPC source is
unavailable. The rebuilt container keeps the entry's first four header
words byte-exact (see [`character-mesh.md` § Not a dual
consumer](../formats/character-mesh.md#not-a-dual-consumer---the-battle-vdf-pack-is-a-different-entry)
for what that rule rests on, and what it does not).
`R_align` reads the rig as a whole rather than joint by joint, and that
is a correction, not a stylistic choice. Building each part's frame from
a per-joint bend-plane reference makes the alignment depend on **which
kind** of reference each side happened to find - child bone, parent bone,
or a world axis - and the two sides do not always find the same kind. Che
is the only rig with a measurable pelvis-to-torso bone, so his torso took
its bend plane from real anatomy while every player torso (whose pelvis
pivot sits on its torso pivot) fell through to world Z; the two are
incomparable, and the alignment came out as a **167.9-degree roll about
his own spine** - he wore his torso backwards, and the shoulder tuck then
seated his arms through it, flattening each upper arm to a quarter of its
width. Gi's case is milder and has the same shape: both sides picked real
anatomy, but the rigs flex their elbows in unrelated directions (122
degrees apart), so his upper arm rolled 166 degrees about its own axis.
Refacing the whole rig first (up = pelvis-to-head, lateral = the two
shoulders - landmarks all six rigs carry) and then swinging each part
onto its own host bone leaves no excess roll on any non-terminal part of
any pairing. Terminals - head, hands, feet - deliberately keep their
inherited frame, because the feet normalisation pre-cancels that one.

Note this is **not** a joint-gap defect: no chain edge was ever flagged,
before or after, which is why the gap-based probes were structurally
blind to it. It shows up only in a per-part affine fit against the
sibling's own mesh.

The **victory poses** follow too (`party_swap::winpose`): each
character's eight win-pose streams (the base "ME" archive, `readef.DAT`
slot `3*char+2` - [`battle-data-pack.md` § "ME" stream
archives](../formats/battle-data-pack.md#me-stream-archives-readefdat))
rebuild from the mapped sibling's own victory clip (monster action tag
`0x22`; Che ships none, so his last `0x23` flourish stands in),
retargeted onto the player rig with the bake's per-part conjugation
(which must be built from the **bake's own** frames via
`playerize::bake_frames` - the played pose cancels the bake's `R_align`, so any
change to how the mesh is oriented is a change the win pose has to
mirror or every converted pose inherits the difference),
re-encoded with the retail channel-delta codec (encoder
`winpose::encode_channel_delta`, round-trip self-checked through the
retail decoder on every apply).

A win pose is **not** a one-shot stream, and this is what a uniform
resample gets wrong. Every one of the 24 retail base records carries a
loop window: entry `+0x84` seeds the hold counter `actor+0x176` and
`+0x85`/`+0x86` bound the frames the tick replays (`FUN_80047430` -
once the 12.4 cursor reaches `+0x86 << 4` it subtracts
`(+0x86 - +0x85) << 4` and decrements the counter, so the stream cycles
that span up to 255 times before the results sequencer moves on). Retail
authors those frames as a seamless celebration cycle: measured across all
24 records, the pose gap over the wrap is 0.1-1.0 model units and
0.35-2.01 degrees per part, at or below the window's own mean frame step.
Resampling a one-shot flourish uniformly into that shape points the
window at the flourish's own tail, so the last second replays for as
long as the results panel is up - measured on the rebuilt streams, a
4.0-164.1 unit / 5.4-122.9 degree snap at every wrap.

So the rebuilt stream is **composed**, not uniformly resampled - and
the lead-in `[0, +0x85)` stays **retail, verbatim**. The base streams
are not victory-only: anim `0x11` (entry 0) is the Spirit / Super
power-up flourish the battle plays right after every `0x10` charge
(probe: actor `+0x1D9` steps `0x10 -> 0x11`, retail and swapped alike),
and an early build that wrote the sibling's victory flourish over the
lead-in swept the sibling's limbs across the battle closeup as dark
mesh streaks. Only the loop window `[+0x85, +0x86]` - the frames the
results screen parks on - carries the sibling's own post-win cycle,
retargeted alone and phase-picked in host space against the retail
lead-in's last frame. Lu contributes her `[16, 24]` sway, Gi his
`[30, 30]` hold; Che declares no window of his own, so his entries hold
on the flourish's last frame. The window bytes themselves are never written - `+0x84` is
the byte `ArtAnimRecord::uses_base_archive` keys on. Entries 4/5 - the
weak-victory actions (`0x15`/`0x16`), which the sequencer also loops -
take the idle as a whole cycle (see
[`audio.md` § Hero victory voices](../subsystems/audio.md#hero-victory-voices---the-tail-clips-of-monstersnd)
for the action/tier mechanism). Battle **voices** follow as well
(`delilas_voice`): the party's reaction grunts are SPU one-shots out of
the always-resident battle bank's programs 7/8/9, so the mapped siblings'
samples (single-program VABs inside `monster.snd`) splice over them in
place - cue tracks, routing and residency all stay retail. The replaced
characters' **XA voice lines** are silenced rather than left wrong-voiced:
the per-character arts-shout banks (`XA2`/`XA4`/`XA6`) whole; the
party's channel groups (0-3 / 4-5 / 6-9, anchored on the traced swing
channels 0/4/6) of the shared short-vocalization bank `XA30.XA` (see `docs/subsystems/battle-action.md` § Battle voice cues);
the battle-event bark bank `XA21.XA` whole (the sound-command dispatch
in `FUN_8004E568` resolves the char-keyed victory ids there. **All eight
channels are victory-bark arms**, not the subset an earlier reading
named: `FUN_8004FCC8` decodes a bark id as `a1 = id - 0x100`, channel
`a1 & 7`, file `a1 >> 3` (slots `1`/`3`/`5` remapping to
`0x1A`/`0x1B`/`0x1C`), so `0x1A0..=0x1A7` are `XA21` channels 0-7 one
for one, with `0x19F` = `XA20` channel 7 and `0x1AF` = `XA22` channel 7;
the `gp+0x9F4` switch at `0x8004FA24` reaches all ten; the
whole file is short bark reels), plus channel 7 of `XA20.XA` and of
`XA22.XA` - the jukebox's outlying arms (ids `0x19F`/`0x1AF`; the whole
`gp+0x9F4`-keyed jukebox is a special-sequence path that ordinary
victories never enter), short reels interleaved beside those files'
music channels, which stay byte-identical. `XA12.XA` stays retail - its
only captured battle fire is the non-voice jingle path (results music). And
the six staged-event voice banks whole - the voice-id space (`id >= 0x100`
through `FUN_8004FCC8`, ids picked by the anim materialiser
`FUN_8004AD80`'s inline char table: Vahn `0x101`, Noa `0x111`, Gala
`0x121`) resolves 16 ids per hero across `XA1`+`XA27` (Vahn),
`XA3`+`XA28` (Noa), `XA5`+`XA29` (Gala) - item use, Spirit, cut-ins, KO
and victory lines. Subheaders and routing survive everywhere, other speakers' channels
stay byte-identical - and the silenced hero slots are then **re-voiced
with the siblings' real grunts** (`delilas_xa_voice`): each sibling's
`monster.snd` SPU samples decode to PCM, peak-normalize, resample to
the channel's CD-XA rate and re-encode through the crate's XA-ADPCM
encoder (`legaia_xa::encode`, mono and dual-mono-stereo 4-bit; the
staged-event banks and victory barks ship stereo), written at each
channel head. Arts shouts, swing grunts, staged-event lines and
victory barks all speak with the Delilas voice. The **ordinary
victory-pose voice is none of those XA tiers**: it is an SPU sample
streamed from `monster.snd`'s own sector TOC (`FUN_8003E104`; pose
action → clip byte via SCUS `0x800788A0` / `0x80078867` - see
[`audio.md` § Hero victory voices](../subsystems/audio.md#hero-victory-voices---the-tail-clips-of-monstersnd)),
so the heroes' clip bands (Vahn `0xB8..=0xBC`, Gala `0xBD..=0xC3`, Noa
`0xC4..=0xCB`) are overwritten with the mapped siblings' grunt bodies -
verbatim SPU-ADPCM copies out of the same file, END-flagged when
truncated to the clip span.
XA3 / XA5 are not "stereo Miracle fanfares": the fires captured there are the
heroes' voice lines through the same dispatcher.

What the **Tactical Arts voice banks** carry is a separate three-way
pick, `--delilas-arts-voice MODE` (browser: the "Arts voices"
sub-option under the swap). It governs two bank families: the per-art
**shout** banks (`XA2`/`XA4`/`XA6`) and the Hyper / Super / Miracle
**fanfare** banks (`XA1`/`XA3`/`XA5` plus the Seru-magic fanfare
streams `XA27`/`XA28`/`XA29`).

The fanfare banks are the reason a Super or Hyper Art can go silent.
They are not one-shot voice lines but 3-7 second stereo cue beds
carrying the hero's voice over a jingle, and a Hyper Art fires **no**
shout from the `XA2`/`XA4`/`XA6` pool at all - the fanfare is its only
audio. Filling one with a quarter-second grunt therefore leaves the
cue silent for over 90% of its window. They follow the same three-way
contract as the shouts:

- `adjusted` - the retail hero shouts are captured before the
  mute and **re-voiced toward each mapped sibling** through the tuned
  pitch/formant map in `delilas_voice_fx` (`DEFAULT_VOICE_MAP`, tuned
  by ear per hero-sibling cell). The DSP chain is deterministic pure
  data: WSOLA time-stretch (duration preserved), resample, one
  cepstral spectral pass doing the formant warp and the timbre
  transfer toward the sibling's longest grunt, pitch-contour bend,
  and an RBJ biquad tone chain; each voiced clip is processed alone
  and laid back at its original start so arts cue timing holds. A
  fanfare channel takes a reduced cell (`fanfare_fx`: pitch and
  formant only) - the timbre, carrier and attack-graft stages assume a
  lone voice and smear a music bed - and keeps its full retail length,
  so the cue plays end to end.

  The two axes are **not** independent, and mis-reading that is what
  made the gender-crossing cells resist tuning. `pitch` is a resample,
  so it drags the whole spectral envelope with F0: the audible formant
  shift is `pitch + formant_st`, and `formant_st: 0` therefore means
  "formants dragged the full pitch" - a slowed-tape voice, deep without
  being male. A female-to-male recast wants F0 far down and the vocal
  tract only 10-15% longer, i.e. a **sum** near -2..-3, not 0.
  Measured on a synthetic vowel: `pitch` alone moves the envelope by its
  full amount (-12 -> -11.78 st), `formant_st` alone is accurate
  (+9 -> +9.00).

  Worse, past about 6 semitones of downshift the correction went inert.
  The cepstral lifter was hardwired at `SPEC_L = 48` bins of a 1024-point
  FFT, an envelope resolution of about 790 Hz at 37.8 kHz; an octave
  down the formants are 450-570 Hz apart, below that resolution, so the
  warp reads one broad blob with no peaks to move. Sweeping
  `formant_st` 0 to +12 at `pitch: -12` moved the envelope 0.16 st in
  total. `formant_pre` (warp before the resample, in the clip's native
  scale) and `env_track` (scale the lifter order by `2^(-pitch/12)`)
  each restore it independently - measured -2.93 and -2.92 against a
  -3.00 target.

  **This was a host drift, not just a bug.** The browser tuning
  dashboard applies its spectral pass *before* the `playbackRate`
  resample, so it has always behaved as `formant_pre = 1`, while the
  Rust bake applied it after. `formant_st` worked in the tab the map was
  tuned in and was inert on the patched disc, which is exactly the shape
  [`host-drift.md`](host-drift.md) warns about: a value that reads as
  tuned because the surface you tuned it on honoured it.
- `original` (default) - both bank families are left retail (never
  muted): Vahn /
  Noa / Gala call their own arts out of the siblings' bodies.
- `removed` - the banks stay silent; the spliced SPU grunts remain the
  audible attack voice.

Each hero slot also gives up one Hyper art to carry the mapped sibling's
**signature special** - Gi's *Blazing Slash*, Che's *Megaton Press*, Lu's
*Plasma Strike*, all three names read off the disc's own spell table
(actions `0x79`/`0x7A`/`0x7B`). The host is that character's 50-AP Hyper:
Vahn's Burning Flare, Noa's Vulture Blade, Gala's Explosive Fist. They are
the only three that clear every gate at once - the replacement combo must
be the same **length** as the one it replaces, which rules out every 3- and
4-input Hyper; Noa's Hurricane Kick carries its combo on three bank records
and shares its stream with a Super Art; and the remaining candidates share
a combo **string** across characters (`0x80014198` is both Vahn's Tornado
Flame and Gala's Thunder Punch, so rewriting one rewrites the other's menu
glyphs). The new combo `L R L R D` is free on all three.

Four coordinated edits per slot (`delilas_party::reskin_signature_art`):

- **Name** - written through the arts-table record's own `+0xC` pointer
  into its NUL-padded field (`arts_table::name_field`), never by searching
  the image for the old text. The names nest: searching for `Hurricane`
  finds the `Hurricane Kick` containing it, so a text-driven rename is one
  table row from corrupting a neighbour.
- **Combo** - written to both copies retail keeps in sync, the SCUS display
  glyphs and the player-file `record0` matcher, after a collision check
  against every one of that character's arts.
- **Animation** - the sibling's own choreography retargeted onto the player
  rig into the host art's "ME" stream, addressed by monster-archive **entry
  index**, not action tag: Gi's and Che's signature clips are both tagged
  `0x23`, so no tag band reaches them.

  A signature move is a **chain of stages**, not one clip. The enemy-side
  modules stage several entries in sequence (`delilas_dome` records Lu's
  action `0x7B` as `14 -> 12 -> 13` and Che's `0x7A` as `10 -> 11`), so
  shipping only the final stage shows the payoff swing with no wind-up -
  Megaton Press without its lift, Blazing Slash missing two of its three
  beats. The stages are retargeted individually and concatenated: Lu 90
  frames, Gi 75, Che 100, against host streams of 21/58/20. Gi's chain
  `10 -> 11 -> 12` is the one no static evidence pinned; it was inferred
  from clip shape and later corroborated by a player independently
  reporting three distinct beats in the move.

  Stages do not share a rate (Gi's first is authored at 1, the rest at 2),
  and a concatenated stream has only one. Each stage is therefore
  resampled to hold its authored duration at the chain's fastest rate -
  `frames_i * R / rate_i`, since a clip runs `frames * 8 / rate` ticks -
  so a slower stage stretches rather than any stage being decimated.

  When the slot cannot hold the whole chain, the next rung is a **coarser
  keyframe density, not a lost stage**: halving the rate and every stage's
  frame count together leaves `frames * 8 / rate` unchanged, so the move
  plays for exactly as long on half the poses - and several of these
  stages are authored at rate 1 to begin with, so the coarser stream is a
  density retail itself ships. Only exact divisors are tried, so no stage
  is silently re-timed relative to its neighbours. Lu's chain needs this
  rung whenever she lands in Noa's slot, whose rig has 16 parts to
  everyone else's 15: at full density her three stages do not fit and the
  wind-up would be dropped, while at half they fit at the same duration.
  Dropping stages from the **front** (a move that loses its wind-up still
  reads; one that loses its strike does not) is the rung below that, and
  the retail frame count with `winpose::retimed_rate` the last.
- **Hit timing** - the art's hit events (entry `+0x10..0x13`) are frame
  indices into the stream that just changed under them, and a proportional
  rescale across the whole chain is the wrong correction. The host's hits
  were spaced against a single swing, so spreading them over wind-up plus
  payoff drops most of them into the wind-up: Burning Flare's four hits at
  frames 11-14 of its own 21-frame clip land at 42-53 of a 75-frame chain
  whose strike does not begin until frame 52, and the damage fires while
  the character is still winding up.

  Anchoring them on the payoff stage's **start** is the wrong correction
  too, and it is the one that shipped first: the payoff stage opens with
  its own approach, so the first application landed 1.1-3.0 seconds after
  the body connected in all nine sibling/host pairings. The anchor has to
  be the connect itself.

  `delilas_party::chain_contacts` derives that connect set, per stage, in
  the rebuilt stream's coordinates. A stage in the damaging tag band
  (`0x0C..=0x1F`) carries its own contact beats on disc and those are
  authority - Lu's three strike stages are authored, and they need not be
  deceleration frames because a lunge connects at speed. A stage retail
  gave no beats to has only its motion to go on: Gi's and Che's signature
  stages are tag `0x23`, damaged by their PROT 958 / 959 cast modules
  rather than by beats, so their connect is **measured** as the frame the
  whole body arrests hardest (the largest single-frame fall in the mean
  per-part translation delta). A measured stage under 40% of the chain's
  hardest stop is a wind-up settling, not a connect, and is dropped.
  Measured connects are good to about a frame; authored ones are exact.

  The hits then sample that set: with more connects than hits, evenly with
  both endpoints included, so the first application lands on the first
  connect and the last (biggest) power byte on the finisher; with fewer,
  the extras ride the same impact on consecutive frames - retail's own
  multi-hit idiom (Burning Flare is `11 12 13 14` on one swing). The
  **effect-script gates take the same set**, so the burst, the damage and
  the body all run off one clock; rescaling the donor cast's own beats
  (which is what shipped) puts the burst on a third timeline entirely.

  The number of non-zero slots is always the host's. `entry[+0x00 + i]`
  (power) and `entry[+0x10 + i]` (frame) are parallel arrays walked by one
  cursor (`actor[+0x1F4]`), so moving the count would re-pair the power
  bytes, and a zero slot ends the walk outright (`0x801EC47C`) - nothing
  here may write `0`. Two things make editing these bytes safe: the damage
  gate tests `0x0C <= entry[+0x00] <= 0x1F` on the **host art's** power
  byte 0 (`0x1D` / `0x17` / `0x18`, untouched by the swap), and the
  mid-clip early-commit at `0x80047918` needs `entry[+0x76] == 0` while all
  three host arts read `1`, so moving a hit earlier cannot truncate the
  clip.

  The firing rule itself (`FUN_801EC3E4`, `0x801ec440`-`0x801ec480`): a hit
  fires on the first tick where `frame >= hit_frame - 1`, `frame` being
  `node[+0x68] >> 4`, the integer keyframe index of the stream playing. So
  landing damage on visual contact `C` means writing `C + 1`, and `rate`
  only sets wall-clock - one keyframe is `8 / rate` ticks - so no units
  mismatch exists between the two.
- **Effects** - the host's script is eight `[frame_gate, effect_id, x, y,
  z]` records, and for Burning Flare all eight spawn flame `0x96` across
  the swing. That flame is why a reskinned art keeps reading as the host's
  move however faithful the body gets, so it is replaced. The staged
  special clips carry no script of their own - as an **enemy**, a sibling's
  signature move draws its visuals from a per-spell code module (PROT 960
  for Lu's), which spawns from module-resident parameter blocks that a
  one-byte art id cannot name. What the siblings do have is their ordinary
  casts, whose entries carry real scripts in exactly this format, so the
  spawn comes from there (direct-form `0x84`, unless the slot claimed the
  transplant cave). With nothing to borrow the host's script is suppressed
  instead, by deferring every gate to `0xFF` - the walker never advances
  past a gate it has not reached, so nothing spawns and no terminator arm
  runs.

  The **gates** come from the same connect set the hit list is scheduled
  on, not from the donor's own beats, so one clock drives the burst, the
  damage and the body. The walker's gate rule is the hit rule
  (`frame + 1 >= gate`, `0x801decb4`), so a gate equal to a hit value fires
  on the same frame.

  Rewriting the script is **not sufficient on its own**, which is the trap
  here: entry `+0x7A` carries an impact-effect class that two further
  renderers read straight off the actor, neither of them through the cue
  records. `FUN_8004998c` streams an element spark along the swing path
  and `FUN_80049348` draws afterimage copies tinted from a per-*character*
  table - so a slot whose host art sets that byte keeps showing the host's
  element however completely the script is replaced. Of the three hosts
  only Vahn's Burning Flare sets it, which is why a sibling in Vahn's slot
  wore his fire through every other edit the swap makes. It is zeroed per
  slot. Re-pointing it at the sibling's own element instead is tempting -
  class `2` is the lightning-class spark, which is Lu's - but the same
  byte switches the afterimages on and those take their colour from the
  character, not the art, so it would trade the host's sparks for the
  host's ghosts. Removing what is wrong is measured; adding what is right
  needs the colour word's channel order settled first (see
  [art-data.md](../formats/art-data.md#impact-effect-class-entry-0x7a)).

The rename has an **enemy half**, and it is what makes the Nivora duel
read correctly. The mod already reskins each sibling's monster block with
the mapped hero's model and name, so that fight already puts Vahn, Noa and
Gala on the enemy side - but the cast it announces came from the spell
table, which is the same table the party path uses. `FUN_801E9FD4`'s
`0xA2`/`0xA3`/`0xA4` arms fire on the round counter (`% 3 == 2`) and write
`actor[+0x1DF] = monster_id - 0x29` (the subtraction is a literal at file
`0x1CFFC` of the raw battle overlay, `0x2442FFD7`), so Gi's `162` resolves
to spell `0x79`, Che's to `0x7A` and Lu's to `0x7B`. Left alone, the enemy
wearing Vahn's model announces *Blazing Slash*. Pointing the sibling's row
at the host art's retail name completes an exchange rather than a
one-way rename: the party art gives up `Burning Flare` to become
`Blazing Slash`, and the enemy row gives up `Blazing Slash` to become
`Burning Flare`. Written through the record's own `+8` pointer into
padding measured by `spell_names::name_field`, never grown - the retail
slots hold 16 bytes against 13-14 byte names.

The mirror's two remaining halves are closed by their own passes.

The Delilas **field forms** in `nilboa` are scene-resident: MAN placements
carry `model = 106/107/108`, pack-member indices into PROT entry `0639`.
The scene mirror (`legaia_patcher::party_swap::nivora_field::heroize_nilboa`
via `legaia_patcher::nivora_field`) rebuilds those three members as the
mapped heroes' field rigs, pivot-baked onto the siblings' scene-idle rest
frames so the scene's own ANM records pose them, and repaints the three
sibling head TIMs in PROT `0638` with the heroes' faces. It fits at full
geometric detail - dropping the two unposed equipment-template groups per
rig and flattening non-head textured prims to the retail field flat-shade
style brings the three rigs to 29200 bytes against the members' 30276 -
with non-face head-texture islands packed at half resolution (face fronts
stay full). The retail hero source is the **pre-fieldize** PROT 0874
capture: by the time this pass runs, 0874 itself carries the siblings.
Verified by `nivora_field_real`.

The other three story appearances mirror through the same bake driven by
a per-scene coordinate table (`party_swap::event_field::EVENT_SCENES` via
`legaia_patcher::nivora_field::apply_event_field`): the map-stone
confrontation in `stone` (bundle `0175`, members 33/34/35 anchored on ANM
records 37/52/62), Zora's floating castle in `taiku2` (bundle `0426` -
four per-beat shading copies per sibling, members 116..127, all four
carrying one bake anchored on the placement records 14/20/26), and past
Conkram in `conc2` (bundle `0624`, court-outfit meshes 165/166/167 on
records 64/66/68, head TIMs in the sibling `tim_pack` entry `0625`).
Every coordinate is the scene's own MAN actor placement (`model_index` =
TMD-section member, `anim_id - 1` = anchor record). These scenes keep
their NPC meshes inside the bundle's LZS TMD section, so the rebuild
reflows the member pack in the decoded section and recompresses -
in-place when the stream fits its retail span, else a whole-bundle
section re-lay inside the entry footprint. One trap is pinned in the
spec: `taiku2`'s kneeling anchor stances fool the geometric limb
splitter into a clean-looking but wrong role pairing (the torso lands on
a leg bone), so its slots carry the assignment measured off the
byte-identical rigs' neutral stance in `stone`. Verified per scene by
`delilas_event_field_real`.

The enemy special's **body motion** no longer reads as the sibling's
move: the enemy-side anim mirror (`party_swap::enemy_anim` via
`legaia_patcher::enemy_anim_mirror`) rewrites each swapped block's
archive entries with the hero's own clips - idle, walk, reactions and
swings, plus the hero's 50-AP Hyper split wind-up-to-strike across the
entries the cast module stages by raw index (and the hero's victory
flourish in the tag-`0x22` close). Streams are the raw 9-byte packed
family and re-encode byte-exactly; frame-indexed head fields rescale,
sound cues / AGL / tags / root motion stay retail. Per-entry frame
floors respect the one measured module gate (PROT 0960's damage tick
waits for the caster's clip cursor to reach keyframe 22, so Lu's payoff
entry holds >= 23 frames; everything else floors at retail's own
smallest staged entry, 11). A budget ladder (exact keyframe-density
halving, a compact close, then family drops) covers tight blocks; all
six mapping permutations fit with no ladder action, and a block that
missed every rung would keep the sibling's clips with a note rather
than fail the apply. The fire, lift and camera stay the module's own
hardcoded `jal` sites (15 in PROT `0958`, 41 in `0959`, 24 in `0960`) -
on the enemy side that is retail behaviour and stays. Verified by
`enemy_anim_mirror_real` (bake-parity affine-fit bounds included: the
enemy-side model bake now runs the same whole-rig alignment as the
player side, closing the per-part roll defects the old per-joint re-aim
left in every hero-under-sibling-clip pose).

The **battle idle** is rebuilt too, and it is a stance change more than a
motion one. Retail authors each character's combat stance in idle frame 0,
and the siblings' stances suit their own proportions - Che stands wider
than Gala's staggered guard - so a swapped character holding the host's
stance reads as the wrong body wearing the right model.

The player idle is raw packed (`2 + frames * parts * 9`) **inline in
`record0`**, not channel-delta like the readef "ME" bodies, so it is
rewritten at its exact retail length: keeping the frame and part counts
keeps every later offset in the block valid and needs no relocation. Two
consequences follow from that budget. The sibling's cycle is resampled to
the host's 8-9 frames from its own 13-35, and the rate byte floors at `1`
where all three hosts already sit, so the rebuilt idle cycles 1.3x to 1.9x
its authored speed and cannot be slowed back down. And the stream is
**re-anchored to the host's rest**: the retarget rebuilds translations by
forward kinematics, so its frame 0 does not land where the host's does,
while every clip the swap does not rebuild - walk, flinch, block, get-up -
still starts from the host's rest, and an un-anchored idle pops the whole
body on each transition into and out of it.

The re-anchor is **one rigid whole-body translation**
(`winpose::idle_anchor`), added to every part of every frame. Battle poses
are flat - each part carries an absolute `R * v + T` about the object
origin and nothing in the stream hangs one part off another - so a
translation written to the torso channel alone does not re-seat the
character at all: it shears the torso off the body. That is a defect this
feature shipped with, and it read in game as the torso floating clear of
the model; measured, it opened a 21.6 to 89.8 unit gap at every torso and
pelvis joint of all nine sibling/host pairs, against the 1.8 to 2.4 units
retail's own idles carry. Oracle:
`crates/patcher/tests/party_swap_idle_continuity_real.rs`.

The translation's two axes come from different references, because they
answer different questions. **x / z from the torso** - the FK root the
retarget hangs the skeleton off, and the part the actor's world position
and the attack camera frame. Anchoring the support point (the ankles'
midpoint) instead is the physically tidier reading but costs more: the
siblings plant their feet 16-101 units from where the host plants theirs,
so foot-anchoring slides the visible body that far off its mark. **y from
the floor** - the character has to stand on the ground, and the torso's
height above it is exactly the part of the stance worth keeping, so
taking y from the torso as well pins the wrong end of the leg (measured,
it floated Lu 46-50 units clear of the floor on both her hosts and sank
Che 48 into it on Noa's). The floor is the deepest ankle pivot over the
whole cycle on both sides, so a lifted foot in either frame 0 cannot bias
it and no frame plants deeper than the host's own idle does.

Hand orientation in every retargeted sibling clip follows a
**wrist-attitude law** rather than the conjugation. The retarget maps
each part's absolute orientation independently, so a played pose
preserves neither rig's hand-to-forearm relation - measured 85 degrees
off retail on the weapon hand in Gi's idle, which pointed the welded
equipped-weapon blade (correct under every host art clip, since those
keep retail's own wrist relation) straight across the torso, reading in
game as stray green texture on the chest and back. Hands carry no
authored detail worth keeping - unlike the head, whose bob is
deliberately carried across - so `retarget_clip` slaves each hand's
rotation to its forearm at the host's rest wrist attitude, the same
terminal law the feet get from `normalize_battle_rest_feet`. Hand
rotations feed no FK translation term, so the override changes
orientation only, about the wrist pivot the fist is anchored at.

The idle rides the **same** `record0` write as the signature-art edits,
because it is the only one of them that adds bytes and batching means one
LZS re-fit rather than two that must each clear the footprint alone. That
write is also where `patch_player_record0_full`'s `None` return stops
being safe to ignore: it means "nothing changed" and "did not fit"
equally, so an overflow silently drops every edit in the batch - leaving
the art displaying its new combo in the menu while still answering to the
old one in battle. It now falls back to `legaia_lzs::compress_optimal`
before giving up, and the caller treats a `None` it cannot explain as a
hard error.

The **run-in** stays the host's: it is a separate queue action (constant
`0x19`, `Starter`) shared by every one of that character's arts, so
retargeting it would change all of them.

The **swing camera** needs re-timing, not replacing, and that is the
correction to make here: the earlier reading treated the camera as a
choice among arms, and the arm was never the problem.

`FUN_801D71B8` dispatches per (character, art constant) through three
per-character jump tables (`0x801CEA88` / `0x801CEAD0` / `0x801CEB20`,
file `0x0270` / `0x02B8` / `0x0308` of the raw battle overlay), slot =
`(constant - 0x1A) * 4`. Thirteen distinct arms exist and **no live arm is
shared across characters**; 37 dead slots point at a bare return. The
dispatcher's prologue admits only party seats `0..2` and only action
category 3 (Attack), so no enemy cast and no Super-Art expansion can reach
a slot at all (every Super finisher constant falls outside its table's
bound; Super Arts get no attack camera in retail).

Each arm is a cascade of `slti` tests on the animation cursor
`actor[+0x22C][+0x68]` - sixteenths of a keyframe - and that cascade is
how a swing gets several framings instead of one. Gala's Explosive Fist
arm changes shot at keyframes 4, 7 and 10; Noa's Vulture Blade arm and
Vahn's Tornado Flame arm at 14. **Those thresholds are literals sized for
the retail clip** - the highest threshold anywhere in the dispatcher is
keyframe 17 - and a signature chain runs 46 to 100 frames,
so the camera finishes its entire choreography inside the wind-up and then
holds one shot for the rest of the move. That is the whole defect, and it
is invisible to any check that only asks which arm ran.

`delilas_party::retime_camera_arm` scales each threshold so the **last**
shot change lands on the frame the payoff stage begins, with the earlier
ones scaled by the same factor to keep their spacing. Anchoring on the
payoff beats scaling by the raw length ratio: the final framing is the one
that films the strike, so it should start when the strike does, and a
chain can come out the same length as its host while still opening with a
wind-up the retail thresholds know nothing about - Lu's stream is 58
frames either way, so a length ratio of 1 would leave her final shot
sitting in the wind-up.

The immediates are found by shape rather than by address: a `slti` (the
dispatcher's own bounds checks are `sltiu`, a different opcode) against a
register some `lh`/`lhu` loaded from `+0x68`, with a threshold in the
keyframe range. Linear liveness analysis would be **wrong** here - the
arms are branch cascades, and the path that reaches a later test jumps
over the block that reuses the register, so a straight-line read says the
register is dead where it is live.

Editing an arm is only safe while exactly one art dispatches to it, and
that is checked rather than assumed. Burning Flare's own arm `0x801D7650`
reads **no cursor** at all - one static framing for the entire swing, and
so nothing to re-time. Four of the thirteen arms are flat like that, but
it is the only flat one a host art dispatches to. Its slot instead
**swaps** with Tornado Flame's `0x801D74A8` (two cursor bands, three ramp
folds, no side effects). A swap, not a
retarget: every arm is already live somewhere, so a plain retarget would
alias an arm a second art still uses and the re-time would follow the
alias into that art. Exchanging leaves the set of live arms unchanged -
only which art dispatches to which - and leaves the borrowed arm reachable
from one slot, which is what makes it re-timable. Noa's and Gala's arms
are each already theirs alone and are re-timed in place.

Borrowing an arm *across* characters is possible but strictly dominated,
and the reason is worth recording: both per-character adjustments are
applied in the dispatch **preamble**, before the arm runs, so a borrowed
arm cannot compensate for them. Character 3 seeds camera `TR.y = 0x600`
against `0x400` for characters 1-2, and `ctx+0x26D` (the table column) is
forced to 0 for character 3 while everyone else gets `rand() % 2`. Gala's
Lightning Storm arm on Vahn therefore flips columns per turn - measured
row deltas include sign flips, so alternating turns frame from opposite
sides - and Gala's Explosive Fist arm on Vahn sits 512 units low in the
two bands that do not write `TR.y` themselves.

**Slow motion is already there** and needs no change: retail halves
`actor+0x21D` on every party Tactical Art strike, and a SpecialStarter
adds a freeze-frame plus quarter speed, both from `FUN_8004AD80` and both
gated party-only.

### Each slot fights in its sibling's element

The swap also moves the slot's **element**
(`delilas_party::retarget_character_elements`). The battle overlay's
per-character element table (`0x801F5480`, PROT 0898 file `0x26C68`, one
byte per 1-based character id) is the only per-character element the disc
carries, and retail seeds it Vahn = fire, Noa = wind, Gala = thunder. Both
affinity readers index it with `DAT_8007BD10[actor] - 1` and use the result
as a row/column of the matrix at `0x801F53E8` - `FUN_801DD864`
(`0x801dd8ac`, `0x801dd900`) and the hit kernel `FUN_801EC3E4`
(`0x801ecf38` attacker, `0x801ecf94` defender) - so the byte decides what
element every one of that slot's attacks deals and what it takes.

Each sibling's own element is already on the disc at monster record `+0x1D`
(the byte `FUN_801EC3E4` reads at `0x801ecf68` for an enemy attacker):
**Gi = fire, Che = earth, Lu = thunder**. The pass copies it into the
slot's table row, taken from the archive image captured before the model
loop so a re-skinned block cannot feed itself back. Nothing else moves -
the affinity matrix and the five characters outside the party keep retail
values.

Until this ran, a swapped party fought in the host's element however
faithful the rest of the identity swap got: Lu's Plasma Strike landed as
**fire** out of Vahn's slot and Che's Megaton Press as **thunder** out of
Gala's. That is per character, not per art - retail has no per-art element,
so a Ra-Seru cast and a basic swing scale through the same byte.

Two earlier readings of that report were wrong and are worth keeping: the
art record's 8-record effect script (`+0x14`) and the impact-effect class
(`+0x7A`) both looked like the culprit and neither is. The effect script is
genuinely live - a mid-battle RAM capture shows the art bank the runtime
walks is a verbatim image of the player file's decoded `record[0]` (see
[the bank the runtime walks](#the-bank-the-runtime-walks)) - and `+0x7A`
writes the **target's** hit-flash (`FUN_801EC3E4` `0x801ee3d4` stores it at
`actor[+0x21F]` of `attacker[+0x1DD]`), not the attacker's move colour.

### The bank the runtime walks

The art-record edits the reskin makes reach the runtime through one chain,
stated here because it is easy to doubt:

```text
DAT_801C9360[char]  ->  the decoded record[0] image
record0[+0x58]      ->  art bank (u32 count, then 0xD0-stride records)
bank + 4 + row*0xD0 ->  the bank record
record + 0x24       ->  the action entry
```

`FUN_8004AD80` materialises a staged anim id `q >= 0x10` by writing
`bank + 4 + (q-0x10)*0xD0 + 0x24` into `record0[q*4]` (`0x8004b708`
loads `record0[+0x58]`, `0x8004bc84` stores the entry pointer), and
`FUN_80047430` hands that pointer to `FUN_801DEA50` as `node[+0x4C]`
(`0x800478b8`). Measured against the mednafen `party_battle_gobu_gobu`
capture: Vahn's 33-row and Gala's 32-row live banks are **byte-identical**
to this crate's decode of the disc `record[0]`, and Noa's differs in 4
bytes of 7284 - all four inside row 0's entry `+0x04..+0x07`, a field the
tick writes and the patcher never touches. So a same-size edit inside a
bank record is an edit the runtime sees.

### Casting a sibling signature attack from a party slot

The genuine enemy-side move - Lu's Plasma Strike as PROT 960 drives it -
is **reachable but deliberately not shipped**.

What is *not* the blocker: "a party actor has no monster block". A party
actor has a first-class equivalent anim table (`DAT_801C9360[slot]` against
a monster seat's `DAT_801C9348[slot-3]+0x4C`, the two arms of
`FUN_8004AD80`), the raw indices the module stages resolve on it, and the
module's single monster-block access is a hardcoded **seat-0** write to
one field of one clip, unrelated to who is casting.

What actually blocks it, in order of severity:

- **Kernel-RAM corruption, on every cast.** That seat-0 access reads word
  32 of monster seat 0's block. The loader fixes up only `magic_count`
  words at `+0x4C`; past that the words are unfixed block-relative
  offsets, so a seat-0 monster with `magic_count <= 13` - Che Delilas
  himself has 12 - turns the pointer into a bare offset and the following
  store lands a halfword in PSX kernel RAM. Retail never trips it because
  the module is only ever reached with Lu (16 entries) in seat 0.
- **A softlock with no escape.** Battle state `0x70` re-enters the module
  every frame and advances only when it returns 0; there is no timer and
  no bail-out. Four phase gates can stall it, and the last needs a clip of
  at least 23 keyframes - which Gala's party index `0x0D` fails today at
  17 of 19 equippable section-2 ids.
- **Friendly fire.** The damage call and both HP writes are hardcoded to
  actor slot 0, not the chosen target.
- **The choreography is not free either.** The staged raw indices mean
  "the four basic swings" on a party actor, so getting the sibling's clip
  there needs a detour in the hottest per-actor path in the battle loop.

The cheap part of what the module provides is separable and is what the
swap ships instead: the camera above, and the effect script.

The effects go further than borrowing, via a **transplant**
(`delilas_effects`). A cast module's part prototypes are ordinary
move-VM records in the same format the `0x801F6324` prototype table
holds, they contain **no absolute pointers** (measured over each
module's whole data region), and retail ids 50-60 - the Super Arts'
own bursts - are the identical shape (`model_sel = -1`, submode 2, the
same opcode set) and are already reached from a player art's effect
script. So this is the Super Arts' mechanism pointed at different data,
not a new one: copy the record into a spare prototype slot and the
art's one-byte effect id names it.

Six ids are unreferenced, and the argument is structural rather than a
byte scan. `0x801F6324` is materialised at six `lui` sites, all in PROT
0898, and every index is a byte lifted straight out of data with **no
computed index anywhere** - so censusing the carriers closes the set:
1811 monster action entries over 186 blocks, 286 player action entries,
all 44 move-power records, all 13 cue groups. Of the six, only `37`,
`38` and `47` own their record outright; the rest have a free table slot
aliasing a live record.

**The cave is 88 bytes and that is the entire budget** - the battle
overlay is packed to the byte. Records 37+38 are contiguous and bounded
by live id 39; every other inter-record slack in the prototype region is
2 bytes, and the 530 bytes that look free after id 44 are two burst-arm
triggers, their 130-byte stager records and three further live records.
So exactly one sibling gets the transplant - the first hero slot claims
it - and the other two keep the borrowed cast projectile. Records using
op `0x20` (an unidentified `gp[0x714]` hook) or naming a `DAT_8007C018`
mesh are rejected outright, so the worst case for a picked record is a
differently-coloured burst, never a missing resource.

Safety: the id can only draw a different retail effect (nothing else
reads 37/38, and 38 is parked on id 0's record so the cave's middle is
never decoded as a header); a clobbered cave ends at the move VM's own
`>= 0x47` bound check; and the stager takes no pointer out of record
data. One knob left for a listener: `0x801F6418[37]` is sound cue 208,
so the transplant fires that cue - a one-byte edit if it is wrong for
the move.

### The Delilas move set

`--delilas-moves` (browser: the "Move set" dropdown under the party
picker) picks how much of the hero's Tactical Arts kit becomes the
sibling's. `hybrid` is the default and is exactly the behaviour above:
every art keeps the animation retail authored for it, and only the one
reskinned Hyper plays a Delilas motion. `delilas` re-authors the rest of
the kit (`delilas_party::apply_delilas_moveset` +
`legaia_patcher::party_swap::moveset`).

**The archive is rebuilt, not extended.** A character's art streams live
in one `0x10800`-byte `readef.DAT` slot, and retail fills most of it: the
three main slots have 20374 / 2446 / 17361 bytes free. Noa's cannot take
even one more full-length clip. It does not have to: retail already
points several art records at one stream (Vahn's 25 records resolve to at
most 17), so the record's `+0x0A` stream index is a free-standing pointer
and the whole archive can be re-emitted as long as every record that
reads it is repointed in the same pass. What ships is the signature
stream carried over byte-identical, the sibling's locomotion clip, and
one entry per distinct sibling swing, against 17 / 18 / 19 retail
streams. On the default `gi,lu,che` mapping that is 5 / 6 / 6 streams in
16511 / 16133 / 26565 bytes; the counts follow the sibling, not the
slot, so a rearranged party moves them.

A swing is an archive entry whose action tag falls in the `0x0C..=0x1F`
band and that no stage of the signature chain claims; that yields 3 (Gi)
/ 4 (Che) / 4 (Lu) without a per-sibling table. The tag alone is not
enough - Lu carries an unstaged `0x23` - which is why the chain is
subtracted rather than the specials being inferred from tags.

Every record that reads the archive is then repointed round-robin over
the swings and re-timed to the clip's own rate; the two combo-starter
records take the locomotion clip. Each record's frame-indexed fields -
the hit list at entry `+0x10..0x13` and the eight effect-script gates -
are rescaled from the stream it used to read onto the one it reads now,
and the host's impact-effect class (`+0x7A`) and mid-clip loop hold
(`+0x84..0x86`) are cleared, both being keyed to choreography that no
longer exists.

**The renames are load-bearing, not decoration.** Every record's inline
name becomes the label of the clip it now plays, and a handful of
repeated strings compresses far better than 22 distinct ones - which is
what keeps the rewritten `record[0]` inside its LZS footprint. Spare
bytes with the whole pass applied, on the default mapping: Vahn 143, Noa
272, Gala 56.

The menu side follows through each arts-table record's own `+0xC`
pointer, over the retail string plus its measured NUL padding. Labels are
capped at seven bytes for **every** sibling, not sized against the slot
each usually lands in: the tightest field any retained art carries is
Vahn's `Cyclone`, the mapping is a free permutation, and a label that
does not fit is skipped - so a per-sibling cap would silently keep the
retail name under a rearranged party. The apply says so when it happens
rather than passing in silence.

#### What survives, and why hiding the rest is free

An art is only listed once `FUN_801EFBFC` has inserted it at char record
`+0x185` on a successful performance, so an art that can never be
performed never appears. What makes a blanked combo unperformable is the
`combo_len == 1` guard at `0x801EF424`: a blanked combo is
zero-terminated at byte 0, so a match can only ever complete at length 1,
and that length is abandoned outright. This is retail's own mechanism for
the same job - the Super and Miracle **finisher** rows all carry a
single-`D` combo and are unreachable for exactly this reason. The
`token - 0x0B` compare at `0x801EF3EC` is a second line of defence
(`0x0B` is `BlockAnim`, not an input), but it was not proved exhaustively
over every queue writer and the conclusion does not rest on it.

Four groups keep a working combo:

- **the signature host**, which now carries the sibling's special;
- **bank row 11**, the Miracle Art. Its combo is the only route to the
  wholesale queue overwrite: `FUN_801EED1C` branches to the replacement
  table at `0x801F64F4` only while its rows-visited counter is still zero
  (`0x801EF4D8`-`0x801EF4E0`), i.e. only on that first row, and the disc
  confirms it - row 11 carries `RDLULURDL` / `LURDULUDR` / `RRDUDUDLL`,
  the three combos the SCUS arts table flags as Miracle;
- **every art a Super Art trigger names.** A Super is not entered as a
  combo. `FUN_801EF9E4` walks the *finished* action queue at
  `actor[+0x1DF]` and tail-matches it against the resident trigger table
  (`find` `0x801F6524 + char*0x41 + row*13`, `replace` `0x801F65E8 +
  char*0x50 + row*0x10`), and the only writer that puts an art constant
  into that queue is the combo matcher. So a blanked component silently
  costs the Super. Bank row and queue constant differ by `0x10`
  (`0x801EF63C` writes `row + 0x10`), which turns each trigger's art
  sequence into a row set: 8 rows for Vahn, 10 for Noa, 8 for Gala;
- **every art at or below the innate cap** at `0x801F686C` (`[3, 5, 3]`
  on the USA disc - each character's Hyper block). `FUN_801EFBFC` only
  self-teaches ids *above* that cap, so those arts arrive through the
  script grant instead (the `+0x74E` insert at `0x80041FB4` in SCUS
  `FUN_800402F4`). Blanking one would leave an art listed that can never
  fire, so they are kept and re-animated like the rest.

That leaves 12 / 16 / 12 performable arts per character and 3 / 1 / 3
hidden. The residual: the script grant is data-driven and its operand set
has not been enumerated, so a scene that grants an id *above* the cap
would list a blanked art. No retail grant of that shape is known.

The Miracle Art itself needs no component art kept. Its replacement
string is written into the queue verbatim, so the arts it names only have
to exist as records - which they do, untouched.

**Save metadata follows the mapping.** The save-select face and the PSX
memory-card block icon for the three hero slots come off the save-slot
portrait sheet ([`save-icon.md`](../formats/save-icon.md) - tiles 0..2 by
party id / card slot), and the boot load screen keeps its own standalone
copies of those three tiles; the swap exchanges each hero tile with the
mapped sibling's own portrait tile (Che 11, Gi 12, Lu 13 in the same
sheet, its character order), byte-exact, on all surfaces - the party's
saves show the siblings, and the card slots whose icons were the
siblings' faces now show the heroes. Existing card saves keep the icon they were
written with; `save-tool rename` covers the names on an existing card.

**Dialog follows the swap.** Every line that names a sibling (the
speaker prefixes and self-introductions in the ravine, map-stone,
Floating Castle and past-Conkram events) is rewritten through the
translation machinery to name the hero who took that sibling's place -
"Delilas" itself stays, so "Gi Delilas:" reads e.g. "Noa Delilas:" and
the world reads as Vahn, Noa and Gala *being* the Delilas family.
Word-boundary matches only. Hero names run longer than sibling names, so
a line that overflows its fixed segment budget goes through a
least-destructive fit ladder: drop the " Delilas" surname one occurrence
at a time (speaker prefix first), then contract "I am " to "I'm ",
taking the first candidate that fits; nothing needed the whole-sector
MAN relayout in either direction of the default mapping.

Still retail: menu
portraits, battle HUD faces. Composes with
`--delilas-challenge` - the challenge applies first, so its memory-tight
dome 1v2 streams slim clones cut from the retail sibling blocks while the
1v1 ravine duels (ample heap headroom) carry the swapped models. Not part
of any preset; unaffected by the seed.

> Verified by the `delilas_party_real` disc oracles (apply / re-decode /
> idempotence / determinism / mapping rearrangement, plus a hybrid-mode
> contrast against retail that pins every coordinate the Delilas pass
> owns) and the `party_swap_real` conversion oracles over all nine
> pairings.

Any produced rom can be classified after the fact with `legaia-patcher
delilas-audit --input patched.bin --baseline retail.bin` - a static
battery (no emulator) over the three rebuilt player files, four checks
per slot. Stream census: every base-archive lead-in frame the battle can
reach mid-fight must be byte-retail (the loop window and ME streams may
differ). Pose battery: FK arm-closure and extent metrics over every
battle and art clip, banded against the baseline's own numbers. Hand
radius: a textured hand whose radius blows past twice the baseline is
the welded-weapon class. Equip invariance: every record of an equipment
section must carry the identical texture pool as the section default, so
equipping any item stays a VRAM no-op - a record still holding its
retail pool would stomp the sibling's body texels at equip time, the
class a bare-handed test run never sees (this check also flags a
half-applied or version-mixed swap). The e2e harness runs the audit as
its static stage; a rom from an older build fails loudly instead of
shipping its era's defects silently.

### The retail cast route

A Delilas signature is, in retail, a capture-class spell whose cast pages a
per-spell module (PROT 958/959/960) into the battle's side overlay window and
runs the whole boss choreography - camera track, summoning pillar, blackout
lift, multi-hit damage build-up - from data, keyed only off the spell id. The
swap uses that: for a slot whose module has passed the player-caster audit,
performing the signature art routes the finished arts queue into the real
cast (`delilas_cast` module) instead of playing the art-side reskin. Four
defect classes separate an enemy cast from a player cast. Three are small
expect-verified word edits inside the module: the hardcoded party-seat-0
damage/HP sites (retargeted to the derived victim), the dead-victim
party-wipe arm (a boss cast's victim is a hero, so a dead victim meant game
over), and a finale teardown that leaves a model-less effect entity in a
carrier's draw table (its stream words are neutralised at the settle tail -
the kill-marked corpse otherwise routes the TMD walk's colour read to
unmapped memory and hard-freezes the exact frame the choreography ends).
958 alone needs a fifth: its finale arm opens with a reaction-row wait
(`beq playing(+0x1D9), reaction(+0x1F1)` - before the HP fork, where 959
forks on HP immediately and 960 waits on a countdown). A hero victim
leaves the reaction row when the battle SM stages its KO, so retail never
blocks; a monster corpse is re-staged by nothing until the action ends -
which that very wait gates - so a kill at Blazing Slash's finale
deadlocked (savestate-pinned: `mph 0x18`, victim parked in row `== +0x1F1`
with the clip long finished, caster holding a full `0xFF` park). The fix
gates the wait on the victim being alive: the arm's free `nop` loads HP
into a dead register, the wait branch retargets a 4-word cave in the wipe
body (alive -> the retail wait, dead -> the phase-advance convergence),
and the end-of-action liveness sweep (state `0x5A`) runs the real death.
960 needs a sixth, on the audio side, in two parts. First the fire
itself: the phase-0 opener fires the 16.9 s cast bed through the jingle
wrapper `FUN_8004FCC8`, which DROPS a cue outright whenever the XA
system is busy (`jal FUN_8003DE7C(1)`; no deferral queue exists) - the
opener's `jal` is rerouted through a 5-word preempt cave split across
the SCUS pools that calls the guard-free player `FUN_8003D53C` (slot
`0x13`, channel `2`, dur `0x3F6`) directly, whose own head stops any
active stream, so the bed fires at module open unconditionally. Second,
the schedule the bed is authored against: the bed's blast bump sits at
stream `+14.8 s`, which lands on the retail walk's damage stage
(`mph0+909` ticks, walk end `+1264`) once the real CD's stream-start
latency is added - but the stage-row fold breaks the mp5 hold that
provides most of that runway. Retail's mp5 arm waits for the previous
stage's clip to end (`lbu +0x1D9 == 0xD`, a ~320-tick clip boundary)
plus a playhead-cursor threshold; with every stage folded onto row
`0x0A` the played-id half is true the tick mp5 opens and the cursor
half is nearly met, so the walk reached damage at `mph0+829` and the
blast arrived seconds after the whiteout (worse under accurate CD
timing, where the stream starts later still). The fix replaces the
cursor half with a deterministic tick counter kept in a dead
wipe-body word of the module image itself (`0x801F85B0` - re-streamed
from disc each cast, so it self-resets; the caster's `+0x176` hold cell
is the clip player's live budget and deadlocks the clip if borrowed),
with the count-store riding the wait branch's delay slot. The threshold
is probe-calibrated to the retail schedule: damage stage `mph0+938`,
walk end `+1324` (retail `+909`/`+1264`).

One residual is content, not timing. With the blast pinned to the
damage whiteout, the bed's audible onset is a pure function of the
stream's internal music-to-blast spacing - no fire-timing, trim or
threshold lever can move one without the other - and the retail bed
opens with ~0.82 s of digital silence and a faint (-30 dB) pre-swell
before the first real musical event, a full-level hit at stream
+1.31 s, which on a real-latency drive reads as "the music starts
late". The fix is a front shift in place
(`delilas_xa_voice::boost_cast_bed_intro`): the first 70 sectors of
`XA20.XA` channel 2 are decoded, the stream is advanced 1.24 s so the
opening hit lands at +0.07 s (50 ms fade-in for the seek landing), and
the advance is paid back with a 1 s linear dissolve into the unshifted
stream ending at +3.05 s. The advance and the bridge point are the
argmax of LOCAL WAVEFORM correlation between the shifted and unshifted
branches over the blend window (0.883 - the bed is a ~0.565 s ostinato,
so an in-phase lag exists), which makes the paid-back material read as
one extra ostinato cycle. A spectral-similarity pick is NOT sufficient
for this: the first ship of the pass bridged at a spectrally-matched
point whose waveform correlation was -0.15, and the out-of-phase blend
was audible as the music restarting mid-cast. The blend is linear
because equal-power on correlated material bumps the middle +3 dB.
Every frame past the bridge is bit-identical
input, so the true-stereo re-encode
(`legaia_xa::encode::encode_stereo_4bit`) converges back onto the
retail bytes within the written span (the disc oracle asserts the
span's tail equals retail) and the blast keeps its authored +14.8 s
offset - the walk sync is untouched by construction. An RMS guard skips an
already-shifted stream (retail is silent at +0.15..0.6 s; re-shifting
would stack).

The bed is also why the signature special's FANFARE channels - the
host art's channel pair plus the generic Super-chain channel 1 (a
chained special fires the generic id `0x101`/`0x111`/`0x121`, not the
pair) - are written to digital silence in every arts-voice mode, with
their duration-table rows shrunk to a token 0.1 s. An earlier revision
spliced the bed's own head into them; the commit-time fanfare fire then
pre-played the opening and the module-open preempt replayed it from
zero - audible as "her intro repeats" the moment the remaster made the
head loud. Enemy-side retail has no pre-cast fanfare either: the reel
starts when the module opens.

The fourth is data, not module code: the module stages the CASTER's two
clips by raw index (`actor+0x1DA` = `0x0A`, then `0x0B` at the lift
boundary), which on a monster caster are its archive's wind-up/smash
entries but on a party caster resolve through the record[0] action table -
where retail row `0x0A` is an empty placeholder and row `0x0B` is the
character's Block clip (whose record the stage boundary is probe-measured
to choke on). The swap authors real rows instead
(`party_swap::cast_stage`): the sibling's wind-up and smash, retargeted
onto the host rig with the same conjugation as the art-side reskin and
re-encoded raw-packed, are **inserted below `clut_a_off`**, each
carrying its source clip's `+0x54` sound-cue track (frames rescaled
with the loop-window map, ids translated for the PLAYER arm of the cue
player `FUN_800508DC` - see below) - the decoded
record[0] grows by the rows' length, the two image payloads and the
`clut_a_off`/`clut_b_off`/`budget` header words (plus the paired `+0x5C`
sibling word) shift up with it, and the table words `0x0A`/`0x0B` point
at the inserted entries. The placement is load-bearing, not cosmetic:
everything in the decoded record[0] from `clut_a_off` on is battle-load
scratch (the member init uploads the CLUT-A/B blocks, then LZS-decodes
the five equip-section sub-records sequentially into the same region -
see [`battle-data-pack.md`](../formats/battle-data-pack.md)), so rows
parked any higher survive every post-load RAM probe yet are destroyed
before the first turn - the shipped symptom is a cast that freezes the
screen while its effect/SFX ticks keep looping. The Block clip survives: its entry is
re-homed byte-unmoved onto placeholder row `0x06` in all four player
files, and the one party-init literal that seeds every actor's Block
reaction id (`li 0xB` before the `+0x1F3` store in `FUN_80053cb8`, SCUS
`0x80054008`) becomes `li 6` - every consumer reads the seeded value back,
none hardcodes `0x0B`. When the rewrite cannot land, the module falls back
to pinning both stages onto the empty row (`addiu v0,v0,1 -> nop` at the
staged-index step): the caster holds a pose, and the enemy-side cast also
loses its smash stage, since the module writes the same index for both
caster kinds.

The cue tracks are the punch sounds. Every clip-synchronised battle
sound that is not an effect-script cue rides the action entry's
`+0x54` table (8 x `[u16 frame][u16 cue]`, truncated at the first zero
cue): the per-frame player `FUN_800508DC` walks it against the clip
cursor per actor and hands each fired cue to the ring producer
`FUN_8004FE5C`, which maps ids differently per arm - a party actor's
small cue (`< 0x48`) lands in the SFX ring as `id - 1` (static bank),
its `0xA7..0xC7` band as `id + 0x19C` (runtime bank), while a monster's
big cue also takes `+0x19C` and a party big cue (`>= 0xC8`, after the
track player's own `+0x38` skew) misroutes into the XA-direct path.
Probe-pinned on the retail duel (`nivora_duel_pre_plasma_strike`,
FE5C-arg capture): each enemy Plasma Strike punch volley is three
fires - the VICTIM's reaction track fires `0xAD` (ring `0x249`,
runtime bank) and `0xD` (ring `0xC`, the static thud), and the CASTER
fires `0x172` (a runtime-bank voice line only the Delilas battle VAB
carries). The authored player rows translate accordingly
(`cast_stage::author_player_cue_track`): the flurry's punch cue `0x4A`
becomes the `(0xAD, 0xD)` pair - byte-identical ring traffic to a
retail volley, resolvable in any battle - small ids (footsteps `0x11`,
impacts `0x16`) pass verbatim (identical on both arms), and
runtime-bank voice ids drop. Zeroed tracks were the shipped symptom:
the authored entries carried no `+0x54` table at all, so a
player-caster flurry punched in silence.

All three modules are audited and probe-verified: every mapped slot routes
to its sibling's retail module - Blazing Slash (958), Megaton Press (959),
Plasma Strike (960) - with per-module edit sets covering the same defect
classes. The player-side stage walks are **un-folded to the full retail
chains** when the host file's LZS budget takes them (the shipped
`lu,gi,che` mapping does; the folded two-row shape stays as the
per-module budget fallback): the player file hosts every chain clip
below `clut_a_off` (Gi's crouch/leap/slash/finale, Lu's
raise/charge/channel/strike/flourish), the staged IDS keep the folded
`0x0A`/`0x0B` values - so 960's **paired stage/confirm gate** (its
phase-5 arm re-reads the playing id `lbu +0x1D9` against the same
literal it stages) stays valid and the enemy-side caster still resolves
its own archive entries - and each staging store (`sb id,0x1DA`)
becomes a `jal` into a small SCUS-resident **stage cave** that repoints
the head-table word (`0x28`/`0x2C`) at the stage's clip, writes the
entry's `+0x88` stream pointer (`entry+0xAC` - the loader writes it
only for table-bound entries, and a mid-chain entry commits a NULL
stream without it, a probe-pinned dynarec crash), redoes the staging
store and returns (`$ra` is dead between calls at every hooked site;
no hooked word is a branch target). Each hosted clip is authored
**duration-true** at every LZS-pressure rung: the pose ladder never
sheds frames (an earlier ladder halved them, which - with the rate
byte already at its floor of 1 - halved wall time, so the clip hit the
shared tick's natural-end re-commit and visibly replayed from frame 0
mid-stage); instead a deeper rung holds each pose 2 or 4 output frames
(duplicated 9-byte rows LZS-compress to repeat tokens). And each entry
carries its SOURCE clip's **loop window** (`+0x84..+0x86`, rescaled to
the hosted frame count - reader
`legaia_asset::monster_archive::animation_loop_windows`): the retail
cast clips are authored to park or cycle inside their windows through
each stage's dwell (Gi's crouch holds `[9,10]`, his finale parks on
its last frame; Lu's charge parks, her flourish loops its tail sway),
and a zeroed window is what made the hosted stages replay whole clips.
Lu's strike goes one step further and is hosted **identity** - the
source's own 39 frames at rate 2 with its authored `[15, 15]` park
intact - because module 0960 tests the strike clip's cursor against
two ABSOLUTE thresholds: the phase-5 confirm waits for cursor `0x90`
(strike frame 9), and the damage tick waits for cursor `0x160`
(frame 22) a fixed 28 ticks after the module itself RELEASES the park
(file `+0x1638` clears the caster's `+0x176`/`+0x21B` hold budget -
the park is choreography, not a stall hazard). The anim cursor climbs
`2 * rate` sixteenths a tick, so the duration-true rate-1 re-timing
halves the climb: the confirm lands 36 ticks late, the un-parked clip
replays, and the burst decouples from the release - audible as the
strike's sound beats drifting off the motion. The caves live in three pools that
are free exactly when the route runs: the SCUS injection-gap tail
behind the queue hook, shiny-seru's read-watch-verified `ARENA2` +
`SLOT6` pockets (option-exclusive features), and PROT 958's own dead
party-wipe body (three 2-word stubs - the `ori` rides each `j`'s delay
slot - then the 4-word dead-victim HP gate; the body's last word stays
the finale victim cell). Two facts shape the per-module damage retargets:
958 keeps the victim in `$s1` only per-arm (its finale arms burn
`$s1`-`$s4` as GPU-packet constants, so the first damage arm banks the
victim pointer in that same wipe-body cell and the finale pairs reload
it), while 960's `$s3` holds tick-wide and takes plain `move`s. 960
additionally carries 959's cached-finale-entity teardown hazard (same
`ctx+0x102C` halt-quad pattern; same settle-tail neutralise, hosted in
its dead wipe body) and two seat-3 monster-record toggle stores that
are nop'd (an arbitrary victim's record `+0x80` word is not a vetted
pointer). One data constraint crosses the module edits: 960's damage
tick holds until the playing clip's cursor reaches keyframe 22, and
that tick rides the strike stage (the wind-up row under the folded
fallback) - under the full chain the strike hosts identity (39f rate
2, park window kept - the module releases it); under the fold, where
the gate rides a different clip's restages, the bound clip is
stretch-floored at 23 keyframes and ships windowless, player and
mirrored-block alike. Module anatomy (image shapes, phase
machine, staging ABI, the seat-0 damage hardcode):
[cast-module.md](../subsystems/cast-module.md). The hook stub lives in
the SCUS injection gap, so the route composes with neither
`--shiny-seru` nor `--show-super-arts`; on a conflict the patch keeps
the art-side signature and says so in the summary.

The queue hook does not convert whole-queue any more - that discarded
any arts entered BEFORE the signature combo (somersault then Plasma
Strike played only Plasma Strike). The rework splits the conversion in
two, both routines in shiny-seru's `ARENA1` (`0x8007AE00`, free under
the cast route by the same option-exclusivity, claimed
free-or-identical like the gap). At assemble time the hook's HIT block
hands the matched `[0x19|0x1A starter][marker]` pair to the ARENA1
queue-edit: a starter **at the queue base** converts to the cast
exactly as before - immediate cast, the approach run never fires; a
starter **anywhere else** defers with the queue untouched. The
base-or-defer split follows the retail matcher's own emission shapes
(`FUN_801EED1C`): the Hyper arm (`0x801EF4E8`) consumes the matched
arrow span, so a bare signature input tokenizes to `[1A marker]` at
the base and a chained one to `[.. 19 art 1A marker]` - no windup
directions ever precede the signature's starter in a real queue. The
only queues with raw directions there are chains whose leading art the
matcher's AP admission gate dropped (`0x801EF424`: tiers are charged
per completed match, later-starting matches first, and an unaffordable
match emits nothing), and those directions are basic strikes the
player entered - deferring plays them out instead of eating them. The
route also halves the Hyper admission tier
(`install_chain_admission_tier`, `li t4,0xA -> 0x5` at `0x801EF32C`)
so an art-then-special chain clears admission at realistic mid-battle
AP (the five-arrow signature admits at 25 instead of 50; the walk's
deductions are refunded at the applier's end, `+0x170 += +0x224`, so
this is the per-command charge, not a second pool). The deferred half
is a two-word detour at the attack band's
strike-loop fetch (`FUN_801E295C` state `0x1E`, `lbu v1,0x1df(v0)` at
`0x801E374C` -> `jal` with the fetch riding the delay slot; no branch
targets either word, and the displaced `+0x1DC` busy-latch load returns
in the morph's exit delay slot): when the fetched byte is a starter
whose next byte is the ACTIVE slot's route marker, the action morphs
mid-chain - category 2, spell over the consumed queue head,
`ctx[7] = 0x28` - and the capture-class spell routes `0x28 -> 0x6E`
into the module without re-reading the mid-queue cursor. A missed
morph fails soft: the marker plays as an ordinary art row and the
chain ends normally (probe-measured on a routes-zeroed image). The
per-slot markers double as the replaced host art's own constant
(Vahn/Che `0x1C`, Noa `0x1F` = Vulture Blade), so performing that host
art alone still casts - the intended replacement semantics.

The banner follows the same conversion. Retail's state-`0x28` body
raises the `0x4C` spell-name banner for **monster** casters only
(`lbu v0,0x2(s5); sltiu v0,v0,3` at `0x801E43D0` - the party side has
no banner writer anywhere), so a converted signature kept whatever the
arts chain last wrote: somersault into Plasma Strike read "Somersault"
through the whole cast. The un-gate
(`delilas_cast::install_cast_label_gate`) is two in-place words -
`lbu v0,0x1DE(s3); sltiu v0,v0,2` - retesting on the action category:
the Item band (the summon items, retail's own skip) still skips, and
every Magic cast runs the retail label block, monster casts
bit-identically. Player Seru casts gain the same banner enemy casts
always had - a deliberate presentation upgrade; no free injection
arena exists on a delilas image (gap 1, ARENA1/2 and slot 6 are all
carved), so an id-scoped gate had nowhere to live. The name the banner
reads is the sibling special's own: the spell rows `0x79..=0x7B` keep
their retail names ("Blazing Slash" / "Megaton Press" /
"Plasma Strike"), because the enemy-side rename that once pointed them
at the host art names is retired - the mirrored hero's signature is a
physical attack now, so the un-gated player banner is those rows' only
reader.

`--delilas-che-hammer` is a visual comparison option on top of the
swap: Che's welded giant hammer stays on his mesh (instead of the
mirrored-fist replacement) and the host's weapon fusion is skipped for
his file, so he fights with the hammer regardless of the equipped
weapon. The kept hand also switches wrist law: every retargeted clip
normally slaves each hand to the HOST's retail rest wrist attitude
(the fix for the fused host weapon pointing across the torso), but
with the sibling's own weapon in the hand that slaving is exactly
wrong - a hand-sized fist hides what a 300-unit hammer amplifies, and
the kept hammer measured 73 degrees of long-axis error against Che's
own rest. The kept welded hand plays the sibling's natural conjugated
wrist instead (`winpose::retarget_clip_wrist`,
`party_swap::playerize::kept_welded_hand`), which pairs with the
unmodified mesh bake to reproduce Che's model-space attitude exactly
(re-measured 0.0 degrees). `delilas-verify` detects the state from the
disc (a hand channel's textured radius far past any real fist) and
waives only the fusion-presence check; `delilas-audit` reports the
welded radius as a FAIL unless `--allow-kept-hammer` is passed.

## See also

- [`randomizer.md`](randomizer.md) - the feature and flag reference.
- [`randomizer-internals.md`](randomizer-internals.md) - write path, injection arenas, hook pattern, tests.
- [`crates/party-swap`](../../crates/party-swap/README.md) - the swap kernels (rig permutation, rest-pose bake, movesets).
- [Cast modules](../subsystems/cast-module.md) - the capture-class cast modules the retail cast route drives.
- [Muscle Dome](../subsystems/minigame-muscle-dome.md) - the arena ladder the challenge course joins.
- [Battle data pack](../formats/battle-data-pack.md) and [monster animation](../formats/monster-animation.md) - the model and pose formats the swap rebuilds.
