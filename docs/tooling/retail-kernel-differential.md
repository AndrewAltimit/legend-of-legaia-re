# The retail kernel differential

`port-catalog.py` asks whether a `// PORT:` routine has a tag and a caller, and
[`check-port-provenance.py`](port-provenance.md) asks whether the address names
the right routine. Neither asks whether the Rust **computes what the bytes
compute**. The retail-compare corpus does ask, but only for what a save state
happens to show.

The differential asks directly. `crates/retail-exec` is a user-mode R3000
interpreter. Its disc-gated tests load `SCUS_942.54` and any overlay off the
user's disc, seed RAM with generated inputs, call the routine, and compare what
it wrote with what the Rust kernel produces. There is no decompiled C and no
hand reading between the two sides. No Sony bytes are committed; without
`LEGAIA_DISC_BIN` the tests skip.

```bash
cargo test -p legaia-retail-exec --profile dev-fast --test integration
```

## What it can run

Integer instructions, `hi` / `lo`, unaligned loads and stores, RAM and the
scratchpad. `Cpu::hardware_zero_div` runs a `div` by zero as the chip does, for a
routine with no `break` guard, where that result is what ships. A routine that reaches the GTE or COP0 stops with
`Stop::Unsupported`, so a pure kernel is the natural target. A call can be
replaced with a `Cpu::hook`, which is how `rand()` is scripted so both sides
draw one stream. `Cpu::calls` records the calls made, which is how a test proves
it reached the arm it claims.

## How to add a routine

1. Read the disassembly and find what it reads and writes. Seed those bytes with
   the shapes a game reaches, including the caps and the empty cases.
2. Compare whole regions rather than chosen fields. A write the port does not
   know about is exactly what this is for, and a field-by-field test cannot see
   one.
3. Assert the run reached the interesting arms, so a seed change cannot leave
   the test passing on nothing.

## What it has found

The first routine in it, the stat aggregator `FUN_80042558`, showed its grant
and revoke scans had no port at all, behind a module heading that tagged them
ported. Nothing in the engine put a Talisman's summon or a final-form Ra-Seru's
summon on a spell list ([the mechanism](../reference/glitches/summons-and-jette.md)).
It also showed the record's live stat window was only rebuilt for HP and MP, so
an accessory's percent boost never reached the status screen's ATK.

The battle seed `FUN_80053CB8` showed the port clamping after the flat equipment
bytes where retail clamps before them. The level-up applier `FUN_801E9504` showed
the stat jitter was never enabled on any host, so every level-up took the curve's
exact mean and drew none of retail's eight `rand()` calls a level, and that the
six battle stats had no cap on the way up.

The melee resolver `FUN_801EC3E4`, run whole against `World::land_melee_hit`,
showed that a clip's `+0x7A` selector inflicted nothing on a normal swing (Venom,
Toxic, Rot and Curse were only rolled on enemy specials) and that a Stoned
defender still took damage when its Stone was the raw status word a cast writes.
It also showed the Mystic Shield stopped no plain swing and halved instead, the
block breaker's quarter damage and the drain clip had no port, the shared
finisher's no-damage floor had been wired onto a routine that has none, the Spirit
percent wraps at a byte in this routine alone, and a block the roll grants spends
the approach terms even when the verdict is then overturned.

The spoils arm of `FUN_8004E568` showed the port paying EXP and gold for every
seat in the formation, where retail pays for a seat that died and was not
captured: a Seru the Ra-Seru took pays nothing, and neither does a monster that
ran. The level-ups draw `rand()` ahead of the drop walk, and the port drew them in
the other order, so a level-up changed which item dropped. A Stoned member takes no
EXP and is not counted in the split, a fallen member's Items Up still counts because
the EXP loop has stood them up at 1 HP before the scan, and a level 99 member keeps
earning up to the record's cap.

The monster flee roll `FUN_801EC0DC` read the fleeing monster's INT back off its
record under the scripted fight's profile even in a random encounter.

The round opening showed four more. A fallen actor draws its initiative roll, and
the port skipped it, so a fallen member shifted every later roll. The written turn
order of Tetsu's spar and of Cort's final form had no port. The formation roll's
spread divide has no guard: level sides divide by zero, the chip leaves the raw
draw as the spread, and the port neither drew it nor added it, so an evenly matched
fight had no ambush and no pre-emptive strike. The status tick read the typed status
list where retail reads the word, so a poison a cast wrote never drained; poison
wore off on a timer retail does not have; and Life Grail and Magic Grail restored
nothing in battle.

The action validator's kernel matches over every arm. Its battle host read the raw
word alone, so a member poisoned by a hit had nothing a cure could target.

The sound driver runs through [its own harness](../subsystems/audio.md#the-keying-differential),
because a track takes minutes of vsyncs and the SPU has to feed `ENVX` back: libsnd's `FUN_80062F98`
over a state's RAM, its registers applied to the port's `Spu`. It found the note-off keyed on channel
where `FUN_8006688C` keys on program, a pan or volume event missing voices already released, the
sequence volume re-reading a moved pan, a pause leaving tails sounding, and the allocation scan reading
live voices where retail reads the flush's own record, which had voice numbers apart in every score.

The bag's accessor family, run as a stream of mixed operations, showed normalize
`FUN_800423E0` to be one pass of two cursors rather than a whole compaction: one call
can pass a hole by, and the slot order after it differed from the port's. The walk
step's wall probes `FUN_801CFE4C` match, except that a negative coordinate wraps
through the grid's masks where the port called it a wall.

The monster picker `FUN_801E9FD4` found a walk that matches nothing drawing a physical
target and a full swing budget the retail seat never gets; a zero-gauge monster with
no base gauge never arming its swings; the interrupt arm of monsters `0x28..0x2A`,
`0x7F`, `0x85` casting at members who had already acted or were Stoned, with the wrong
denominator on `0x29`; monster `0xB6` casting past its five round cases; Tetsu's
post-amble missing whole; and a pass that failed on its target forgetting the cast it
had stored. The generator's spell ids stop at the table's end, and a second test holds
every shipped record to that bound.

The encounter roll `FUN_801D9E1C` found the anti-repeat latch cleared on every scene
entry, where retail's one global stands across them; the counter, the rate shifts, the
story-group walk and the region's battle setup all match. The death arm of
`FUN_8004AD80` and the MP-cost leaf `FUN_80035394` match. The death arm's table test
leaves out row `0xBF`, which holds the neighbouring table's `[9, 0]`: retail would add
item `0` there and the port refuses it, and a second test holds that no shipped monster
reaches such a row.

The applier's battle path `FUN_800402F4`, over every item the battle menu offers,
found Medicine and Antidote reading the typed status list where retail masks the word;
MP items restoring a fallen member, where the arm gates on HP; the MP readout `+0x178`
unwritten; a Phoenix restoring a flat quarter where retail restores `40% + rand %
(max / 8)`, drawing `rand()`, clearing the status word and the gauge boost and
restoring the live gauge; and Fury Boost landing on a fallen member. The whole arm ran
at the commit: the action SM calls it at state `0x3F`'s timer (`0x801E4134`), so a heal
showed on the bar before the user moved, and any `rand()` it drew came ahead of the
band's. The Point Card was not offered at all, on a catalog note that no item reaches
its arm: it does, by id, through state `0x3E`. Selectors 9 and 10 match.

The Arts entry `FUN_801D0748` state `0x50`, driven with a pad script, found a pool that
cannot pay the first command leaving for the review screen with an empty string, where
retail stays in entry. The round sweep `FUN_801D88CC` stops at seven slots, so a fifth
monster seat keeps its spent gauge and its stream; the port reset it. The magic-level
check `FUN_801E70BC` matches.

The walk step `FUN_801D01B0`, with its helpers, found the port reading the held mask
where everything after the slide resolver reads the resolved one, so a skid along a wall
neither turned the player nor took the diagonal's quarter off. The cut also has a camera
mode gate the port lacked, and the terrain slow tile had no port at all.

The queue builder `FUN_801EED1C`, with its learn check and the Super applier, over the real
art banks, found the Spirit price charged after the walk instead of gating it: an art the
gauge could not pay still ran, for nothing beyond an empty gauge. A learn verdict of "not
known" dropped nothing; the Miracle row landed only on a whole-string match, not on the
walk's run, and left the later groups reading arrows it should have covered; and a limb
rotted after the entry kept its swings.

The Spirit block of the action state machine's cleanup arm, `0x801E5D60..0x801E5E8C`, run on
its own off the disc against the Done band, holds the port to the arm that pays an art's
price: the accumulator comes off the gauge ahead of the action's own `+8` or `+0x20`, the two
AP Boost passives and the cap, with the gauge left alone until then.

The shared cast kernel `FUN_801DD0AC`, its scale and finisher, and the two wrappers
`FUN_801DD4B0` / `FUN_801DD6B4`, on both branches, found no cast waking a sleeper or
freeing a Numbed seat, where the finisher strips `0x0F80` as the melee resolver does;
a monster's hit on a monster taking the defender's element off the party table;
spell XP and the Seru debuff landing in a special battle, where the tail returns
first; spell XP banked off a Stoned victim whose hit the finisher had zeroed; and a
module's own roll ignoring both seats' Venom and Toxic, drawing a third `rand()` on
the bypass wrapper, which draws two, and handing a Stoned victim the whole hit.
A third test calls each capture-class special the way its module's damage site does:
it reads the site's `jal` (which wrapper) and its `addiu a0, zero, imm` (which power)
off the module image, calls that, and holds `World::enemy_move_predamage` on the move
id to it, half the victims in the Spirit stance. It pins the routing from a move id to
its module, baked power and wrapper, which the wrapper tests take as given.

The Auto assembler `FUN_801F0450`, run whole over the real art banks, found the tail's
Spirit gate drawing `% 7` where the bytes draw `% 14`, and the pool dropping every
command past the sixteenth entry, where the scratch has room for all thirty-two. The
delegated arm's redirect `FUN_801DB124` takes no seat: it reads `ctx[+0x274]`, so the
roll it follows stands dead or not, and its draws and its rewrite land on the turn
picker's last pick. The spend loop can spin for ever in retail; the test holds the port
to ending the spend there.

Running the builder after it found the round playing Auto's arrows raw, where the seed
tokenizes them as it does an entered string, and the learn check `FUN_801EFBFC` teaching
on every use, where a seat on the Auto flag draws `rand() & 0x1FF` on each art and learns
only on a zero. The head of the same builder found the AI companion drawing twenty
`rand()`s a turn, reading the leader's status word off the typed list only, and giving up
on a row after sixteen target redraws, where retail's redraw has no bound. Her HP and MP
pinned at 200 and the string the spar's third round forces on the lead had no port.

The slot machine's kernels in PROT 0975 found a resolved win ending every reach and hot
mode, where a reach stands until its jackpot lands and hot mode ends on a one-in-150 LCG
draw; the winning line taken by best payout, where retail keeps the highest symbol id; a
stop always reaching its landing row, where the budget can leave the reel short of it; and
the per-frame LCG draw missing, so a stop's landing never depended on when it was pressed.

The seed's retarget `FUN_801E7320` found a confused monster in a party of two misreading
its own band's first seat as a party seat, the gate reading the typed list where retail
masks `0x380`, and a confused cast always crossing sides: in retail a group cast crosses
one time in three and comes to nothing the other two.

The field VM `FUN_801DE840`, one instruction at a time over every distinct instruction in
the disc's scene scripts with its calls stubbed, finds the operand widths of the port's
`step` right everywhere it can compare them, and one wrong exit: `4C 80` left the parent's
PC on the child records, where retail walks past them. A call that lands on an empty
function pointer is why `Cpu::stub_outside` counts toward the step budget.

Baka Fighter's combat tick `FUN_801D3F44` and round SM `FUN_801D3468`, run whole rounds at a time in the pool walk's order, found the opponent committing on a cooldown of its own, with the player free to read its clip and counter it, where retail's opponent throws only in answer to the player's commit and blind to it. The "cooldowns" those arms store are the rumble levels and pace nothing. Exchanges ended at the booking, so an attack's second strike never landed, a struck fighter's pad was never masked, and the hit streak never cleared when a reaction clip ended; it climbed across whole rounds, and every later hit paid the streak's bonus. The player could throw during the round banners, the push-back and the camera's follow had no port, and neither did the rumble.

The dance floor's handler `FUN_801D1358` with its award routine, run over whole songs for every dancer, found the port holding each dancer to one press a beat, where only the latch and the move clip hold one, and rivals scored about a fifth low for it (a hands-off qualifier's two rivals `245` / `208` against `295` / `253`). It also found a Circle miss counting one, the feed's schedule running on through move clips and refusing to move with no stock, free play's rivals never riding the lead's gauge, the free-play Disco King's schedule cut off at the table's end, and his loop-named moves locking him.

The field VM's state arms close with the host answering what clear RAM answers: `4C 53` holds on the staged mode switch `FUN_801D65D8` where the port stepped past it on one frame, and `4C EA` runs once and advances where the port re-ran it at its own PC. Every other arm the first sweep could not compare agrees on both polarities of its test. Its write-up above is the sweep's stub world; a test seeded with no actor and no item holds the skip paths.

The hooked fish `FUN_801D4004` with the gauge's reel arms matches over whole fights at every species, rod and frame step.

The Seru side-effect stager `FUN_801F3D3C` found the port rolling the scripted bypass on the first enemy the cast targeted, where the bytes read the formation's first record whoever is targeted and whether that seat stands, and comparing a group cast's first target, where the bytes walk to the first seat with HP left and stage with no compare when there is none.

Reading the shop's quantity steppers for a differential turned up the word they step on: the window kernel's cursor auto-repeat `FUN_80035C24` had no port, so no list in the game repeated on a held direction, and buying ninety-nine of anything took ninety-eight presses or nine and eight. The steppers' own arithmetic reads the same as the port's. The inn's restore op `4C 82` writes exactly the two HP and MP words the port writes.

The Best Equipment row `FUN_801CF88C` / `FUN_801CF760` found the port taking the last copy of a piece by zeroing its count, where the take helper zeroes the id and leaves a hole, and walking the bag in id order, where a tie goes to the first slot the walk reaches. The impact arm of `FUN_801E09F8` found an enemy special's Rot leaving the Auto strings alone, where a limb newly rotted empties both, as the melee arm does. The action overlay's pool scans and recentres match.

The fishing cadence recogniser `FUN_801D3DB4` found a different walk from the port's: retail passes over one slot that does not match between two steps, spends the template's window as a budget over visited slots, matches within nine frames rather than ten, and on a match leaves the last-button latch unwritten. The waiting arm of `FUN_801D26CC` (`0x801D3198..0x801D382C`) found the recogniser running every frame in the port, where it is called only from inside the band body, so its ring stands still through a held band; the strike draw is taken every frame, held or not; and the countdown runs on below zero at a short line. The power meter of `FUN_801CF3BC` state `0x14` steps `0x20` per frame-step from zero, and the hosts ran it at four times that on a figure the code called unpinned.

The Muscle Dome's arena re-entry `FUN_801CEA6C`, the leg's rows `FUN_801D1184` and the settlement `FUN_801D0F60` match the port's `DomeContest` over whole contests. Reading them for the differential found what the contest does around that arithmetic: the arena copies the `0x2000`-byte SC block out on first entry and settlement copies it back, keeping only the lead's `+0xE8` / `+0xEC`. The port kept the legs' HP, items and gold, and an Expert or Master fighter's stripped gear never came back. It also set the prize latch `0x6CB` itself, which the arena only reads and the lobby's winner's scene sets after testing it.

The dialog pager `FUN_801D84D0`, call by call over generated pages at every frame step and window size, confirms the row window's pacing, scrolls and taps, and found a page turn on `0x48` typing the next box on the second call. Retail's state `9` puts the row count back to three, state `0xA` collapses the box over `0x1000`, and `0x801F273C` keeps it gone for `0x18` frame-steps, about two thirds of a second at the field's step. A press on a box's opening call does nothing there, where the port took it as a skip.

The lure's flight and in-water move in `FUN_801D26CC`, with the tail's record, replaced what the port had reconstructed: the record is the lure's distance from its spawn point, and line comes in by the press, not by holding. An inn's `4C 82` and `3A` match.

Baka Fighter's round score `FUN_801D2A28` matches, and tracing what feeds it found the channel unwired: no host installed the two bonus tables, the match bonus row was never seeded, and the running total never reached the cabinet, so neither secret opponent could ever appear. Retail also scores only the player's round win, off the player's HP.

The same search for setters with no live caller found the bag's window selector `FUN_8004313C` unwired: the live game never left the full window, so a lone character saw the whole array.

The bearing `FUN_80019B28`, run over the `i16` plane against `bearing_12bit_approx`, disagreed on
about half of all inputs by one unit: the port computed the arctan table at `0x8006F4C8` by rounding
where the retail table truncates, so 1037 of its 2049 entries stood one high. Every battle facing,
approach and separation nudge read through it, and the motion VM's face-target ramp and the field
NPC facings used a rounded `f32` `atan2` with the same error. The one table and the one integer
bearing now sit in `legaia_engine_system::trig`, and the separation pair `FUN_80050BB8` matches
`push_apart` over it. Reading the bearing's callers found the move VM's sub-op `0x3A` measuring from
`+X` (`atan2(dz, dx)`), which mirrored every angle it wrote.

The take-item fallback `FUN_800430AC` walks the present-party list at `0x80084598`; the port walked
the whole roster, so a script could take an accessory off a member who was not in the party. The
shout selector `FUN_8004C140` matches over every roster id and action constant.

The Use list's applicability probe `FUN_8003043C`, run with the validator live beneath it over every
field-usable item, found the port answering it from the catalog's effect where retail asks the validator.
An MP restore lit for a fallen member, a stat seed for a member already at the stat's cap, and an arts
book for a party without its reader (classes `0x0B..0x0D` match one roster id); the Doors never read
system flags `5` / `6`. The Items screen's confirm used the same stand-in, so it could use a row the list
had greyed. The probe now runs the ported validator over the roster records, and the party heal's walk
reads a present id of `3` as the dead quad: out of battle the validator's pointer arrays stop at three,
so that read is stale stack in retail. The spell twin `FUN_8003053C`, which greys a Magic row and refuses
its cast, matches over the same seeding.

The floor sampler `FUN_80019278`, with the kind-2 lookup `FUN_801D5630` live beneath it, matches on every
tile of the grid, ramps and stairs included. Off it the port called the floor `0`, where retail halves the
sub-cell toward zero (a position in `-64..0` stands on tile `0`), wraps the object cell through its masks
and reads the corner block on into the descriptor table or the object map.

The Arrange kernel `FUN_801D64A8` matches the port's sort over every window. Reading its caller for the
differential found the port sorting only the screen's rows: the bag never moved, so the next Items screen,
the battle window and the save all kept the old order.

The menu's stat block, the seeder `FUN_801CF5D0` and the summer `FUN_801CF650` the Status page, the Equip
compare columns and the shop preview draw, matches `menu_stat_block` over the disc's equipment table.

The whole turn picker `FUN_801DABA4`, with the monster picker, the redirect and the Counterattack roll live
beneath it, found four defects. The picker's Numb tail had no port: a Numbed monster cast or struck where
retail's turn arms nothing. A walk that matched nothing stored category `3` in the port, so the redirect
re-rolled and the Counterattack coin was flipped where retail's stale `0` does neither, and the roll never
read the stored category at all. The side scans counted a seat down only on a Stone the status tracker held,
so a cast-written Stone left a petrified side's monster deciding a turn. The sweep, the tie pick and the
item refund match. The Confirm-path sale `ShopSession::try_sell` priced per unit off the shop's stock; it
now pays the stepper's `(item-table price * qty) >> 1`.
