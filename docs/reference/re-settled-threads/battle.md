# Settled threads: Battle / arts / level-up

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers the battle runtime: how a fight is seated and flagged, the command ring and its
refusals, the action state machine, damage / status / spoils rules, the cast and summon modules, the
Muscle Dome hub, and the Arts and level-up bookkeeping. Each row states one answered question with
the instruction addresses that pin it, so a reader porting, modding or auditing battle behaviour can
check the claim against the disc and follow the link to the page that owns the full description.

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [The battle hit tint - what a landing hit writes and how it reaches the pixel](#the-battle-hit-tint---what-a-landing-hit-writes-and-how-it-reaches-the-pixel)
- [Battle end - the results sequencer's timeline, and who strikes the pose](#battle-end---the-results-sequencers-timeline-and-who-strikes-the-pose)
- [What a normal party attack sounds like](#what-a-normal-party-attack-sounds-like)
- [`FUN_8003EAE4` is a seek plus bookkeeping - and the driver is not untraced](#fun_8003eae4-is-a-seek-plus-bookkeeping---and-the-driver-is-not-untraced)
- [Formation species limit - what the battle setup does with 3 distinct monster ids](#formation-species-limit---what-the-battle-setup-does-with-3-distinct-monster-ids)
- [Attack-band damage pacing - the hit-event model](#attack-band-damage-pacing---the-hit-event-model)
- [The battle HUD's per-phase surfaces are the sub-draw script table](#the-battle-huds-per-phase-surfaces-are-the-sub-draw-script-table)
- [The ring's element chip reads `-` or the Ra-Seru, and the gate is the equipment byte](#the-rings-element-chip-reads---or-the-ra-seru-and-the-gate-is-the-equipment-byte)
- [The combo counter's glide is placement record 80's](#the-combo-counters-glide-is-placement-record-80s)
- [The chrome `kind` byte is an index into the widget-class table](#the-chrome-kind-byte-is-an-index-into-the-widget-class-table)
- [The element-badge palette selector](#the-element-badge-palette-selector)
- [The status-element badge sheet (`0x18..=0x20`)](#the-status-element-badge-sheet-0x180x20)
- [Battle-intro tile shatter - the side-face shade page](#battle-intro-tile-shatter---the-side-face-shade-page)
- [Battle-intro transition length - `DAT_801D2458`](#battle-intro-transition-length---dat_801d2458)
- [Battle ground grid depth cue - the far colour](#battle-ground-grid-depth-cue---the-far-colour)
- [Who calls the battle on-screen test `FUN_8005126C`?](#who-calls-the-battle-on-screen-test-fun_8005126c)
- [Action-SM state `0xFF` treated as battle end by the port](#action-sm-state-0xff-treated-as-battle-end-by-the-port)
- [Endless camera orbit - the `0x19` attack-approach park](#endless-camera-orbit---the-0x19-attack-approach-park)
- [The in-fight action framing and the yaw-counter ladder](#the-in-fight-action-framing-and-the-yaw-counter-ladder)
- [The Done band framing and the two orbit writers](#the-done-band-framing-and-the-two-orbit-writers)
- [The summon-then-melee park trigger - the stale field is `+0x1DC` bit 2](#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2)
- [Super / Miracle Arts trigger chain](#super--miracle-arts-trigger-chain)
- [Super Art connectors and physical inputs - the tokenizer keeps the leading arrows](#super-art-connectors-and-physical-inputs---the-tokenizer-keeps-the-leading-arrows)
- [The runtime art-record chase - the `+4` and the `- 0x10` grid origin](#the-runtime-art-record-chase---the-4-and-the---0x10-grid-origin)
- [Character-record HP/MP/AP pair order](#character-record-hpmpap-pair-order)
- [Effect-VM pass-1 "state token algebra" (`FUN_801E0088`)](#effect-vm-pass-1-state-token-algebra-fun_801e0088)
- [Battle face-stamp issuing site](#battle-face-stamp-issuing-site)
- [Spine flag `0x482` (Drake mist-wall) writer](#spine-flag-0x482-drake-mist-wall-writer)
- [Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer](#flag-0x63a---the-vellvozz-p27-gate-with-no-script-writer)
- [Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer](#spine-flag-0x142-caruban-beat--dolk-dolk2-switch-writer)
- [Drake Castle deep interiors (`jouinc`/`jouind`) depth decode](#drake-castle-deep-interiors-jouincjouind-depth-decode)
- [cave01 P2[16] spawner - the slot-counted interact chain](#cave01-p216-spawner---the-slot-counted-interact-chain)
- [NPC dynamic facing - two laws and an execution order](#npc-dynamic-facing---two-laws-and-an-execution-order)
- [0x4C 0x51 byte +3 reconcile - facing wins; no motion-bytecode synthesis](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis)
- [kor-family op-0x49 flag window [0x138..0x13F] - Uru Mais warp-pad picker](#kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker)
- [Encounter MAN sub-section layout](#encounter-man-sub-section-layout)
- [Seru-magic summon visual (e.g. Tail Fire)](#seru-magic-summon-visual-eg-tail-fire)
- [The party cast trigger is a params stager](#the-party-cast-trigger-is-a-params-stager)
- [Player-summon presentation](#player-summon-presentation)
- [The fade actor's hold word](#the-fade-actors-hold-word)
- [`summon.dat` / `readef.DAT` side-band streaming](#summondat--readefdat-side-band-streaming)
- [Monster steal item (Evil God Icon)](#monster-steal-item-evil-god-icon)
- [Per-spell magic power / multiplier](#per-spell-magic-power--multiplier)
- [Stat growth-rate source](#stat-growth-rate-source)
- [Monster stat-record archive source](#monster-stat-record-archive-source)
- [Monster mesh + texture pool](#monster-mesh--texture-pool)
- [Terra slot-3 / story-flag overlap](#terra-slot-3--story-flag-overlap)
- [Battle party meshes = **assembled from the player battle files** (PROT 1204 = Baka Fighter / default-equipment sibling)](#battle-party-meshes--assembled-from-the-player-battle-files-prot-1204--baka-fighter--default-equipment-sibling)
- [MP-cost ability-bit priority (half vs quarter)](#mp-cost-ability-bit-priority-half-vs-quarter)
- [Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg)](#scripted-tetsu-encounter--battle-v01-oracle-battle-leg)
- [The battle-intro enemy-name banner](#the-battle-intro-enemy-name-banner)
- [What a port owes the slot-B module band](#what-a-port-owes-the-slot-b-module-band)
- [readef groups 19..21 and monster record `+0x1C`](#readef-groups-1921-and-monster-record-0x1c)
- [The dome panel-still arming](#the-dome-panel-still-arming)
- [The Miracle marker is an equipment byte](#the-miracle-marker-is-an-equipment-byte)
- [Two battle-context bytes read wrong](#two-battle-context-bytes-read-wrong)
- [Stone and Curse in the SCUS effect applier](#stone-and-curse-in-the-scus-effect-applier)
- [Attack x2, the swing class and the apply-mode arms](#attack-x2-the-swing-class-and-the-apply-mode-arms)
- [How a cast reaches its slot-B module](#how-a-cast-reaches-its-slot-b-module)
- [What an art body costs in AP](#what-an-art-body-costs-in-ap)
- [Tutorial prompt machine exclusivity](#tutorial-prompt-machine-exclusivity)
- [The _DAT_8007BA78 census is closed](#the-_dat_8007ba78-census-is-closed)
- [The 0xF8 halt-acquire handshake](#the-0xf8-halt-acquire-handshake)
- [Growing a scene TMD pack member](#growing-a-scene-tmd-pack-member)
- [What of a signature cast is data](#what-of-a-signature-cast-is-data)
- [Op `0x23` MOVE_TO keys on ctx identity](#op-0x23-move_to-keys-on-ctx-identity)
- [Dome ringside panel stills PROT 1221 and 1222](#dome-ringside-panel-stills-prot-1221-and-1222)
- [The display object's sound-bank category](#the-display-objects-sound-bank-category)
- [The evolved-Cort flow park](#the-evolved-cort-flow-park)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Does the fight against monster `0xAF` (Tetsu) seat and flag as retail does? | resolved (yes) | `disassembly` | Tetsu's only row, `town0d` row 4, carries header byte `1`, so `ctx+0x287 = 4`, Run is refused and the roll is skipped. Battle init still raises `0x200`: its `0xAF` arm (`0x800519DC..0x80051A04`) runs straight-line after the seat loop, ungated by the row. The lone seat is `(0, 800)` from either seat family - counts 1 and 2 are authored identical in both, and every solo library battle reads it. The engine enters the row this way (`tetsu_row_is_flagged_and_forbids_the_ra_seru`). No state inside the fight exists; the tutorial spar is monster `0x4F` with the word `0`. |
| Do party statuses survive a battle? | resolved (no, except in an arena leg) | `disassembly` | The exit's party loop clears `+0x16E` unless `_DAT_8007BAC0 & 0x100` (`0x80046E90..0x80046EB0`), and floors HP 0 to 1 on every exit. The controller node runs before the actor nodes, so the party ticks copy the cleared word to record `+0x12E` (`0x80048040` / `0x80047680`), which battle init reloads (`0x80051718`). That the list walk finishes in the exit frame is inference; all 38 non-battle library states read `+0x12E` as 0. Port `battle_formulas::battle_exit_party_reset`. |
| What is the loss window `0x42`? | resolved (the result window's twin) | `disassembly` | `FUN_801D8DE8` ids index the SCUS placement table `0x80076C10` (jump table `0x801CEB68` at `id - 0xA`); records `0x41` and `0x42` are byte-identical, a framed 288x42 box landing at `(16,160)`. The loss string is the defeat buffer `ctx+0x129` (`FUN_801D84C0`): the lead's name plus the suffix at `0x801F4C94`, or the team string `0x801F4C78` with its name escape patched. Both hosts draw it through `World::battle_defeat_banner` ([`battle.md`](../../subsystems/battle.md)). |
| Does a monster's one-shot clip tween into its queued clip? | resolved (yes, and the step it tweens across moves the actor) | `disassembly` | On the stream's last frame the next entry is frame 0 of the queued clip when HP `+0x14C` is non-zero and `+0x1DA < 0x10`, with the committed entry's `+0x0E` added to the Z delta unless `+0x228` is set (`0x80049A7C..0x80049BD0`, `0x80049D64`). The natural end (`frame >= frames`) moves `+0x34/+0x38` by that `+0x0E` along the facing (`0x80047A68..0x80047B2C`); the event-path cut moves it pro-rated (`0x80047950..0x80047A28`). Port `World::battle_tween_target`, `MonsterAnimPlayer::set_tween_target`, `motion::end_root_step`. |
| Which keyframe does `FUN_8004998C`'s re-blend retry rewrite? | resolved (the next frame's, in a scratch journal) | `disassembly` + `capture` | The counter `gp+0xA10` is zeroed per part (`0x80049D6C`) and gated `> 0xC00` (`0x80049FD8`); the retry rebuilds the next frame's triple as `(x+0x800, -(y+0x800), z+0x800)` in the journal `0x801C9060`, never in the stream. The battle unwrap guards on `> 0x800`, the field helper on `>= 0x800`. On PROT 867, 103650 of 9448740 blended part samples take it; whole-frame samples are unchanged. Port `engine-vm::battle_pose_blend`, live on both hosts. |
| Which arts directions does Rot disable? | resolved (Left `0x08`, Right `0x10`, Up and Down `0x20`) | `disassembly` | `0x801D1E60..0x801D1F7C`, each refused press answered with cue `0x23`. The ring refuses Attack at `0x38` (`0x801D1560`) and Magic under Curse (`0x801D1434`). Port `rot_blocks` / `ring_arm_refused`. |
| What does a special battle do at the wipe check? | resolved (the whole party counts down when the leader is fully rotted) | `disassembly` | With `_DAT_8007BAC0` non-zero, `0x801E6578..0x801E65AC` counts the whole party down when the leader's `+0x16E & 0x38 == 0x38`. The three Rot limbs accumulate: the applier ORs each new limb in at `0x801E1734..0x801E1740`. Port `battle_formulas::special_battle_wipe`. |
| What does Run do in an arena leg? | resolved (the leader acts first and the leg reports a run) | `disassembly` | Flow `0xFE`'s tail (`0x801D3228..0x801D328C`) stores `ctx+0x274 = 0` and the leg outcome `0x80084448 = 4`. First monsters `0xAF` and `0x3D..=0x3F` are exempt - exactly the two `0x200` fights. Port `special_battle_run_forfeit`, a leader-first dispatch. |
| Does a special battle open the result window? | resolved (no) | `disassembly` | `FUN_8004E568` skips `FUN_801D8DE8(0x41)` at `0x8004F614` and `FUN_801D8DE8(0x42)` at `0x8004F8F0` when the word is non-zero. Port `VictorySequence::window_opened`, which gates the spoils panel both hosts draw. |
| Who writes `_DAT_8007BAC0`? | resolved (eight stores; none between the formation roll and the results) | `disassembly` | A `find-gp-relative-refs.py` sweep finds eight stores, including boot `FUN_8001D424` (`0x8001D528`) and the minigame exit `FUN_80026018` (`0x80026098`). None runs between the formation roll's raise (`0x8005205C`) and `FUN_8004E568`, so the Rim Elm ambush pays nothing ([`battle-formulas.md`](../../subsystems/battle-formulas.md#no-writer-clears-the-word-before-the-results)). No state reaches that victory, so it is not capture-confirmed. |
| What does the special-battle word `_DAT_8007BAC0` suppress, and who makes a fight unescapable? | resolved (spoils, absorb, spell XP and monster flee; the formation row's header byte) | `disassembly` | Whole-word readers: `FUN_8004E568` zeroes gold (`0x8004F0AC`) and EXP (`0x8004F274`) and skips the drop (`0x8004F480`); `FUN_8004AD80` skips the steal (`0x8004B48C`); `FUN_801DDB30` skips spell XP (`0x801DE450`); `FUN_801E91E8` answers an absorb as known (`0x801E9224`); `FUN_801E9FD4` drops a granted monster flee (`0x801EA994`, after the roll). Op `3E FF` writes no escape flag (`0x801E070C..0x801E0788`, PROT 0897); `ctx+0x287` is the row's `record[+0]` (via `0x8007BD60` bit 7). The Rim Elm ambush row carries `0` (escapable; engine: `rim_elm_ambush_disc`). |
| Why does the Tetsu spar never open on a back attack? | resolved (the battle-stage id skips the formation roll) | `disassembly` | Its `3E FF` row (`town01` row 4) carries header byte 0, but `FUN_80051D84` also skips the roll on `DAT_8007B64A` at `0x80051DB8`, which the tutorial arm sets. |
| Why do three-on-one formations sit `+13` in Z? | resolved (the per-round recentre `FUN_801DB318`) | `capture` + `disassembly` | The battle flow runs `FUN_801DB318` at every round start (`0x801D0EE4`) and on the ring's first cancel (`0x801D11E0`): it squashes an axis wider than `0x800` and subtracts the centroid `((max + min) as u32) >> 1`, which is `-13` for `z = -825 ..= 800`. Balanced formations read their authored rows. Engine `World::normalize_battle_formation` ([battle.md](../../subsystems/battle.md#stage-seats-fun_800513f0-placement-tables)). |
| What does a downed party member play? | resolved (knockdown, then entry `7`, then entry `8`) | `capture` + `disassembly` | The tag-`4` commit at HP `0` stages entry `7`, the tag-`7` commit stages `8`, and the tag-`8` commit raises `+0x1DC` bit 3; entry `8` - root speed `0`, the downed kneel - re-commits at each natural end. Three states read `+0x1D9 = +0x1DA = 8` with `+0x1DC = 8`. Engine `world::battle::clip_ladder` ([battle.md](../../subsystems/battle.md#the-commits-clip-tag-ladder)). |
| What does `FUN_8004DC68` do? | resolved (the near-camera ghost pass) | `disassembly` + `capture` | Every battle frame (`jal` at `0x80047124`) it sets `+0x8 \|= 0x83000000` (semi-transparent, blend `3`) on bodies within `dist/4` of a point on the camera's view axis and ghosts a caster's side during states `0x28..0x2E`; a RAM replica matches 277 of 279 seated slots. Engine `camera_ghost_pass`, drawn on both hosts ([battle.md](../../subsystems/battle.md#the-near-camera-ghost-pass-fun_8004dc68)). |
| Who forbids the Ra-Seru chip, and where is it read? | resolved (bit `0x200` of `_DAT_8007BAC0`: two raisers, two readers) | `disassembly` | Battle init clears a word of exactly `0x200` and raises the bit for first monster `0xAF` (`0x800519C0..0x80051A04`); the formation roll raises it for `0x3D..0x3F` on map `0x0C` / `0x15` (`0x8005200C..0x8005205C`). The ring crosses the chip out at `0x801D12DC` and refuses its arm at `0x801D1448`; no other load of the word tests `0x200` ([battle.md](../../subsystems/battle.md#the-ra-seru-forbidden-bit-of-the-special-battle-word)). |
| What do the Baka Fighter impact pair's templates spawn? | resolved (a sprite-arm flip-book and two stage meshes) | `disassembly` | Templates A (`0x801DB8FC` / `0x801DB960`) are op-`0x23` sprite-arm quads (`FUN_8002A5A4`) stepping a 16-cell flip-book; B (`0x801DBBA4` / `0x801DBBD4`) are stage TMDs 1 / 2 (`gp[+0x754]` zeroed at `0x801CF1B0`), faded through `+0x78` from `0x1000` under an additive colour word. Port `engine-core::baka_impact_fx` ([minigame-baka-fighter.md](../../subsystems/minigame-baka-fighter.md#the-round-start-cameo)). |
| What does `FUN_801F0450`'s tail do? | resolved (the Auto command's art insertion) | `disassembly` | `0x801F0B4C..0x801F1274` walks the character's art-animation bank (`0xD0` stride) from record `rand() % 5 + 0xB` and splices learned arts' arrow strings over the end of the queue the spend loop wrote, paying out of a local copy of the Spirit gauge `actor[+0x170]` - nothing is stored back. The per-arrow cost is halved under record `+0xF8 & 0x800` (`0x801F0D00`), the routine's only test of that bit. The pool arm and its tail run only for a slot whose `ctx[+0x266 + slot]` Auto flag is set (`0x801F0704`) ([`battle-action.md`](../../subsystems/battle-action.md#the-art-insertion-tail-0x801f0b4c0x801f1274)). |
| Where does the target cursor's name plaque sit? | resolved (placement record `0x29`, centred on `x = 0xE8`) | `disassembly` + `capture` | `FUN_801D5854`'s target arm (`0x801D5B08..0x801D5BAC`) measures the target's name (`jal 0x80035F04`) and seats the box at `x = 0xE8 - w/2`, pulled back to `0x130 - w` past `0x130`, row `162`; the live seat starts at `max(x + 0x80, 0x148)` and slides in. On `party_basic_attack_vs_gobu_gobu` "Gobu Gobu" measures `55` and rests at `(205, 162)` ([`battle-action.md`](../../subsystems/battle-action.md#the-target-select-plaque-record-0x29)). |
| Which battle limbs draw dark, and how? | resolved (a party seat's Rot limbs, per mesh object) | `disassembly` | `FUN_80048A08` reloads the colour word `+0x74` and blend `+0x78` for each object (`0x80048BEC..0x80048C00`) and, for a party seat (`+0x5A < 3`), overrides both when the seated actor's `+0x16E` carries a Rot bit and the object falls in that bit's range from the five-byte row at `0x80077998 + (0x8007BD10[seat] - 1) * 5`: bit `0x08` objects `row[0]..=row[1]`, `0x10` `row[2]..=row[3]`, `0x20` from `row[4]` up ([`renderer.md`](../../subsystems/renderer.md#rotted-limbs-draw-dark)). |
| Does the tag-`0x67` streak ribbon have a caller outside the move-FX dispatcher? | resolved (yes, the per-clip pass of `FUN_8004CE2C`) | `disassembly` | Gala's tag-`0x67` arm (`0x8004D1E8..0x8004D248`) calls `FUN_801E1D98` with the target's seat vector `+0x3C` (`addiu a0,s1,0x3c` at `0x8004D220`) and the literal trail id `0xC` on every frame the clip cursor sits in `0xB0..=0xF0` ([`battle-action.md`](../../subsystems/battle-action.md)). |
| What is `ctx[+0x25]`? | resolved (the round's skip count) | `disassembly` | Actors whose turn the round passes over without acting: `0x801DAB84` clears it (`sb zero,0x25` in the delay slot of `jal FUN_801DABA4`) and the dead-slot sweep bumps it (`0x801DAC2C`). Every other `sb ...,0x25(...)` in PROT 0898 stores a GPU-packet `u` / `v` byte; the round counter is `+0x28A`. The action SM's round-end bound subtracts it ([`battle-action.md`](../../subsystems/battle-action.md)). |
| What does `0x801F696C` gate? | resolved (the strike loop's per-frame swing drift) | `disassembly` | `FUN_801EED1C` clears it at its head (`0x801EED88`); the Miracle arm (`sw` at `0x801EF5B8`, a `j` delay slot), the Super tail match (`0x801EFBD4`) and the head of `FUN_801F0450`'s auto-fill arm (`0x801F0518`, not its art tail) set it. Its one reader, `lw` at `0x801E3840`, arms the drift that moves the actor and its target along their facings each frame when the committed clip carries a header byte and the latched clip is outside `0x10..=0x1A` ([`battle-action.md`](../../subsystems/battle-action.md)). |
| How is the command-phase commit log laid out? | resolved (records `0x2B + 3n`, seated off the name's width) | `disassembly` + `capture` | `FUN_801D388C`'s commit arms land name, command and target through `FUN_801D5718`, seat them at `x = 16`, `name_w + 0x20`, `name_w + 0x60`, and scroll rows `170` / `146, 170` / `146, 170, 194`; an all-target commit logs record `0x3D` / `0x3E`. `party_basic_attack_vs_gobu_gobu` holds the one-row case ([`battle.md`](../../subsystems/battle.md#the-commit-log)). |
| What does the `0x6E` commit-confirm screen gate, and where does `Reselect` land? | resolved (party-wide, after the last able member; one step back onto the last able member) | `disassembly` + `capture` | Every commit site stores `0x6E` instead of `0x28` once `FUN_801DB81C` equals the party count, for every party size and behind no option; with nobody able, the prompt's `Begin` stores it directly (`0x801D10A0`). `Reselect` is `FUN_801D388C(0x21)` -> `FUN_801D32BC(1)` (`0x801D4750`), and the dispatcher refunds the landing member's Item (`0x801D30BC`); the `party_basic_attack_vs_gobu_gobu` display list holds the two `64 x 20` plates at `(84, 82)` / `(172, 82)`. Both hosts stage it ([`battle.md`](../../subsystems/battle.md#the-commit-confirm-screen-0x6e)). |
| What does `FUN_801DABA4`'s first loop do to a dead actor? | resolved (zero the key, clamp Spirit, count the skip, refund the Item) | `disassembly` | For a slot with `+0x14C == 0` and an unspent key: key zeroed (`0x801DABF8`), Spirit `+0x170` clamped to 100, `ctx[+0x25]` bumped, and a staged Item (`+0x1DE == 1`) refunded through `FUN_800421D4(+0x1DF, 1)` (`0x801DAC44..0x801DAC5C`). Its tie list is biased: a seat that raises the maximum is entered twice, so the first seat above 0 to reach the top key wins `2 / (ties + 2)` (`0x801DAC7C..0x801DAD60`). |
| What ends Koru's four turns, and how long is the strip up? | resolved (Koru's own AI arm; the command phase) | `disassembly` | No code compares `ctx[+0x28A]` to a bound. The AI switch at `0x801CF1CC` sends entry 178 to `0x801EB52C`, whose round switch (`sltiu v0,v1,5` at `0x801EB540`, table `0x801CF49C`) casts `0xA2..0xA5` on rounds 0-3 and the finisher `0xA1` on round 4. The strip is text actor 1 on the `gp+0x148` list, drained by `FUN_800355F0` at `0x801D0EB4` and through `FUN_801D99BC` (`0x801D9A24`) when a round plays out. |
| What does battle state `0x5A` do? | resolved (the arts target cursor) | `disassembly` | Left / Right walk `+0x1DD` through `FUN_801D8D00` (`0x801D21D4..0x801D2268`) after the arts entry; retail does not pre-pick the target ([`arts-command-gauge.md`](../../subsystems/arts-command-gauge.md)). |
| Does the arts-shout selector remember a last pick per character? | resolved (no - one byte party-wide) | `disassembly` | `FUN_8004C140` re-rolls while the draw equals `gp+0xA4A` = `0x8007BD62` (`beq` back to the `rand` call at `0x8004C3F8`), stores the pick there, then forces channel `0xC` when formation id `gp+0x9F4` is `0x4F` (`0x8004C400..0x8004C414`). A one-entry pool whose channel is the last pick would spin. |
| What does the dome hub's first visit draw, and in what order? | resolved (course card over title art; last emitted paints first) | `disassembly` | Arms 4 / 5 call the course card `FUN_801D042C` (six corner draws from records `5 + course` and 8), arm `0x14` raises nothing on a first visit, and `FUN_801D1610` is a subtractive `POLY_G4` (tpage `0x46`, ABR 2). Every hub emitter links at OT slot 3 through the LIFO `FUN_8003D2C4`, so the last packet emitted is drawn first, and retail clears record 4's semi byte before each face (`sb zero,0x176b` at `0x801CFAF8`) ([`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md)). |
| What triggers retail's in-battle steal? | resolved (the killing blow, once per strike chain) | `disassembly` + `capture` | Not a command and not a hit: `FUN_8004AD80`'s death-spoils arm (`0x8004B29C..0x8004B65C`) runs when a monster's knockdown clip ends at HP 0, and sets the latch `ctx[+0x27]` at `0x8004B3E4` **before** testing the killer, so an action's first kill spends its attempt. The action SM re-arms it at every strike chain's exit (`sb zero, 0x16(s5)` at `0x801E3A84`, `s5 = ctx + 0x11`). The gates, the roll (doubled by Items Up), the caption record and the table's two readers are on [`steal-table.md`](../../formats/steal-table.md). |
| Does the same arm do anything else? | resolved (it returns a thief's loot) | `disassembly` | When a monster that stole dies, the item goes back to the bag with a "took" (`0x80077A38`) or "recovered" (`0x80077A4C`) caption, or "recovered all" (`0x80077A70`) when `ctx[+0x18]` already reads `0x5B`. PROT 0941 fills the stolen band at `0x801C8FE0`. |
| How does the command ring step back to an earlier member? | resolved (cancel with a non-zero step counter) | `disassembly` | The ring's cancel tests `ctx[+0x1F]` at `0x801D11B4` before the direction arms: zero returns to Begin / Run, otherwise `FUN_801D388C(0x10)` calls `FUN_801D32BC(1)` at `0x801D4010` and refunds an Item commit (`0x801D12AC`). Case `0x21` is the Reselect on the `0x6E` commit-confirm screen (`0x801D3040..0x801D3088`). |
| What do the first-visit dome hub arms draw? | resolved (course card, title art, backdrop ramp) | `disassembly` + `capture` | Arms 4 / 5 draw the course card `FUN_801D042C` over the title art; arm 2 raises the backdrop level by `4 * dt` while the intro card fades; arm 6 drains it and kicks the battle load (`0x801CF9E4`, `0x801CFC48`). The ROUND banner `FUN_801D02F0` is drawn by arm `0x15` only. |
| How many writers does `ctx[+0x28B]` have? | resolved (five) | `disassembly` | The four SCUS raises plus the tick's own clear, `sb zero, 0x28B(v0)` at `0x801E263C` in `FUN_801E2524` (PROT 0898). Only the MIRACLE raise (`0x8004B7D0`) and the side-array raise (`0x8004B840..0x8004B868`) fire the fixed cue; the HYPER default matches the art byte `actor[+0x1DF + ctx[+0x15]]` against `0x1C..0x1E` and coin-flips through `jal 0x80056798` at `0x8004B91C`. |
| Which stat does each damage wrapper read? | resolved (`801DD4B0` INT, `801DD6B4` ATK) | `disassembly` | `FUN_801DD4B0` reads `+0x168` and `FUN_801DD6B4` reads `+0x158`. `FUN_801E7320`'s `+3` is a constant, and its modulus is the seated monster count, not the actor table's length. |
| What actually breaks a rebuilt PROT 0874 container at battle load? | resolved (a truncated pack) | `disassembly` + `capture` | `FUN_8001E890` does check integrity in its own frame, on one arm of three: with `gp+0x6AC` at `2` it reads the raw container back out of VRAM (`0x8005842C`, four rects from `(0x180, 0)`), re-sums every word at `0x8001E9C4..0x8001E9F4` against the boot sum at `gp+0x6B8`, and reloads on a mismatch. That guard is on the decompress **source**; the register-only arm (`bne v1, v0` at `0x8001E974`) skips it, so the gate's writers stay the reason nothing walks a clobbered pack. The "header size word" and the "decoded length" are one descriptor field, so a rebuild cannot vary one alone. [`character-mesh.md`](../../formats/character-mesh.md). |
| What is `FUN_801F44A0`? | resolved (the floating damage-number ring push) | `disassembly` | `(value_i16, seat_u8)`: value at `ctx[+0x83C + cursor*4]`, seat at `ctx[+0x318 + cursor*2]`, `ctx[+0x85C + cursor*4] = 0`, then the cursor `ctx[+0x262]` bumps and folds `mod 8`, `ctx[+0x273]` counting pushes. `FUN_801E09F8` inlines the block at `0x801E1898..0x801E1918`; PROT 0955's Kiss of Death calls it to show its literal `1`. Not the Point Card clamp. |
| Which entry does the battle loader's VDF registration walk? | resolved (raw TOC 874 = extraction 872, `vdf`) | `disassembly` | It reads raw entries `873`+`874` (`etmd`+`vdf`) in one contiguous transfer, keeps the **second** half's base in `_DAT_8007B878`, and walks a flat `[u32 count][u32 offsets[count]]` pack there, handing each `base + offsets[i]` to `FUN_8001FBCC` at `0x80052584`. Its four constants `0x368..0x36B` are raw TOC `872..875` = extraction `870..873`, the whole `befect_data` block. The player-character pack is extraction 874 = raw `0x36C`, a *different* entry. Body in [`battle.md`](../../subsystems/battle.md#battle-scene-loader-fun_800520f0). |
| What makes a Ra-Seru chip render? | resolved | `disassembly` | Three gates in order, and only the third selects a mark: the member's own chip byte `ctx[+0x25F+member]`, then `+0x16E & 0x1000`, then the special-battle word `0x8007BAC0 & 0x200`. Three emitters draw the marks - `FUN_801DBC30` (`(0, 96)`, 64x16, CLUT `0x7704`, the red X), `FUN_801DBD04` (`(80, 96)`, 32x24, CLUT `0x770B`, the Rot stamp, over the Attack chip under `+0x16E & 0x38 == 0x38`) and `FUN_801DBEC4` (`(120, 96)`, 64x16, CLUT `0x7700`, the Curse plate, over the Ra-Seru chip under `+0x16E & 0x1000`) - all `POLY_FT4`, tag `0x09000000`, code `0x2C808080`, tpage 7, each early-out on `ctx+0x6CE`. See [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md#what-makes-a-ra-seru-chip-render). |
| Does the port draw retail's Rot / Curse marks over refused arms? | resolved (yes, on both hosts) | `disassembly` | The ring stamps Rot on the Attack chip under `+0x16E & 0x38 == 0x38` (`FUN_801DBD04(0xA0, 0x42)`) and lays the Curse plate on the Magic chip under `+0x16E & 0x1000` (`FUN_801DBEC4(0xF8, 0x42)`), `0x801D1314..0x801D1360`; the arts entry stamps each rotted direction with `FUN_801DBDDC(x, y, cost)` (`0x801D1DA8..0x801D1E54`). The texels are the `etim` page's blue "Rot" and "Curse". Both hosts draw them from `engine-vm::battle_party_panel`. No library state carries a Rot or Curse party member, so no retail frame pins the draw. See [`arts-command-gauge.md`](../../subsystems/arts-command-gauge.md#status-limb-gating). |
| What restricts a special battle's commands (`0x8007BAC0`)? | resolved | `disassembly` | Bit `0x100` bars Item, `0x200` bars magic, course = `((word - 1) & 0xFF) >> 4`. **Two** writer families: SCUS battle init keys on the first enemy monster id (`0x800519DC`; `0x8005200C` under mode `0xC`/`0x15`), and the **arena seeds the word itself**: `FUN_801CEA6C` stores `0x101` / `0x111` / `0x321` over a zero at `0x801CEBA0` / `0x801CEBB4` / `0x801CEBC8` on story flags `0x536` / `0x537` / `0x538`, last match winning - so a seeded visit bars Item on every course and magic on the top one. 13 stores over 84 images, 5 clearing. Five readers gate on non-zero; `0x100` also gates the award arms `0x801E7978` / `0x801E7B40`. |
| What does a cast cost? | resolved (MP, not AP) | `disassembly` | The cast path `0x801D1408..0x801D1528` writes the action queue `+0x1DF[0]`, sets `+0x1DE = 2` and `+0x1E7 = 9` and enters phase `0x46`; it spends no AP. The cost is the spell table's `DAT_800754C8 + id*12 + 3`, discounted by the character record's `+0xF4` ability bits `0x20` / `0x10`. See [`battle-formulas.md`](../../subsystems/battle-formulas.md). |
| `0x801E6218` - the multi-cast sweep the port was said to be missing | resolved (a latched arm, ported; no multi-cast sweep exists) | `disassembly` | The address is a **latched** arm, ported as `battle_action::done`; a five-form reference sweep over 84 images finds nothing that reaches it. See [`battle-action.md`](../../subsystems/battle-action.md). |
| Which routine sets battle status bit `0x400`? | resolved | `disassembly` | PROT 0955's Kiss of Death **miss** arm, `ori v0,v0,0x400` at `0x801F8CFC` followed by `sh v0,0x16e(s0)` (the status appliers are tabulated in [`battle-formulas.md`](../../subsystems/battle-formulas.md)). The hit arm sets the victim to exactly 1 HP and clears `0x0F80` with no wrapper. |
| PROT 0955's six no-damage tick bodies | resolved | `disassembly` | White Shield multiplies the defence pair by 3/2 idempotently from `0x801C9348[seat-3]`; Power Charge adds `x >> 2` to ATK capped at 999 (`sltiu 0x3E8` at `0x801F74D4`); Melt Spray takes 20% off ten halfwords and **underflows** `x` in `{0, 1}` to `0xFFFF` (only 2 of its 10 floor tests read a live 32-bit register - `0x801F83E0`, `0x801F8504` - the other eight re-`lhu` the store, same outcome); Void Accessories rolls one of three Goods slots off `record[+0x19B+slot]`, refunds through `FUN_800421D4` and rebuilds the bitfield with `FUN_80042558`; Kiss of Death is the row above; Terror Scream steals only the turn. Table in [`cast-module.md`](../../subsystems/cast-module.md#the-twelve-bodies-the-trampoline-map-names). |
| Which slot-B images write the actor stat block? | resolved (**eight**, not 0955's four) | `disassembly` | Eight of the 64 images write `+0x150..+0x16D`: 0940 (`0x801F78B8`), 0942 (`0x801F7D34`), 0943 (`0x801F6A04` - `0x801F69D8` there is the `0xB5` body's head table, not the writer), 0945 (`0x801F69F8`), 0954 (`0x801F6A58`), 0955's own cells, and 0925 / 0956 which touch `+0x16C` only. See [`cast-module.md`](../../subsystems/cast-module.md#the-band-has-eight-stat-block-writers-not-one). |
| How many capture trampolines are there, and what keys a tick body? | resolved (21 trampolines / 48 arms; the key is a **pair**) | `disassembly` | The band holds 21 trampolines carrying 48 `(action id -> body)` arms over 32 distinct bodies. A body VA is **not** a key: `0x801F69D8` is a tick body in six different modules, so the seam is keyed `(entry, body)`. See [`cast-module.md`](../../subsystems/cast-module.md#the-trampolines-are-their-own-port-and-one-cell-holds-six-spells). |
| What does the AI companion pick read? | resolved (it watches the wrong actor by design) | `disassembly` | `FUN_801EED1C`'s character-id-4 arm reads `actor_table[0]` and writes `actor_table[1]`. Its physical leg redraws `rand() % ctx[+1] + 3`; a monster record with `+0x1E == 2` yields a single `0x0E`, otherwise two draws of `rand() % 2 + 0x0C`. See [`battle-action.md`](../../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4). |
| What are the battle applier's 132 selectors? | resolved (116 of them are the epilogue) | `disassembly` | Jump table `0x80014FA0` holds 132 slots over 15 distinct targets; 116 point at `0x800421A8`, the shared epilogue, so there is no "stat-up / status-clear / queue-end / item slot" family above `0x0E`. The one body above `0x0E` is slot `0x82` at `0x800421A0`, the Incense window top-up `FUN_80046870` (port `top_up_cooldown`; not a brightness ramp). Decoded arms: `0x08`, `0x0A`, `0x0B..0x0D`, `0x0E`. See [`battle-formulas.md`](../../subsystems/battle-formulas.md). |
| How does a learned skill enter the displayed list? | resolved (ordered insert, not head insert) | `disassembly` | Both list writers insert in **ascending** order: the party-slot learned-Arts list at `+0x74D` (selector `0x0B`) and the displayed-skill list at `+0x186` (arm `0x80041FB4`). A head insert would have reversed both displays. |
| `FUN_80043264` - how many equipment slots does it scan? | resolved (three, not eight) | `disassembly` | The counter starts at `li v1,0x5` (`0x80043284`) and runs while `slti v1,0x8`, so it walks `char +0x19B..+0x19D` - the three accessory ("Goods") slots of the `+0x196..0x19D` block. Row on [`functions/battle.md`](../functions/battle.md). |
| What does `FUN_801E09F8`'s hit arm write? | resolved | `disassembly` | The `+0x1DC` writes are **ORs**, not stores (`|= 4` at `0x801E19D8`, `|= 1` at `0x801E1A18`); the face store carries no `+0x800`, so the victim turns **toward** the attacker; and the reaction pick has three legs. Ported as `effect_child_hit`. |
| Which routine applies a player-Seru module's magnitude? | resolved (the tick body, not the stager) | `disassembly` | The `0x801CF4EC` arms are `ctx+0x279` phase machines of 3396..7260 bytes, and the `actor+0x14C` write lives in them - in PROT 0910's case one call deeper still, in the applier `FUN_801F81DC` its tick reaches three times. The per-entry "data stager" is a different routine in the same image and carries no magnitude. Bodies in [`cast-module.md`](../../subsystems/cast-module.md#the-player-seru-bands-tick-bodies-are-code-not-data). |
| Which player-Seru modules heal, and by what formula? | resolved (**three**: Vera and Orb in the base band, Spoon in the evolved one) | `disassembly` | PROT 0905 (Vera) restores `record[+0x729 + slot] * 0x20 + 0xE0`, clamped to the `+0x14C`/`+0x14E` pair, skipped at `HP == 0` or `+0x16E & 4`, and pushed to the popup accumulator as a **negated** value at `0x801F7D0C`. PROT 0911 (Orb) restores `(magic_level << 6) + 0x1C0` over the party row `actor_table[0..ctx[+0]]`. Both unlock a status cleanse at level `>= 3` (tier chosen through `0x801F6960`). The input is the per-magic **level** byte (ids `+0x705`, levels `+0x729` off `0x80084140`, matched on `actor[+0x1DF]`). PROT 0919 (Spoon) restores `(level << 7) + 0x380` over party seats `0..3`. Everything else in the base band subtracts. |
| How does the battle AP gauge fill, and what does a level-9 heal do to it? | resolved (`+8` per action, `+32` for Spirit **instead**, damage taken, and a level-9 light heal **doubles** it) | `disassembly` | Actor `+0x170`. State `0x50` of `FUN_801E295C` (`0x801E5D60..`) subtracts the art cost, then adds the accumulator `+0x224 = 8` (`0x20` for a Spirit action), capped at 100; the defender of a hit gains `max(1, damage * 100 / maxHP)`; and cure tier `4` of Vera / Orb / Spoon (`sll` at `0x801F7F2C` / `0x801F7E18` / `0x801F839C`) doubles each cured seat's gauge under the same cap. The attacker's own hit credits it nothing. First reported by the_rabidsquirel (retail save-state testing; [every writer](../../subsystems/battle-formulas.md#the-battle-ap-gauge---every-writer)). |
| How many phase stores does a player-Seru body make? | resolved (3..10, counting both store forms) | `disassembly` | Per module: 0903 4, 0904 6, 0905 4, 0906 8, 0907 10, 0908 6, 0909 6, 0910 5, 0911 3, 0912 6, 0913 5; eight of the eleven write the terminal `0xFF` through a pre-formed pointer. `0x801F69D8` is the tick arm for **six** of the eleven (0903 / 0904 / 0905 / 0908 / 0911 / 0912). A census keyed on the literal `0x279` displacement misses every store through a saved register and reads PROT 0908 as zero ([falsified](../re-do-not-re-walk.md#battle--arts--level-up)). |
| What is actor `+0x1DC` at the cast band's reaction sites? | resolved (a bitfield) | `disassembly` | PROT 0903 and 0904 `ori` bits `4` and `1` into it; PROT 0906 and 0908 store `1` and `5`. Not a reaction counter: nothing increments it. |
| What does PROT 0907 (Nighto) roll? | resolved (a kill roll and a resist roll, and the boss case is forced) | `disassembly` + `capture` | `0x801F8534` is `rand() % 8` - zero kills, anything else confuses - and `0x801F853C` is `rand() % (0x13 - level) >= 9` for the resist. The resist is **forced**, not rolled, when `ctx[+0x287]` is non-zero *and* the victim's monster record `0x801C9348[seat-3][+0x20]` is set (`0x801F6BF0..0x801F6C24`), with an extra forced resist on `rand() % 3 == 0` for character index 3 alone. Gaza 2 reproduces it live: `ctx[+0x287] = 4`, record `+0x20 = 1`, boss untouched. |
| Which band routine allocates a battle seat? | resolved (PROT 0940's `0x50` / `0xAE`, and only that one) | `disassembly` | It claims `actor_table[ctx[+1] + 3]`, copies the monster-record pointer into `0x801C9348[seat]`, and seeds `+0x16C = 0`, `+0x1DE = 2`, `+0x1DF = 0x50`, `+0x1DD = 9` plus an HP copy. The `0xAE` arm then coin-flips one of `HP 1` / `MP 0` / `ATK 1` / `AGL 0x20` onto either the clone or the caster - the "nine stores in two passes" shape is two mutually exclusive branches, not one pass. |
| What does PROT 0941's `0x51` (enemy Steal) take? | resolved (a bag slot or an item off the steal table; no damage) | `disassembly` | Against a party victim it rejection-samples the 256-slot bag at `0x80085958` (`rand() % 0x100`, up to `0x400` draws), flooring the slot against `*(0x8007B5EA)` while `DAT_8007BD10[1] == 4`, then calls `FUN_80042310(id, 1)`. That gate is `lbu v1, 1($s5)` at `0x801F77E8`, `$s5` formed as `0x8007BD10` at `0x801F77B0`: **seat 1 holding roster character 4**, the split-bag condition - not a context field, since PROT 0941 makes no `+0x11` access at all. Against a monster victim it rolls `rand() % 100` against `0x80077828 + id*2`, the same table the player's Steal reads. The stolen id lands at `0x801C8FE0 + (seat-3)*4`, the message pointer at `0x800774AC`. |
| What does `ctx[+0]` count in a cast module? | resolved (the **party**, not the whole actor table) | `disassembly` | `0x8004B3F0` walks `DAT_8007BD10`'s party ids to form it, and `ctx[+1]` is the monster count. The `+0x1DD` group codes read against it: below 7 is one seat, `8` sweeps `0..ctx[+0]`, anything else sweeps `3..3+ctx[+1]` (PROT 0956 folds codes below 3 into the `8` case and skips the caster). |
| What escapes PROT 0906's phase-5 dispatch hole? | resolved (the stager's arm 2) | `disassembly` | The tick's arm 4 advances the phase byte into a slot its own table does not serve, and retail parks there busy until the **stager** runs: `0x801F783C..0x801F7858` is `ctx[+0x279] += 1`, reached as arm 2 of the seven-word head table at `0x801F69D8` (`7788`, `780C`, `783C`, `785C`, `7920`, `7964`, `79A8`). |
| What is PROT 0904's arm 12? | resolved (the swept quantity is an **angle**, not a radius) | `disassembly` | `ctx[+0x6D8]` accumulates `frame_delta * 8` and the arm advances when it passes `0x1000` - a full 12-bit turn - so what moves per tick is the direction of a `±0x30` cone, not the radius of a ring. Seats `3..=6` are tested against it by bearing (`FUN_80019B28`), with the unsigned form `(|d| - 0x30) < 0xFB1` admitting both ends of the circle, and `+0x1D9` is what stops a seat being hit twice as the cone comes round. Port `engine-battle-vm::cast_seru_ticks_a` (its comments call the cone the "ring sweep"). |
| What is PROT 0962's arrival predicate? | resolved (it arrives when the test reads **zero**) | `disassembly` | `bne v0,zero,<hold>` at `0x801F7C0C` and `0x801F7D08` holds the approach while `FUN_8004E2F0` returns non-zero, so a non-zero return is *not yet there*. `FUN_80050BB8` on that path is the separation nudge, not the approach step. |
| Is `ctx[+0x287]` the counter-attack byte? | resolved (**no** - it is the scripted-fight flag, derived at battle init) | `disassembly` + `capture` | `FUN_800513F0` computes it as `(DAT_8007BD60 >> 5) & 4`, i.e. bit `0x80` of the per-battle flags that `FUN_801DA51C` raises for a formation row with a non-zero `record[+0]`; the counter-attack byte is `+0x288`. Its value space is `{0, 4}` (`lbu`, `srl 5`, `andi 4`), and 9 of 96 battle states read `4`. All three `FUN_801E295C` reads gate on it, so two audio-duck arms and the attack-return arm are unreachable in an unscripted fight. |
| What gates each player-Seru arm's dwell? | resolved (measured per arm - and it is not one constant) | `capture` | Driving casts from pre-cast states gives per-arm dwell in module ticks for PROT 0903 / 0904 / 0905 / 0907 / 0908 / 0910 / 0911 and 0959. PROT 0907 holds 8 of its 16 arms identical across two fights and moves the other 7, so a single per-arm frame constant is the wrong model. `ctx[+0x6D8]` seeds 120 and drains one per tick on the player half and holds a constant 20 on the capture half. Tables in [`cast-module.md`](../../subsystems/cast-module.md#frame-gating-measured). |
| Do the Seru side-effect debuffs and the encounter boost profiles reproduce live? | resolved (yes, both, at measured magnitudes) | `capture` | A level-9 Seru debuff takes a monster's ATK from 17 to 14 on both halves - the documented 20%. Both boost profiles reproduce off a live fight: a random encounter's `[84, 17, 15, 14, 10, 30]` becomes `17 / 25 / 24 / 12`, and Gaza 2's `288 / 222 / 220` becomes `360 / 444 / 247`. Vera at level 3 restores 320 and Orb 640, matching their closed forms exactly; PROT 0908's splash reproduces `522 -> 130` (`/4`) and `541 -> 405` (`*3/4`). |
| What does the dome tally screen draw? | resolved (six rows, and the HP accumulator sits between lanes 2 and 3) | `disassembly` | The rows are `[lane0 pending, lane1 pending, lane2 pending, the HP accumulator 0x801D1AC8, lane3 pending, the running tally 0x80084440]`, at per-lane brightnesses `[0, 1, 2, 0, 3, 3]` - four steps, not six. The six rows draw through `FUN_801D1308` -> `FUN_801D050C` with the brightness argument **unclamped**; the `0x100 -> 0xFF` clamp at `0x801D08FC` belongs to `FUN_801D08EC`, the corner-anchored sibling, which no tally row calls. `FUN_801D1184` re-forms the `0x801D` base into a different register between the product and the store (`0x801D11E0` / `0x801D11F0`), so `0x801D1ACC` is the round lane. |
| What does the dome arena seed `0x8007BAC0` with before any story flag matches? | resolved (`1`) | `disassembly` | `sw $s2` at `0x801CEB8C`, with `$s2` loaded `1` forty-three instructions and three `jal`s earlier. That is course 0 with no bans - a seeded word, not an untouched one. The `0x101` / `0x111` / `0x321` values, and therefore the claim that every seed carries `0x100`, belong to the three flagged arms only. |
| Is `0x801F69D8` a routine in PROT 0943? | resolved (**no** - it is a head table) | `disassembly` | The five-word table at the image base is dispatched by the body at `0x801F6A04`: `sltiu v0,v1,5` at `0x801F6A70` bounds the index, `addiu v0,v0,0x69d8` at `0x801F6A80` forms the entry, `jr v0` at `0x801F6A94` takes it. The body runs `0x801F6A04..0x801F6EF4` and the MP-pair stores at `0x801F6D08` / `0x801F6D1C` are inside it, so citing `0x801F69D8` as the writer names the table that points at the writer. |
| Is the monster record's `+0x20` a per-monster death-immunity byte? | resolved (**no** - a double-width texture-page flag) | `disassembly` | `lbu a2,0x20(v0)` at `0x801F1D0C` hands it to `FUN_80055468`, which widens the model's VRAM rect from `0x20` to `0x40` halfwords at `0x800554E0..0x800554F4`; 37 of 186 records set it. PROT 0907 / 0908 / 0916 read the same byte as a "big model" resist proxy under the scripted-fight flag; it is not an immunity list ([falsified](../re-do-not-re-walk.md#battle--arts--level-up)). |
| Where does a slot-B module image's **highest** spawn record end? | resolved (its own move-VM program bounds it) | `disassembly` | Walk the opcode widths to a terminator and round the end up to 4, because the records are word-aligned. `HALT` outranks an armed `0x19` / `0x1B` idle loop, the walk chains `[header][program]` rather than assuming one record per pointer, and where it dies with no terminator the last maximal `0x09 0x0FFF` WAIT it stepped over bounds the record. Every image that has a highest record bounds it; the residue misses **below** or lands on the end and never terminates above it. See [`slot-b-module-layout.md`](../../formats/slot-b-module-layout.md#bounding-the-highest-record). |
| What are Enemy Steal's acceptance tests, and what does the consume touch? | resolved (a **third** test on shop price, and a window-bounded consume) | `disassembly` + `capture` | Beyond the roll and the occupied-slot test, the sampled item is accepted only when its `+2` **shop price** is non-zero, so the quest/found-only ids - 96 of the 256 - are unstealable. The consume `FUN_80042310` reads `gp+0x2D2` / `gp+0x2D4` as its first two instructions and scans only `[start, end)`, returning the `0x100` sentinel when it finds nothing; a steal outside the active window banners a success and removes nothing. `gp` is `0x8007B318` live, so `gp[+0x2D2]` is `0x8007B5EA`. |
| How large is the item bag, and what bounds a reader? | resolved (256 slots behind an **active window**) | `disassembly` | One 256-slot array at `0x80085958`, reached through five SCUS helpers that each check the window pair `gp[+0x2D2]` (start) / `gp[+0x2D4]` (end), with `gp[+0x2D6]` the count. `FUN_8004313C` is the sole writer of that pair: a page flag at `0x80084594`, a fourth-bank flag test and the party-id byte at `0x80084598` choose start `0` or `0x80` against end `0x100`, which is the "split bag" a lone character sees. A real three-member memory-card block carries items up to index 159, so a 72-slot bound drops 88 of them - that figure is a cheat page's display page, not a capacity. |
| Do the two move-`0x36` / `0x37` damage wrappers differ from the shared kernel? | resolved (the respect wrapper does not, the bypass one does) | `capture` | Driving both against the same seat, `FUN_801DD4B0` (move `0x36`, the defence-respecting wrapper) reproduces the shared kernel exactly - 251 against 251 - so nothing in a capture distinguishes it; `FUN_801DD6B4` (move `0x37`, the bypass) returns 972 where the kernel returns 311. Exactly one capture-class id per wrapper class carries a move-power record. |
| Does the spell record's `+0x00` class byte reach the action seed? | resolved (yes - it moves the fold by 21 frames) | `capture` | The action seeder's Magic arm compares the byte against `0x14`, and flipping it moves the damage fold 21 frames in a driven cast. |
| What gates each **capture-class** arm's dwell? | resolved for all fourteen trampoline arms; the drain is per arm | `capture` + `disassembly` | The countdown is drawn down by a **per-arm** multiplier of the scratchpad frame byte at `0x1F800393` - its product with `0x1F80037D`, twice it, or once it; no single product fits every arm (arms that read as constants 4 and 8 are `1x` and `2x` a byte reading 4). All fourteen are measured, one fight each (PROT 0943's `0xAB` twice), reading the single `jal 0x801F2160` site in PROT 0898 as one module tick; the last two need a caster with enough spell entries. Three arms park rather than gate and PROT 0950's arm 6 has no gate. Tables in [`cast-module.md`](../../subsystems/cast-module.md#frame-gating-measured). |
| Is the battle loader's model-pack pointer `*(gp+0x6BC)` sane at battle time? | resolved (**no** - the battle load reuses the block) | `capture` | Across the catalogued state population the pointer names the same heap block in 97 of 98 states; it is sane in 37 of 37 field states and garbage in 61 of 61 battle states. The unclamped registrar inside `FUN_8001E890` - `lw a0,0x6BC(gp)` at `0x8001EAFC`, count off `+0x00` at `0x8001EB10`, `jal 0x80026B4C` per entry at `0x8001EB4C` - is reached on all three arms of the `gp+0x6AC` fork, so nothing in its **own frame** keeps it away from such a block. The gate's writers do: over six measured routes it is never entered over one (see the registrar-entry row). |
| How does the character-pack loader size its section buffers? | resolved (from the container header, so a rebuild cannot change the decoded size) | `disassembly` | `FUN_8001ED60` takes the section-0 and section-1 buffer sizes from the container header words held at `gp+0x69C` and `gp+0x6C8`, and the LZS decode is length-driven by the descriptor. A header-byte-exact rebuild whose decoded size differs therefore gets a **truncated** pack rather than a larger one. The battle loader itself reads neither: `*0x8007B878` is the `vdf` pack and `gp+0xA8C` the `etmd` pack. |
| Is `FUN_801F6B24` one walk or two? | resolved (**two** dispatchers picked by one word) | `disassembly` | `beqz _DAT_8007BAC0` at `0x801F6BA8` selects between a 19-arm field-restore table at `0x801F6AD8` (`sltiu 0x13`) and a 12-arm `int.tim` panel-still table at `0x801F6AA8` (`sltiu 0xC`). Both start at phase 2 because SCUS's `FUN_80025358` owns states 0 and 1. An ordinary battle teardown takes the field-restore table - four 64x256 uploads at `x = 384 / 448 / 512 / 576`, one hit each - and the panel-still table records zero. Counts are residency-gated: an ungated breakpoint at the same slot-B VA also fires on whatever else occupies the slot. |
| What are PROT 1221 and 1222? | resolved (the party's ringside **reaction pair**) | `capture` + `disassembly` | Two headerless BGR555 stills, `0x28000` bytes each, uploaded as four 320x64 bands to VRAM `(384, 0)`; one is the cheering party and the other the dejected one, which is what the loader's index rule encodes - the variant is chosen from the lead character's live HP (`u16 0x8008480E < u16 0x80084824 >> 1`). See [`ringside-still.md`](../../formats/ringside-still.md). |
| Where does a retail value-readout quad put its far corner? | resolved (**inclusively**) | `capture` | Retail's own readout quads name the last texel, not one past it: 31 against 31, 47 against 47, 55 against 55 over the sampled frames. An exclusive corner draws every quad one texel wide; with the inclusive corner the digit gap is 0. |
| Is the model-pack registrar ever entered while `*(gp+0x6BC)` holds a reused block? | resolved (**no** - six routes, five entries, count 5 every time) | `capture` | Breakpointing `FUN_8001E890`'s entry, its gate, the registrar at `0x8001EAFC` and every `jal 0x80026B4C`, and write-watching both words, over a door warp, a boss fight resolving to the field, a field walk into an encounter, a cold boot into NEW GAME and a cold boot through CONTINUE into a card load: the routine is entered five times, always over `0x8014D53C`, and the registrar reads count 5 every time. The field-to-battle route enters it zero times. State `1` (register only) occurs solely with the field pack intact; states `0` and `2` always have the file read and the decompress between the gate and the registrar. |
| What is `FUN_8001E890`'s gate? | resolved (the load-state word `gp+0x6AC`, not the game mode) | `disassembly` | `lw v1,0x6ac(gp)` at `0x8001E900` forks three ways: `0` read the entry, decompress and register; `2` re-sum, decompress and register; `1` register only. Twelve writers keep the register-only arm away from a reused buffer - `FUN_80016230` zeroes the word on every mode step outside 2/3 (`0x800163B4`), PROT 0978's post-battle restore writes `0` then `2`, and the core reset, the minigame warp and the field overlay write `0`. Not `0x8007B83C`: that is `gp+0x524`, the game mode ([falsified](../re-do-not-re-walk.md#battle--arts--level-up)). |
| Why do PROT 0943's `0x40` and PROT 0944's `0x53` fault before their first tick? | resolved (they do not - the fault is SCUS walking the caster) | `capture` + `disassembly` | Both stage clip `0x0B` on the caster (`sb 0x0B,0x1DA(s1)` at `0x801F6FBC` / `0x801F758C`), and the anim commit `FUN_8004AD80` resolves a staged clip by indexing the monster record's spell-entry offset array with it (`0x8004AF08..0x8004AF18`). Gobu Gobu's array holds ten entries, so index `0x0B` reads the record's name text as a pointer, an unmapped read. Forced on a twelve-entry caster both casts complete with zero unmapped accesses, walking arms 0..4 in 1, 9, 40, 8 and 32 ticks. No monster record's magic slots name either id, so both are casts retail never performs. |
| What is battle context `+0x276`? | resolved (the side-band applier's **stage** byte) | `disassembly` | Its writers are `FUN_801DABA4` (`0x801DAD6C`), `FUN_801E295C` (`0x801E49F4`) and the applier SM `FUN_801F12D0`; no tutorial routine writes it. Both CD-XA cue arms test it, and the summon modules poll it before raising their own head cue, so it is open by construction at the moment a cast wants it. It is not a tutorial flag; feeding it from one silences the melee sting in those battles. |
| What does `FUN_8003DE7C(1)` count? | resolved (vsyncs left in the CD read span) | `disassembly` | `gp+0x91C` is an `i32` the routine drains by `DAT_1F800393` per call and floors at zero (`0x8003DF24..0x8003DF34`), answering "busy" while it is positive. Every XA clip therefore leaves the drive nominally busy for its own duration, which is what a cue arm polling for an idle read is waiting out. Engine mirror `AudioState::battle_xa_busy_frames`. |
| What does the dance count-in banner draw? | resolved (a sprite record, 160 x 32) | `disassembly` | Record 0 of the 20-byte HUD table at `0x801D46CC` - texel seat `(0x48, 0x90)`, texel cell `0xA0` x `0x20`, CBA `0x7D0A` - seated by `FUN_801D2F38`, which halves the cell at the caller's `0x1000` unit scale (`(w * scale) >> 13` at `0x801D3180..0x801D319C`) and centres the result on the seat. Reading `+0x0A` as half-extents doubles it to 320 x 64. It is not text, and its animator samples once per three vsyncs. |
| Does any item reach the Point Card strike (effect class `14`)? | resolved (no - reachable code over unreachable data) | `disassembly` | Arm `14` at `0x8004209C` takes `min(_DAT_800845B4, 9999)` off the Point Card bank and applies it as battle damage, and decoding all 256 item records against the effect table finds no item carrying class `14`. Unused content rather than a missing item; only a forced bank reaches it. [`item-effect-table.md`](../../formats/item-effect-table.md) |
| What does an arts book write, and to whom? | resolved (the **class** picks the character, the **tier** is the art id) | `disassembly` | `FUN_800402F4`'s arm at `0x80041FB4` turns the class into a roster slot with `addiu v1,v1,-0xb` at `0x80041FC0`, so classes `11`/`12`/`13` are Vahn / Noa / Gala and the ally the player picked is never read; the tier byte is stored verbatim as the learned id by `sb s6,0x74e(a0)` at `0x80042064`, into the sorted position the loop above it opens. The nine book records carry Fire and Thunder `I`/`II`/`III` = `3`/`2`/`1` and Wind = `5`/`4`/`1`, which is a per-character art-id space and not a book level. [`item-effect-table.md`](../../formats/item-effect-table.md#arts-books-class-111213-the-tier-is-an-art-id) |
| Does retail bound the sprite-stack append its fog sheets use? | resolved (no bounds check) | `disassembly` | `FUN_8001FA68` is an eight-instruction append whose store sits in the `jr ra` delay slot; its caller loads a capacity into `a2` (`lh a2,2(a0)` at `0x8003F7FC`) that the callee never reads. Port `cutscene::sprite_stack_push`; `scus_core_helpers::list_append_u16` is the same address. |
| Which battle-action state owns the only arm that reaches `FUN_801F3990`? | resolved (state `0x3D`, and an **item** is the door) | `disassembly` + `capture` | One reference disc-wide, the `jal` at `0x801E3E04`, inside action-SM table slot `0x3D` (base `0x801CED44`, the arm's word at `0x801CEE38`) - the Spirit / Item wait state, entered only from `0x3C`, whose `sb v0, 7(v1)` at `0x801E3B60` is unconditional: the arm's one branch, `sltiu v0, v0, 3` at `0x801E3B28`, rejoins at `0x801E3B40`, above it. The Magic arm `0x801E2EB0` reaches `0x3C` only for a class byte `< 0x14` **and** a spell id `< 0x65` (`sltiu` at `0x801E2EF4`), which the player Seru block cannot satisfy. [details](../../subsystems/battle-action.md#the-one-caller-is-state-0x3d-and-it-is-an-item--spirit-state) |
| What writes the slot-B stage selector `_DAT_8007B64A`? | resolved (the field entity tick, off system flag `0x19`) | `disassembly` | A `gp`-relative sweep finds 14 accesses - 7 stores, 5 loads - where an absolute-address scan finds none. The value writer is the field entity tick `FUN_801DA51C`: it clears the byte at `0x801DA69C` (in the delay slot of a `jal 0x8003CE64`) and raises `1` at `0x801DA6A8` when system flag `0x19` is set, which the consumer then reads back. `1` pages extraction 967, the battle tutorial; `0` skips the load (`beqz` at `0x80052688`). Battle latches `3` at `0x801E6D2C`, right after `FUN_8003EC70(0x4A, 0)` pages 969 directly. Zeroing sites: SCUS `0x80046E74` / `0x800557AC` / `0x80056114`, field `0x801DA69C`, and both stage images at `0x801F7120`. |
| Which extraction entry does the slot-B pager's argument name? | resolved (`selector + 966`) | `disassembly` + `capture` | `FUN_8003EC70` computes `a0 + 0x381` in **raw TOC** space, and raw is extraction `+ 2` (measured across every TOC entry), so the pager's own index is extraction `a0 + 895`. The one site that hands it the stage selector, `0x8005269C`, passes `selector + 0x47`, so selector `n` pages extraction `n + 966`: `1 -> 967`, `2 -> 968`, `3 -> 969`, and `0` skips the load. The same algebra reproduces the band's other two call sites, `+0x28 -> 935 + i` and `-0x79 -> 903..913`. |
| What do PROT 0968 and 0969 do? | resolved (the two boss-stage modules, one function each) | `disassembly` | Neither is a cast module: none of the three PROT 0898 entry tables names them, and only the pager loads them. Each is one function over its whole image, a phase machine on `ctx[+0x289]` driving the first monster seat. 0968 (`FUN_801F69F4`, seven phases behind the table at `0x801F69D8`) is the arrival staging - camera walk, cue `0x20A`, banner, then a hand-back clearing the stage id at `0x801F7120`. 0969 (`FUN_801F69D8`, four phases, no head table) is the form transition - cue `0x20B`, the seat's HP `+0x14C = 1`, a two-position camera shake, then `FUN_8003ED04(0)`. [details](../../subsystems/battle.md#what-the-two-boss-stage-modules-do-overlays-968--969) |
| What is the `!= 4` term in the battle selectable scans? | resolved (a **seat**, not an action state) | `disassembly` | `DAT_8007BD10` is the per-slot roster character id: `FUN_801DA34C` indexes it and subtracts one to reach a character record (`0x801DA37C..0x801DA3B0`). `4` is the AI-companion seat, so the scans are excluding a seat the player does not command. It is not an action-state byte, and no "removed / done" state `4` exists ([falsified](../re-do-not-re-walk.md#battle--arts--level-up)). |
| What is the auto-command string pair at record `+0x1A7` / `+0x1B7`? | resolved (a staged queue and its backup) | `disassembly` | Gated on `DAT_8007BD04`; the band is chosen by `sltu(+0x156, +0x154)`. The primary leg falls back to `+0x77F` when its head is empty; the secondary zero-fills with **no** fallback (`beq` at `0x801DA4CC` into `0x801DA51C`). The write-back overwrites exactly one band and is guarded on `+0x14C != 0` and `+0x1DE == 3`. The `0x5C8` save-window delta corroborates the pair: `0x76F - 0x5C8 = 0x1A7`, and the patcher's performed-Super mask `+0x75D` lands at record `+0x195`. |
| Is `FUN_801D0748` the Muscle Dome's match SM? | resolved (**no** - it is the round SM every battle runs) | `disassembly` + `capture` | It has exactly **one** `jal` across `SCUS_942.54`, every based overlay image and every raw PROT entry: `0x80047014`, inside the SCUS battle frame driver `FUN_80046A20`, with no test in front of it and the fall-through rejoining at `0x8004701C`. Three non-dome battle states enter it 350 / 313 / 255 times over 700 vsyncs each, every entry with `ra = 0x8004701C`. The dome is one of its callers' contexts; see [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md). |
| What draws the dome panel still at VRAM `(384, 0)`? | resolved (`FUN_801D00F8` in the contest hub PROT 0977) | `disassembly` + `capture` | Two `POLY_FT4` quads, tpage `0x106` over screen `(0,-20)-(192,220)` and `0x109` over `(192,-20)-(320,220)`, sampling `(384, 0)..(704, 240)`; `*(0x801D1A7C)` is broadcast into all three colour lanes, so it is a fade level and the call is skipped at zero. The emitter never materialises `384` - a textured primitive addresses VRAM through the packed `tpage` page index, `384 / 64 = 6` - so no `0x180` literal exists to find. Geometry in [`ringside-still.md`](../../formats/ringside-still.md#what-draws-it). |
| What raises the still's fork `_DAT_801D1AE0`? | resolved (the arena init, on **re-entry**) | `disassembly` + `capture` | Not the match teardown: `FUN_801CEA6C` forks on the arena word `_DAT_8007BAC0` and stores `0` to the latch at `0x801CEB54` on a first entry (op `0x3E` leaves the word clear) and `1` at `0x801CEC04` on every later one. A 3600-vsync walk into the arena enters the emitter 79 times and takes the six-tile arm every time. The natural arm: the checkpoint RAM of a three-visit dome run reads latch `1`, hub arm `0x0A` and level `8` at the second and third visits, with both still packets (`0x2C080808`) in the primitive pool. [`ringside-still.md`](../../formats/ringside-still.md#on-a-natural-re-entry) |
| Who raises the Arts banner selector `ctx[+0x28B]`? | resolved (SCUS `FUN_8004AD80`, in sequence, not as alternatives) | `disassembly` | Every `sb ...,0x28b` on the disc is in `FUN_8004AD80`: the raises at `0x8004B774` (`3`), `0x8004B80C` and `0x8004B87C` (`2`), all in the `actor[+0x1DA] == 0x1A` SpecialStarter arm - and `0x8004ADDC` adds `4` to a live one for the matching seat. `0x8004B774` falls through to `0x8004B7D8`, whose read of the side array `0x801F6990` (`0x8004B804`) is stored over the `3`. The four positions are `1` NEW (build-starter mark), `2` HYPER (default), `3` MIRACLE (`ctx[+0x28D + slot]`), `4` SUPER (super-starter mark). [`battle-action.md`](../../subsystems/battle-action.md#the-raiser-and-why-its-three-writes-are-not-alternatives) |
| Can a RAM clobber of the resident PROT 0874 container trip `FUN_8001E890`'s checksum? | resolved (**no** - the sum is over VRAM) | `capture` | Forcing `gp+0x6AC = 2` reaches the sum arm; a genuine mismatch enters `0x8001EA08` and self-heals in about 45 vsyncs (clear the gate, `j 0x8001E900`, CD re-read, `gp+0x6AC := 1` at `0x8001EB0C`). Four XOR'd words of the resident container left the sum byte-identical (`0x7BF74962`), because it is taken over a `StoreImage` (`0x8005842C`) read-back of VRAM. Only VRAM at `(0x180 + 0x40i, 0)` or the boot sum at `gp+0x6B8` can break it. Probe `scripts/pcsx-redux/autorun_w5a_0874_clobber.lua`. |
| What does `FUN_801D84C0` build? | resolved (the four **battle-result messages**, not party panel labels) | `disassembly` | Its two arms copy (`FUN_8003CA78`) and append (`FUN_8003CAC4`) pool strings at `0x801F4C38..0x801F4CC4` in PROT 0898 into `ctx+0xA9` / `+0x129` / `+0x159` / `+0x189`: a victory line with its spoils sentence, a defeat line, and the two escape outcomes. `FUN_8003CBF8(buf, 0xC1, 1)` (`0x801D86A4` and siblings) locates the `0xC1` **name escape** - it is not a width measure - and every patch writes the **first** seat's id, so a lone lead is named and a party is the lead's team. [`live-audit-triage.md`](../../tooling/live-audit-triage.md#panel_labels-read-the-wrong-thing-out-of-the-right-bytes) |
| Where are the steal-result captions composed? | resolved (in SCUS `FUN_8004AD80`) | `disassembly` | The three `jal FUN_8003CB54` sites on the disc are all in `FUN_8004AD80` (`0x8004B2F8`, `0x8004B338`, `0x8004B60C`), each after a `FUN_8003CA78` copy of a template formed at `0x80077A38` / `0x80077A4C` / `0x80077A64`; `FUN_8003CB54` appends the `0xC2` escape pair. The caption is shown through `FUN_801D8DE8(0x5B)`. Port `engine-battle::battle_steal`, run from `World::tick_battle_animations` ([`steal-table.md`](../../formats/steal-table.md)). |
| What is `FUN_801D32BC`? | resolved (the command window's **member cursor**, not a turn order) | `disassembly` | Six `jal` sites in PROT 0898: the round reset `0x801D8910`, `FUN_801D388C`'s case `0x10` (back, `0x801D4010`), `0x11` (forward, `0x801D4128`), `0x21` (back, `0x801D4750`) and its tail pair `0x801D5690` / `0x801D56A0`. Initiative is the execution order; the command order is the slot scan. Port `engine-core::world::battle::member_step` (the backward step) and `engine-menus::battle_input`. |
| At what level does a re-entered hub draw the ringside still? | resolved (`*(0x801D1A7C)` across six hub arms) | `disassembly` | The re-entry init seeds hub arm `0x0A` at `0x801CEE2C` and zeroes the level at `0x801CECD0`. Arm `0x0A` raises it `4 dt` to `0x80`; `0x0B` lowers it `2 dt` to `0x40` while tally lane 0 sits at its clamp; `0x0C` holds; `0x14` raises it `4 dt` to `0x80` on the latch-`1` arm only; `0x15` holds under the ROUND banner; `0x16` drains it `2 dt` to `0`. Dispatch is the hub's 51-entry table at `0x801CE990`. [`ringside-still.md`](../../formats/ringside-still.md#on-a-natural-re-entry) |
| What does the victory drop roll do? | resolved (at most one item per battle) | `disassembly` | `FUN_8004E568` `0x8004F3D8..0x8004F5A0`: for every enemy seat, `rand() % 100` against record `+0x49`, plus `30` when a living member has Items Up (`+0x6C0 & 0x20000`); the **last** winning seat's `+0x48` decides, even a zero one, and a seat whose actor has `+0x227` set is skipped. `_DAT_8007BAC0 != 0` skips the loop. Unless a seat reads 100 or Items Up applies, a `rand() & 3` must then be `0` (`0x8004F584`), and nothing is granted at 99 held ([`battle-formulas.md`](../../subsystems/battle-formulas.md#the-victory-drop-roll)). |
| How does the victory pose pick a party slot? | resolved (a rejection loop) | `disassembly` | `0x801E66A4..0x801E6724` repeats `rand() % party_count` until the slot is eligible; a uniform pick among eligible slots spends the same draws on a different slot. |
| What is `FUN_801E91E8`? | resolved (the already-learned-Seru check of the killing-blow absorb) | `disassembly` | 0898 `0x801E91E8..0x801E92D8`: the 1-based position of Seru `id` in the acting character's learned-spell list (`0x80084140 + char * 0x414 + 0x704`, compared as `id - 0x80`), answering `1` when the slot is `>= 3`, carries no Ra-Seru (`ctx[+0x25F + slot]`) or `_DAT_8007BAC0` is set. Its one caller is the absorb in `FUN_801EC3E4` ([`battle.md`](../../subsystems/battle.md#the-retail-capture-roll-fun_801ec3e4)). |
| What does `ctx[+0x269]` hold? | resolved (the Seru the killing blow absorbed) | `disassembly` | The Done band reads it at `0x801E6224` and teaches it through `FUN_801E92DC` at `0x801E6234` in the same action - the absorb's grant, not a route into the capture cinematic `0x68..0x6B` ([`battle.md`](../../subsystems/battle.md#the-retail-capture-roll-fun_801ec3e4)). |
| What is state `0x3E`'s class-5 arm? | resolved (the gauge-extension item's presentation, not a Spirit arm) | `disassembly` | `0x801E3E80..0x801E4018`, taken when the committed effect class is `5` (the item class that extends the gauge for one battle): raises HUD elements `0x0F` / `0x52`, draws `(rand() % 2) * 2` for the camera variant (`jal 0x80056798` at `0x801E3F2C`), stages `min(0x120, base * 7 / 5 + 8)` and the actor's spirit `+8`, or `+10` with ability bit `0x200`, capped at `100`. |
| Which monsters take the `0x19` short step? | resolved (every record without a tag-`0x20` action) | `disassembly` + disc census | The approach runs a tag search over the monster's installed action table; 180 of the 186 records carry no tag-`0x20` entry and take `0x19`, and the six that do also carry `0x21` (monster 73 at entries 10 / 11) ([`battle-action.md`](../../subsystems/battle-action.md#the-stale-field-0x1dc-bit-2-the-exit-to-idle-anim-event-flag)). |
| What is action tag `0x22`? | resolved (a knockout taunt) | `disassembly` | `0x801E5594..0x801E5658`: a monster whose attack left its target at zero HP stages its tag-`0x22` entry (`+0x1DC \|= 2`) while a party member still stands, so the wiping blow plays none; 166 of 186 records carry no `0x22` entry ([`battle-action.md`](../../subsystems/battle-action.md#the-stale-field-0x1dc-bit-2-the-exit-to-idle-anim-event-flag)). |
| What does selector 8 (Antidote) clear? | resolved (both poison bits, and it skips a dead target) | `disassembly` | `0x80041BCC` / `0x80041C04` in `FUN_800402F4`'s selector-8 arm: the status word is masked with `0xFFFC` and a target at zero HP is skipped. Port `engine-battle-vm::battle_action::effect_selector`. |
| How often does battle action state `0x00` run? | resolved (every round) | `disassembly` | Begin sets the flow word to `0xFE` (`0x801D31AC`), and that arm writes `ctx[7] = 0` (`0x801D3224`), so state `0x00` - the formation arm and `FUN_801F0450`'s pool arm with it - re-runs at each round's start. `FUN_801DB318` is flow cases 0 and 2 (`0x801D0EE4`), so it too runs per round. |
| When does the commit log glide off the ring? | resolved (on a member leaving or returning, never at round start) | `disassembly` | The flow SM's tail table at `0x801CE948` runs `FUN_801D5778`'s copy loops for steps `5`, `7`, `9`, `0x2A` and `0x30` (slide out, mode 0) and `0x2B`, `0x31` and `8` (slide in, mode 1); Begin (`0x24` / `0x29`) just exits. The glide is linear in `FUN_801D9BBC` over `ctx[+0x1C]`, set to `0x10` at `0x801D8904`. Engine: `battle_commit_log::LogLaunch`. |
| How does the battle-intro banner name a repeated monster? | resolved (drop the last character, then append `* N`) | `disassembly` | For a repeated species the label loses its final character (the instance letter) and `strcat` (`0x801D9E60`) appends the rodata string at `0x801CECA8`, giving `Killer Bee * 3`. The banner is skipped when the first monster id is `0xB5` (`0x801D0DF0`). Engine: `battle_hud::battle_intro_names`. |
| What is Swordie's slash routine `FUN_801F81DC`? | resolved (a four-slot progress timer, not a one-shot hit) | `disassembly` + `capture` | Each slash keeps a progress word at `0x801F8D9C + 4*i` that grows by `rate * speed` per call and lands its hit when it reaches `speed << 6`; arm 9 starts slash `i` once `ctx[+0x6D8]` passes `(i + 2) * speed * 16`. At four vsyncs per tick the timers reproduce the measured dwell (arm 7 sixty-five ticks, arm 9 twenty-five). Engine: `cast_seru_ticks_b::swordie_slash_step`. |
| Does anything read the second 8x8 percentage block after the element-affinity matrix (PROT 0898 `0x801F5428`)? | resolved (no; dead data in USA, PAL and Japanese builds) | `disassembly` | Both address scans over every image (`--prot`) find no reference of any form to any of its 64 bytes. The matrix base `0x801F53E8` is formed at eight `lui` sites, including the Seru side-effect stager `FUN_801F3D3C` at `0x801F3E4C`, each indexing `atk * 8 + def` unchecked; no character, monster or summon cast element exceeds `7`, so no index reaches the block ([`battle-formulas.md`](../../subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8)). |
| Encounter MAN sub-section layout | resolved (header shape corrected) | `disassembly` | [details ↓](#encounter-man-sub-section-layout) |
| Battle-intro tile shatter - the side-face shade page | resolved (a resident field asset, not a transition upload) | `capture` | [details ↓](#battle-intro-tile-shatter---the-side-face-shade-page) |
| How long does a field-to-battle transition run (`DAT_801D2458`)? | resolved (132 frames; 252 for the swirl) | `disassembly` | [details ↓](#battle-intro-transition-length---dat_801d2458) |
| Which stage-dome objects does the battle backdrop draw? | resolved (drop index 1, not "keep index 0") | `disassembly` + `capture` | The registration edits the object list rather than truncating it: each backdrop actor owns a private `0x9c` part table at `+0x44` (allocated at `0x80021184`), and `0x80051ad4..0x80051bac` applies one `count -= 1` plus one `entry[i] = entry[i+1]` shift from index 1 to each. Object **1** is dropped and everything else kept, gated on `_DAT_8007b64b == 0`. Indistinguishable from "draw object 0" on the two-object shells; on the seven four-object domes it keeps sky, mountains and the ground ring. See [battle.md](../../subsystems/battle.md#object-1-is-dropped). |
| Battle ground grid depth cue - the far colour | resolved (captured at the draw; port fogs the grid) | `capture` | [details ↓](#battle-ground-grid-depth-cue---the-far-colour) |
| Which element the screen-element `+0x0E` **kind pair** selects, per value | resolved (it is a table index, not a style enum) | `disassembly` + `capture` | [details ↓](#the-chrome-kind-byte-is-an-index-into-the-widget-class-table) |
| The element-**badge** palette selector | resolved (the badge record's own palette byte) | `disassembly` + `capture` | [details ↓](#the-element-badge-palette-selector) |
| The status-element badge sheet `0x18..=0x20` | resolved (nine 48x16 word tags; ladder assignment independently confirmed) | `disassembly` + `capture` | [details ↓](#the-status-element-badge-sheet-0x180x20) |
| Does the battle ground grid roll per-cell randomness? | resolved (no - a four-entry table walk) | `disassembly` | No. `func_0x801d02c0` builds sixteen literal UV words into scratchpad `0x1f800034` (`0x801d0304..0x801d03a0`) and the emit loop reads group `n` for quad `n`, advancing `0x10` each time. They decode to four fixed 32x32 sub-tiles of the `(192..=255)^2` window walked in `sub_row * 2 + sub_col` order, copied into the packet verbatim - no roll, no corner mirror. The grid origin also carries an extra `-0x200` bias on `z`, and pass 1's cull is a view-`z` bracket with **no** screen-space term (that is a separate pass-2 test). See [battle.md](../../subsystems/battle.md#the-grids-own-constants-read-off-the-emitter). |
| Endless camera orbit (Gaza 2 softlock) - the `0x19` attack-approach park | resolved (caught live; root-caused; disc fix shipped) | `capture` + `disassembly` | [details ↓](#endless-camera-orbit---the-0x19-attack-approach-park) |
| Which `FUN_801D5854` case-6 arm a running fight takes, and where `ctx[+0x6DA]` is seeded | resolved | `capture` + `disassembly` | [details ↓](#the-in-fight-action-framing-and-the-yaw-counter-ladder) |
| Which framing the Done band (`0x50` / `0x51`) takes, and the idle-orbit rate at the Begin/Run prompt | resolved | `capture` + `disassembly` | [details ↓](#the-done-band-framing-and-the-two-orbit-writers) |
| `0x19` fallback approach drive - which anim-driver field does summon staging leave stale? | resolved (pinned + causally reproduced on the parked save) | `capture` + `disassembly` | [details ↓](#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2) |
| Super / Miracle Arts trigger chain | resolved (all 15 Supers live-executed) | `disassembly` + `capture` | [details ↓](#super--miracle-arts-trigger-chain) |
| Xain "Bloody Horns"/"Terio Punch" ignore elemental guards (community mystery) | resolved | `disassembly` + `capture` | Not an element drop - a **resist-ladder bypass**. Capture-class casts (spell byte `+0` = `'c'`) run per-spell modules (PROT 944..966) whose damage calls pass the caster's seat but pick one of two wrappers: `FUN_801DD4B0` (finisher `param_5=0`, resist ladder runs) or `FUN_801DD6B4` (`param_5=1`, the whole party-defender jewel/guard block is skipped). BH (952) / TP (953) use the bypass wrapper for their main hits; enemy ESM (966) uses the respecting one (hence Cort reads as Dark). Element attribution law + live confirmation: [battle-formulas.md](../../subsystems/battle-formulas.md); cast classes: [spell-table.md](../../formats/spell-table.md#cast-classes-record-byte-0). |
| First boss trigger → Battle | resolved | `disassembly` | The scripted-battle arm is the field-VM op `3E FF <formation_row>` ([battle.md](../../subsystems/battle.md#scripted-battle-entry-3e-ff-row)): Zeto = garmel `P2[12]` row 9 (lone `0x4B`), Caruban = rikuroa stager `P1[3]` row 17 (lone `0x49`, `World::run_boss_stager_record`). `DAT_8007b7fc` closed: writer-less across `SCUS_942.54` + every static overlay (validated absolute + gp-relative + address-materialisation sweep); readers pin it as the debug forced-battle formation id - battle init `FUN_80055b6c` → `FUN_8005567c` seeds the formation cells `DAT_8007BD0C+` from it, and `FUN_80046A20` routes a nonzero value to its mode-0 debug-menu exit. Retail never sets it. See [battle.md](../../subsystems/battle.md). |
| How an enemy's signature cast picks the spell id it announces | resolved | `disassembly` | `FUN_801E9FD4`'s round-counter arm, `0x801EB7C0..0x801EB81C` (PROT 0898 file `0x1CFF0..0x1CFFC`): the round byte `ctx[+0x28A]` goes through the `0xAAAAAAAB` reciprocal, the arm returns unless `% 3 == 2`, then stores that remainder to `actor[+0x1DE]` (action category), loads the seat's byte from the formation cell `0x8007BD0C + seat`, and writes `actor[+0x1DF] = monster_id - 0x29` - the literal `0x2442FFD7`, with `sb v0,0x1DF(s4)` in the next `j`'s delay slot. Formation id `162` announces spell `0x79`, `163` `0x7A`, `164` `0x7B`, in the party spell table's own id space. See [randomizer.md](../../tooling/randomizer.md#delilas-party-swap). |
| How a Delilas sibling's signature move stages its clips | resolved (walk order capture-pinned) | `capture` | The modules drive `actor[+0x1DA]` as an archive entry index; per-frame capture of natural duel playouts orders the walks. Lu (0960): `0x0E -> 0x0C -> 0x0D`, closing `0x0F`, all literals. Che (0959): `0x0A -> 0x0B`. Gi (0958): `0x0A -> 0x0B -> 0x0C -> 0x0A -> 0x0B`, closing `0x0D` - the `addiu v0,v0,-2` at `0x801F839C` is a **mid-cast rewind**, not a stray step. Sites + walks: [monster-animation.md](../../formats/monster-animation.md#a-special-attack-can-be-a-chain-of-entries); anatomy: [cast-module.md](../../subsystems/cast-module.md). |
| Is the player battle idle channel-delta encoded, like the readef "ME" bodies? | resolved (no - raw packed inline in `record[0]`) | `capture` | Each action entry in a player battle file's `record[0]` carries `[u8 parts][u8 frames]` at `+0xAC` then `parts * frames` raw 9-byte TRS records - the monster archive's own packing, no codec between disc and RAM. Entries tile the block: Vahn's slot 0 is 15 parts x 9 frames ending at `0x5CD`, slot 1 opens at `0x5D0`, and so on down the table (Noa's rig is 16 parts to Vahn's and Gala's 15). `battle_party_pose_live` byte-matches the disc stream against live PSX RAM at `record0_base + stream_off`. So an idle rewritten at its exact retail length leaves every later entry offset valid and needs no relocation. |
| Enemy-ally charm battle softlock | resolved (both tracks fixed) | `disassembly` | The state-`0x5A` victory arm's party-slot assumption OOB-indexes the win-pose roster `DAT_8007BD10` (via `0x801E6770`) when a living charmed ally is the acting actor at monster-wipe victory - the `FUN_801E7320` reroll theory is falsified ([`re-do-not-re-walk.md`](../re-do-not-re-walk.md#battle--arts--level-up)). Fixed on both tracks: engine `victory_pose_fixup`/`charm_widen`, and the disc-side `legaia_patcher::charm_fix` guard - a single-word detour at the `0x801E6690` keep-branch into a SCUS dead-space liveness guard. Full chain + port: [battle.md](../../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock). |
| Battle-actor `+0x16E` bit `0x400` applier (guard-disabling status) | resolved - exhaustive negative | `disassembly` | Bit `0x400` has **no retail setter**: a word-level decode of `SCUS_942.54` + every static-overlay image (all stores covering `+0x16C..+0x171`, pointer precomputes, `ori`/`sllv` bit-set shapes, the `+0x6F6` mirror, the `+0x21F` deferral) finds only clears - accessory cure `FUN_8004CE2C`, the per-round RNG waker `FUN_801F45A4`, item cures, the on-hit strip, battle-exit. The appliers (hit leg `FUN_801EC3E4`, cast leg `FUN_801E09F8`) map kinds 3/4/5/6 → `0x1`/`0x2`/random-`0x38`/`0x1000`, kinds 1-2 → the `0x380` deferral; none reaches `0x400`. Latent content. Writer inventory: [battle.md](../../subsystems/battle.md#the-0x16e-status-halfword---retail-writer-inventory). |
| Who calls the battle on-screen test `FUN_8005126C`? | resolved - exhaustive negative | `disassembly` | [details ↓](#who-calls-the-battle-on-screen-test-fun_8005126c) |
| What spawns the battle XA seek selector `FUN_8004DA00`? | resolved | `disassembly` | Nothing calls it - it is the `+0x08` tick of the [static actor template](../functions/runtime-libs.md#static-actor-templates) at `0x800767F4`, and the battle scene-loader `FUN_800513F0` spawns that record into the system actor pool at `0x80051D3C` as its last act before returning. The selector is therefore a per-frame pass resident for the whole battle. It plays nothing: each arm ends in `FUN_8003EAE4`, a `CdlSeekL` with no read (`0x8003EB68`), parking the drive on the voice's file; the gate at `0x8004DA50` passes on `ctx[+0x276] == 0`. The port (`legaia_engine_audio::battle_voice`) is `REPLACED-BY` ahead-of-time clip decode. The tick is `+0x08` from `0x800767F4`. |
| Effect-VM pass-1 "state token algebra" (`FUN_801E0088`) | resolved + ported | `capture` | [details ↓](#effect-vm-pass-1-state-token-algebra-fun_801e0088) |
| Seru-magic summon visual (e.g. Tail Fire) | resolved (player visual; wired) | `capture` | [details ↓](#seru-magic-summon-visual-eg-tail-fire) |
| Why a disc-booted port fight never showed an enemy special | resolved (the boot catalog lacked the disc's monster-special ids) | `capture` | The picker rolls Gimard's `+0x21` Tail Fire (`0x27`, read off the `battle_gimard_tail_fire_a` RAM) on half its turns, and `take_monster_turn` discards a spell its catalog lacks: `SpellCatalog::vanilla()` has no `0x27`, and placeholder entries collide with real ids (`0x26` at 9 MP over Thunderbolt's 18). The boot catalog fills the block below `0x81` from the disc table; see [`battle-action.md`](../../subsystems/battle-action.md#the-monsters-cast-is-only-as-real-as-the-catalog). |
| The party cast trigger `FUN_801DBF9C` - outcome producer or params stager? | resolved (a params stager; every Seru id takes the summon arm) | `disassembly` | [details ↓](#the-party-cast-trigger-is-a-params-stager) |
| Player-summon presentation - label, readout, hide, flashes, creature seat | resolved (ported behind the band's own seams) | `capture` + `disassembly` | [details ↓](#player-summon-presentation) |
| Fade-actor hold word `-1` - "no hold" or "hold until killed"? | resolved (hold until the actor is killed) | `disassembly` | [details ↓](#the-fade-actors-hold-word) |
| `summon.dat` / `readef.DAT` side-band streaming | resolved (entries + format) | `disassembly` | [details ↓](#summondat--readefdat-side-band-streaming) |
| Monster steal item (Evil God Icon) | resolved | `capture` | [details ↓](#monster-steal-item-evil-god-icon) |
| Battle face-stamp issuing site | resolved | `capture` | [details ↓](#battle-face-stamp-issuing-site) |
| Per-spell magic power / multiplier | resolved (mechanism + roll ported) | `disassembly` | [details ↓](#per-spell-magic-power--multiplier) |
| Arts command sequence - independent source | resolved | `capture` | The SCUS arts-name table (`DAT_80075EC4`) glyph string is byte-exact ground truth for every art's directional command; `legaia_art::ArtsOracle` exposes it, and disc-gated contract tests validate both the best-effort PROT `0x05C4` `parse_record` command-decode and the curated gamedata `directions`/`ap` columns against it (one documented walkthrough error: Hyper Elbow). |
| Weapon-specialty arm width (off-class widens the Arms command) | resolved | `capture` | Not a runtime favored-class comparison. The arm command's AP cost is a per-(character, weapon) byte in the player battle file, at the weapon section's swing record (`section[+0x04]`) `+0x74` (favored `0x1E` / off-class `0x2A` / far `0x36`); LZS-decoded and copied verbatim into the runtime gauge (`DAT_801C9360[char][0x0C]+0x74`) at battle load by `FUN_800557B8`, read by gauge builder `FUN_801D388C` case 9. Byte-validated across all three player files; randomized by `legaia_patcher::weapon_specialty`. See [`docs/subsystems/arts-command-gauge.md`](../../subsystems/arts-command-gauge.md). |
| Stat growth-rate source | resolved (validated + wired; core + opt-in jitter) | `capture` | [details ↓](#stat-growth-rate-source) |
| Character-record HP/MP/AP pair order (`+0x104..0x110`) is `(max, cur)` | resolved (relabeled throughout) | `disassembly` | [details ↓](#character-record-hpmpap-pair-order) |
| Monster stat-record archive source | resolved | `capture` | [details ↓](#monster-stat-record-archive-source) |
| Monster mesh + texture pool | resolved | `capture` | [details ↓](#monster-mesh--texture-pool) |
| Terra slot-3 / story-flag overlap | resolved | `capture` | [details ↓](#terra-slot-3--story-flag-overlap) |
| Battle party mesh pack `other5` = **PROT 1204** (battle form; Baka Fighter reuses it) | resolved (empirical) | `capture` | [details ↓](#battle-party-meshes--assembled-from-the-player-battle-files-prot-1204--baka-fighter--default-equipment-sibling) |
| MP-cost ability-bit priority (half vs quarter) | resolved (dump-confirmed) | `disassembly` | [details ↓](#mp-cost-ability-bit-priority-half-vs-quarter) |
| Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg) | resolved | `capture` | All three inputs derive from disc bytes: the formation-row selection is the standard scripted-battle op `3E FF 04` in `P1[10]` (same case-`0x3E` install arm as Zeto/Caruban; row 4 = lone Tetsu), the sparring-partner reposition is `P1[10]`'s `4C 51 15 0E 07 22` NpcRun→tile `(21,14)` = `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` exactly, and the spar Yes/No is a MES-embedded option picker (`0x29` open + N×2 signed relative-jump table, handler `FUN_80038050`; port `legaia_mes::Picker::jump_target` + `InlineDialogueRunner::last_choice`), not a field-VM opcode. [details ↓](#scripted-tetsu-encounter--battle-v01-oracle-battle-leg) |
| Battle stage backdrop: which `scene_tmd_stream` a scene fights in | resolved | `capture` | A scene bundle carries one stage stream per sub-area, and the battle's is not uniformly the block's first - `map01` uses bundle slot 5 (entry 88), Rim Elm `town01` slot 6 (entry **7**). Engine `ProtIndex::battle_stage_entry_for_scene`. [details ↓](../../subsystems/battle.md#which-stage-stream-a-scene-fights-in) |
| Battle stage backdrop: is the authored half completed, and how | resolved (two actors; per-stage transform; object 1 dropped) | `capture` + `disassembly` | `FUN_800513F0` registers the shell TMD once and allocates **two** actors from it (`ctx+0x106C` / `+0x1070`), both drawn. Copy B takes a half turn unless the stage is on the `DAT_80078B50` table, which mirrors it in X instead. Confirmed live in 15 battle saves: both pointers non-null and distinct, object lists identical, the split matching the table every time. Not "drawn once, so nothing completes it" ([re-do-not-re-walk.md](../re-do-not-re-walk.md#the-backdrop-shell-is-drawn-once-so-no-completion-exists)). [details ↓](../../subsystems/battle.md#backdrop-shell---two-copies-of-one-mesh). |
| Battle-stage overlay band (`+0x47`) | resolved | `disassembly` | `FUN_800520F0` pages a per-stage slot-B overlay via `FUN_8003EC70(_DAT_8007B64A + 0x47)`, skipped when the id is `0` (which every catalogued battle but the Tetsu tutorial reads). Engine `engine-core::overlay_loader::battle_stage_overlay_entry`. [details ↓](../../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band) |
| Battle-intro tutorial boxes (Tetsu sparring fight) | resolved (machine pinned, ported and wired) | `disassembly` (machine exclusivity byte-anchored; prompt pool shared with 0968) | [details ↓](#tutorial-prompt-machine-exclusivity) |
| What arms the battle-stage id `1` (sparring tutorial) | resolved | `disassembly` + disc bytes | Not the formation, scene or monster: a one-shot system-flag arm. `FUN_801DA51C`'s battle-entry tail defaults the stage-id byte to `0` in a delay slot, tests flag `0x19`, and on a set flag writes `1` and clears the flag. The disc's only setter is town01's Tetsu record (`50 19`, two ops before its `3E FF` battle-entry op). Port `battle_tutorial::TUTORIAL_ARM_FLAG`. [details ↓](../../subsystems/battle.md#who-writes-stage-id-1---the-one-shot-arm-flag-0x19) |
| Mid-battle second `_DAT_8007B64A` stage-id writer (the `0x801fd514` phantom) | resolved (coordinate re-keyed) | `disassembly` | `FUN_801E6968`'s tail arm (the Lost Grail Final Heal sweep's epilogue; `sb` at `0x801E6D2C`, battle-SM cleanup state `0x50`): formation head still `0xB5` (Cort) + first monster seat dead → stage id `3` (entry 969, the form-transition module), slot-B page-in issued same-frame. Port `overlay_loader::boss_transition_stage_id` via `World::battle_stage_id`. `0x801fd514` is a phantom print off a base-tag-less dump; see [re-do-not-re-walk.md](../re-do-not-re-walk.md#a-second-stage-id-writer-at-0x801fd514-in-the-0897-band). [details ↓](../../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band) |
| "No party seated" vs "party dead" in the wipe scan | resolved (port-only state; guarded) | `disassembly` + `capture` | Retail's `0x5A` wipe scan (0898 `0x801E6510..0x801E664C`) walks the actor table for exactly the seated count at `*(0x8007BD24)` - the `beq` at `0x801E6524` falls straight into the wipe compare on a zero count, and retail has no guard, only a count that is never zero after battle load. "No party seated" is a **port-only state** (unguarded, it reads as a party wipe on the first end-of-action). `BattleActionHost::slot_seated` gates `PartyWipe` on `party_seated > 0` (`MonsterWipe` still resolves), and the pad ladders seed the retail New Game roster. Disc-free pin: `unseeded_battle_wipe_guard.rs`. [details ↓](../../subsystems/battle.md#an-unseeded-party-reads-as-a-dead-one) |
| Battle command-flow byte `ctx[+0x06]` | resolved | `disassembly` | The *other* battle SM - `FUN_801D0748`, the menu half, distinct from the action SM's `ctx[+0x07]` and overlapping its value space. Its selection band is regular decimal tens `30..120` (turn prompt / category menu / escape / item / magic / arts entry / target / target confirm / commit / attack-mode), which is what identifies it as the tutorial hook table's key: the nine live hook slots are that band minus the magic window. Engine mirror `engine-core::battle_flow`. [details ↓](../../subsystems/battle.md#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes) |
| Action-SM state `0xFF` treated as battle end by the port | resolved (path was live-reachable; port fixed) | `disassembly` | [details ↓](#action-sm-state-0xff-treated-as-battle-end-by-the-port) |
| Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer | resolved (disc writers + engine port + oracle) | `capture` | [details ↓](#spine-flag-0x142-caruban-beat--dolk-dolk2-switch-writer) |
| Spine flag `0x482` (Drake mist-wall) writer | resolved (writer-less; "direct code path" presumption falsified) | `capture` | [details ↓](#spine-flag-0x482-drake-mist-wall-writer) |
| CDNAME scene-window frame (`raw = extraction + 2`) in `Scene::load` | resolved (engine converts; misattributions corrected) | `capture` | A raw-TOC define used as an extraction index lands two entries late, dropping each block's first two retail entries and bleeding in the next block's; `Scene::load` converts. In the right frame the `.MAP` is the retail block's FIRST entry (not "two below"); "suimon == dolk2 MAN" and "rikuroa MAN = [18,70,20]" are next-block sidecars under the wrong label; "urudre1 tests 0x15E" and "0x63A has no writer" are false; "0x1BE = rikuroa Zeto gate" is geremi's arrival one-shot. Head blocks (defines 0/1, inside the TOC header rows) keep legacy windows. See [cdname.md](../../formats/cdname.md#numbering-space). |
| Motion-VM (`FUN_80038158`) bytecode carrier + flag census | resolved (carrier pinned; spine flags negative) | `capture` | The second motion VM's bytecode source is **MAN tail-section 1** (installer `FUN_8003A9D4`; parser `legaia_asset::man_motion`; layout + op table in [`motion-vm.md`](../../subsystems/motion-vm.md#the-second-motion-vm---fun_80038158)). Disc-wide op-7/op-8 census (`--motion-flag-census`): overworld walking-band choreography + one `town0b` clear; `0x142`/`0x482`/`0x1BE` and `549` appear in NO stream - the "549 set by op-7 bytecode" carrier claim is **falsified**. Anchor test `motion_flag_census_disc.rs`. |
| Debug-menu "STR trigger teleports + sets flags" mechanism | resolved (no per-FMV event table; dev-menu tools explain it) | `disassembly` | [details ↓](#the-_dat_8007ba78-census-is-closed) |
| Spawned-record player-channel (`0xF8`) ExecMove/HaltAcquire handshake | resolved (retail machine traced; the port's model diverges) | `disassembly` | [details ↓](#the-0xf8-halt-acquire-handshake) |
| Equipment stat-bonus table - slot model | resolved (slot model + passives) | `disassembly` | The stat-bonus table (`DAT_80074F68`, 8-byte stride) is decoded from `FUN_801CF650`/`FUN_801CF5D0` (`legaia_asset::equip_stats`): `+0`=INT, `+1`=ATK, `+2`=UDF, `+3`=LDF, `+4`=SPD (not AGL / evasion). Five `lbu`/add pairs at `0x801CF6C0..0x801CF72C`; note the asymmetry that rules out a linearised-C reading - `equip+0` lands on the *last* accumulator, out of sequence with `+1..+4`. AGL takes no equipment add at all. The four `+7` categories are Legaia's four weapon/armour slots (body/head/footwear exact by name; none of the 77 accessories appear in this table). Wired: `DiscEquipInfo` gates `EquipSession`'s per-character list. |
| Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer | resolved (script writers exist; the "no writer" premise was the CDNAME +2 skew) | `capture` | [details ↓](#flag-0x63a---the-vellvozz-p27-gate-with-no-script-writer) |
| cave01 `P2[16]` (the `0x15D` entry-key setter) - what spawns it | resolved (slot-counted spawn chain) | `capture` | [details ↓](#cave01-p216-spawner---the-slot-counted-interact-chain) |
| Drake Castle deep interiors (`jouinc`/`jouind`) depth decode | resolved (door-choreography families, not story gates) | `capture` | [details ↓](#drake-castle-deep-interiors-jouincjouind-depth-decode) |
| `scene_destinations` P1-table scan misses P2-only door names | resolved (P2 pass folded in) | `capture` | The P2-only class is the town/dungeon **exit door** (a P2 door-choreography record): `town01`→`map01` (Rim Elm's overworld exit; the P1 pass alone sees *zero* town01 destinations), `retockin`→`retona`, `geremi`→`map02`/`tower` - 13 scenes / 14 destinations disc-wide. `jouinb`→`jouina` is not in the class: it is P1-visible (the over-walk resyncs across that record). Merged kernel `legaia_asset::man_edit::scene_destinations` (P1 pass as prefix + clean-gated P2 pass, `(name, index)` dedupe); the engine delegates to it; disc pins `scene_destinations_p2_disc.rs`. |
| `0x4C 0x51` byte `+3` = `[bit7 special-model \| facing nibble]` vs the glide-speed interim `depth & 7` reading | resolved (facing wins; the two readings were two different ops) | `disassembly` | [details ↓](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis) |
| How an NPC's facing changes **after** spawn - snap vs ramp, and which writer wins | resolved (two laws; order-of-execution priority) | `disassembly` | [details ↓](#npc-dynamic-facing---two-laws-and-an-execution-order) |
| dolk2/rikuroa MAN source (the "v12-embedded MAN" was an over-read) | resolved (streaming carrier) | `capture` | Their own `base+3` bundles are the MAN-less count=4 form `[1,2,6,0x14]`; the "embedded MAN at 0x1000" inside their SceneV12Table entries is an over-read onto the next scene's bundle (suimon's / geremi's; [scene-v12-table.md](../../formats/scene-v12-table.md) § over-read). Retail sources their partition scripts from the block's standalone `data_field_streaming` entry's type-3 chunk (`dolk2` ext 70 `[29,73,17]`, `rikuroa` ext 157 `[13,29,64]`; live script-heap byte-match at the Caruban beat). Engine: `field_man_payload` streaming fallback (`streaming_man_payloads`) + retail-frame `Scene::load` windows; pins `v12_bundle_man_disc.rs`. |
| kor-family op-0x49 flag window `[0x138..0x13F]` - what the 8 flags gate | resolved (Uru Mais warp-pad destination memory) | `disassembly` | [details ↓](#kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker) |
| Attack-band damage pacing - who calls `FUN_801EC3E4`, what paces the hits, when the total lands | resolved (the anim tick calls it every frame; the clip's `+0x10..+0x13` beats fire the hits; one HP write per combo) | `disassembly` | [details ↓](#attack-band-damage-pacing---the-hit-event-model) |
| The battle-**intro** enemy-name banner - which placement record raises it | resolved (**none does** - the composer places it itself) | `disassembly` + `capture` | [details ↓](#the-battle-intro-enemy-name-banner) |
| What does the battle overlay's private PRNG `FUN_801D0290` feed? | resolved (nothing in the battle model - it shapes one effect ribbon) | `disassembly` | All five draws sit inside `FUN_801CFA48`, the lightning effect-ribbon emitter (the `0x2000` arm of `FUN_8001ADA4`'s multi-target case; PROT 0973's dev harness labels it `THERNDER1`): inner / outer half-widths, a 1-in-8 heading kink, the per-segment turn and the advance length. The state word `0x801F6950` is re-seeded on every call at `0x801CFC18` from `param[+0x1C] >> 2`, so it is a shape hash, not a stream. `0x801CFB94` is a branch label inside that routine, not a function (the name came from a slot-A VA collision with a real entry in PROT 0970). |
| Is `0x801F6950` read across overlays (the `overlay_0897_*` dumps)? | resolved (no - three machine references on the disc, all in PROT 0898) | `disassembly` | The field overlay's content ends at `0x801F3818`, below the word. Three of the four `overlay_0897_*` hits are the address appearing as an *instruction* address in a mis-based print; the fourth is the seed store re-keyed by `0x167E8` into `FUN_801CFA48`. |
| What are the halfwords of a `0x80076C10` screen-element record? | resolved (a two-seat pair over shared width / height / string) | `disassembly` | Seat A `(+0x00 id, +0x02 x, +0x04 y, +0x0E kind)` and seat B `(+0x01, +0x0A, +0x0C, +0x0F)`, shared `+0x06` width, `+0x08` height, `+0x14` **content string** (measured by `FUN_80035F04`, the rendered-width kernel - not an animation descriptor). `FUN_801D8DE8` has one spawn arm per seat (`0x801D92E8` / `0x801D935C`, by `mode & 1`) and glides toward the *other* seat. Movers: `FUN_801D5718` land, `FUN_801D5778` launch (seat B pushed `-0x140`), `FUN_801D57E8` clone. `+0x10` is `13` on the framed rows and unread; `+0x12` is zero in all 103 records. Which seat is parked is per record. |
| How is a slot-B cast / summon module entered? | resolved (two link-time tables in PROT 0898; row = extraction - 903) | `disassembly` | The 32-slot cast-tick table `0x801CF4EC` behind `FUN_801F1ED4` (`jal` per arm, key `actor[+0x1DF] - 0x81`; called from the battle SM at `0x801E4B1C` / `0x801E4C7C` / `0x801E4CA8`), and the 64-word move-VM entry table `0x801F6734` copied into `gp[+0x714]` (`0x801E44C8` / `0x801E4630`) and called by move-VM op `0x20` (`jalr` at SCUS `0x80023764`). Both give `PROT extraction = 903 + row`, verified 13/13 and 12/12 against the extracted images. Most slot-B images carry no internal `jal`, so a call-graph walk attributes no dump to them. See [`cast-module.md`](../../subsystems/cast-module.md#the-entry-tables-and-where-the-addresses-live). |
| How is a **capture-class** cast module's tick entered? | resolved (a third link-time table in PROT 0898, keyed on the spell record's sub-id) | `disassembly` | `FUN_801F2160` (0898 file `+0x23948`, sole caller `0x801E50C8` in the battle SM) reads `caster[+0x1DF]`, indexes the static spell table `0x800754C8 + id * 12`, takes the record's `+1` capture-class sub-id (`sltiu 0x20`) and jumps through 32 arms at `0x801CF56C`; arm `i` is a hard-coded `jal` into PROT `935 + i`. Most arms land on a per-spell trampoline (88..204 bytes) that re-reads `caster[+0x1DF]` and calls one body per spell id. It is not keyed `id - 0x81` like the cast-tick table. |
| Is the whole slot-B band mapped? | resolved (all 64 entries carry a `static-overlays.toml` row; 61 are dumped, and 0915 / 0926 / 0935 wait only on their extraction images) | `disassembly` | Function extents recovered by frame matching (prologue → the first `jr ra` whose delay slot restores the same frame); every `0x801F6734` row and every arm of both tick tables lands on a recovered head in its image and no other. The band holds 196 framed functions (1..8 per image), 25 images with internal `jal`s. The extraction test's pointer-resolution gate counts only references landing inside another mapped image, so references that legitimately leave an image (0898's data, the post-image `.bss`) do not hold PROT 0915 / 0926 / 0935 back. |
| The slot machine's cabinet emitter | resolved (a mesh, not a packet: PROT 1200 descriptor 1) | `disassembly` | PROT 1200 carries three descriptors - `TIM_LIST`, a 2160-byte untextured Legaia **TMD** (65 verts, 76 prims, baked greys / dark reds / navy) and a `MOVE` table. The slot init `FUN_801CEC94` loads the entry (`FUN_8003EB98(0x4B2, ..)` at `0x801CEE2C`), `FUN_80020224(0)` at `0x801CEE44` dispatches all three, and `FUN_80020DE0(0x801D3618, ..)` at `0x801CEEA8` spawns the actor bound to model slot 0. Its extents enclose the paylines, dot matrix, reels and pedestals. See [`minigame-slot-machine.md`](../../subsystems/minigame-slot-machine.md). |
| The slot machine's in-game BGM | resolved (host-scene authored, not the overlay's) | `disassembly` | `koin1` (PROT 543, Sol Tower's minigame floor, whose MAN carries the `0x3E` warps `op0 = 103 / 104 / 105`) picks the track in its entry script by spawn bbox: `2018` (M16, the casino floor), `2024` (the bar), `2045` otherwise; `balden` (the Vidna cabinet) plays `2058` / `2010`. |
| Which payline does a forced slot stop land on? | resolved (one of the five, chosen per spin by `rand % 5`) | `disassembly` | `FUN_801d2440` reads `0x801D3630[DAT_801d4134 * 16 + reel * 4]` (`0x801D2444..0x801D2464`) and stops a hit at raw row `R` at `R + 3 + word` (`0x801D24F4..0x801D2520`); the display's `+0x10` bias puts the target on the top, middle or bottom row for word `0` / `21` / `22`, and the five rows are the three horizontals and both diagonals. `DAT_801d4134` is `rand % 5` (`0x801D25A0..0x801D25D0`). The search window is raw `cur + 1 ..= cur + depth`. Port `land_row`; [`minigame-slot-machine.md`](../../subsystems/minigame-slot-machine.md#reel-landing---fun_801d2114--fun_801d2440). |
| The Muscle Dome's Auto arm - which routine picks the commands? | resolved (none on the screen; the Auto flag makes `FUN_801F0450` rebuild the queue every round) | `disassembly` | `FUN_801DA34C` (called at `0x801D15C8` on the phase-`0x28` Attack confirm) reloads the record's 16-byte command string (`+0x1A7` / `+0x1B7`), which `FUN_801DA59C` writes back and the review screen shows. The swings come from elsewhere: the Auto pick raises the per-fighter flag `ctx[+0x266 + seat]` (`0x801D17D0` / `0x801D164C`), and `FUN_801F0450` rebuilds that fighter's queue (`sb` at `0x801F0B04`) at action state `0x00`, which runs every round (Begin sets flow `0xFE`, whose arm writes `ctx[7] = 0` at `0x801D3224`). Engine: `World::run_auto_attack_pool_arms`. |
| The dome's three UI cue ids (`0x21` / `0x22` / `0x23`) | resolved (accept / highlight moved / refused-or-back) | `disassembly` | 37 `jal FUN_8004FCC8` sites in `FUN_801D0748` (15 / 7 / 15), not 34. Pinned by content: every cancel-mask (`*(0x800846D4)`) press is `0x23`, every confirm-mask (`*(0x800846D0)`) press `0x21`, and the pre-pass `0x22` sites fire only when the pressed bit differs from `ctx+0x880`. Arm boundaries come from the compare chain `0x801D0C84..0x801D0DCC`. |
| The arena backdrop's object-1 dust decal | resolved (trimmed by the SCUS battle loader unless `_DAT_8007B64B` is set) | `disassembly` + `capture` | `FUN_800513F0` spawns two backdrop actors and, when `_DAT_8007B64B` is zero (`0x80051ABC`), decrements both part counts and shifts each list down from index 1 (`0x80051AD4..0x80051BAC`). One writer on the disc, `FUN_801D9E1C` at `0x801DA0AC`. The arena leaves it clear: `autorun_w4d_dome_decal_flag.lua` reads `0x00` at the test `0x80051ACC` on the contest's one loader entry and logs no write to the byte ([`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md#object-1-is-trimmed-by-the-loader-_dat_8007b64b)). |
| `FUN_801F2410` / `FUN_801F2E10` - dome routines or shared? | resolved (neither is the dome's) | `disassembly` | `801F2410` is the cast **colour wash** - screen-wide `POLY_G4` onto `*(0x1F8003A0)` tinted `ctx[+0x27E..+0x280] * ctx[+0x27A] / 255`, called only from the cast dispatchers' epilogues (`0x801F2144`, `0x801F23F4`). `801F2E10` has no reference in SCUS or any overlay; its 11 callers are all in slot-B module PROT 0909. |
| What is per-character record `+0x131`, the second byte the new-game seed inits to 1? | resolved (nothing - it is write-only) | `disassembly` | `sb $v0, 0x6f9($s0)` at `0x800561C8` is its only writer on the disc; an opcode-decoding sweep for `lb` / `lbu` / `sb` at displacement `0x131` and at the four slots' block-relative `0x6f9` / `0xb0d` / `0xf21` / `0x1335` over SCUS + all 80 based overlay images finds no reader, and no wider access spans `+0x130..+0x131`. The magic-rank counter is a different byte, capture-pinned at `+0x9C`. |
| What does item-effect flag bit `0x40` mean? | resolved (the descriptor's target side - set = the enemy party) | `disassembly` | Read once, at `0x801D18E0` in `FUN_801D0748` (PROT 0898), forked with `andi 0x20` at `0x801D18E8` into phases `0x5B` / `0x5D` / `0x64` / `0x66` (one enemy / all enemies / one ally / all allies); the spell table's `+2` byte runs the same ladder at `0x801D1C50` / `0x801D1C58`. The five carriers are the Point Card and the two summon-flute pairs. `FUN_801D0F1C` never reads the bit (see the falsified row). |
| What is the arts-name table's `+4` halfword (`DAT_80075EC4`)? | resolved (an authored tier constant with no reader) | `disassembly` | Eight sites materialise the table base; a delta-tracked walk of every load off them reaches only `+0` / `+1` / `+2` / `+8` / `+0xC` / `+0x10`, and a five-form sweep of `0x80075EC8` finds no reference. The values are a per-art tier ladder keyed to list position (`60000` Miracle, `50000..30000` the elemental arts, `20000`, `15000..5000` by input count, `1` terminator) - neither AP nor input count. |
| What is the byte at `actor[+0x22C] + 0x80` the SFX-cue router folds into the category? | resolved (the actor's display object's sound-bank category, values `{0, 7, 8}` - not an element byte) | `disassembly` | [details ↓](#the-display-objects-sound-bank-category) |
| What moves the evolved-Cort battle off flow `ctx[+0x06] = 0x0C`? | resolved (the boss stage module hands the flow back after its own intro countdown; no input involved) | `capture` + `disassembly` | [details ↓](#the-evolved-cort-flow-park) |
| Is `_DAT_8007B64B` clear during a Muscle Dome contest? | resolved (yes - the dust decal is trimmed) | `capture` | `FUN_800513F0` entered once (`ra = 0x80046F7C`) with the byte `0x00`; at `0x80051ACC` the trim arm ran, and a write watch logged zero writes over 1800 vsyncs (`koin1`, modes `0x03 → 0x18 → 0x19 → 0x14 → 0x15`). The field handoff `FUN_801D9E1C` never runs on the arena path. Probe `autorun_w4d_dome_decal_flag.lua`. |
| Do the renderer's light-capable prim kinds 8..11 ever execute? | resolved-negative (no live light path) | `capture` | Zero hits on `0x8004409C` / `0x8004423C` / `0x80044434` / `0x800445B0` over 1111 `map03` overworld frames, 347 + 470 field frames (`map02`, `ropeway`) and ~1160 battle frames. The control had to change with the scene: a kingdom overworld never enters the SCUS prim-dispatch family at all (13 handlers armed, zero hits) - it renders through PROT 0901's eight replacements, all of which fired in the same run. Probe `autorun_w4d_light_kind_hits.lua`; [`renderer.md`](../../subsystems/renderer.md). |
| What are PROT 1221 / 1222 (`other5` / `other6`)? | resolved (`int.tim` / `int2.tim` - the Muscle Dome's two ringside panel stills) | `disassembly` | [details ↓](#dome-ringside-panel-stills-prot-1221-and-1222) |
| What do the higher readef groups' aux slots carry? | resolved (no new kind - textures, the four ME archives, or a never-staged actor record) | `disassembly` | `FUN_801F12D0` is an 8-stage machine (jump table `0x801CF4CC`): stage 2 uploads slot `base` to CLUT row 488 / page `x = 512`, stage 4 uploads `base+1` to row 490 / `x = 640`, gated at `0x801F1500` / `0x801F150C` on `base >= 0x42 \|\| 0x0C <= base <= 0x36`. That gate partitions the file exactly: textures inside it, the four ME archives below it, and in the three excluded groups (`base` `0x39` / `0x3C` / `0x3F`, actions `0x14..0x16`) an actor record byte-identical to its `base+2` twin, which the applier never stages. See [`summon-readef.md`](../../formats/summon-readef.md). |
| Is `cutscene_str` (PROT 0970)'s 123 KB code gap un-dumped code? | resolved (a zero-filled reservation) | `disassembly` | `0x801D1878..0x801F1A00` is 32,793 zero words of 32,866 - a `.bss`-shaped hole. The image's real un-dumped code is about 1.3 KB. Recorded in [`disc-coverage.md`](../../tooling/disc-coverage.md). |
| Can a scene TMD pack member be grown in place? | resolved (nothing outside the pack holds a byte offset into it - rebuild is the only cost) | `disassembly` | [details ↓](#growing-a-scene-tmd-pack-member) |
| Is any of an enemy signature cast's choreography data-driven? | resolved (partially - the spawn layer is data in the art path's own record format; the lift and camera are module code) | `disassembly` | [details ↓](#what-of-a-signature-cast-is-data) |
| How many damage-clamp shapes does the slot-B cast band use? | resolved (**three**, and one module picks per hit) | `disassembly` | Over the 83 wrapper `jal` words in the 64 images, 79 of them inside a frame-matched function of the image carrying them: **70** take shape A (cap live HP, `sltu`, kills), **seven** shape C (cap `HP - 1`, `sltu` - neither kills nor heals), **two** shape B (cap `HP - 1`, signed `slt` - the two AoE stagers). Every shape-C site is a tick body or a body a tick calls. PROT 0910's applier picks its cap from its own slash counter at `0x801F8DAC` - `HP - 1` on slashes 1..3, live HP on slash 4 - so kill capability there is per **hit**, not per module. Image by image in [`cast-module.md`](../../subsystems/cast-module.md#the-three-clamp-shapes). |
| Where does a capture-class cast get its damage constant? | resolved (baked into the module image, not read from a table) | `disassembly` | Each capture-class module carries its own power constant as an immediate in its own tick body, so there is no per-spell power row to patch and no table to randomise - editing the number means editing that image. Per-module values in [`cast-module.md`](../../subsystems/cast-module.md#the-baked-power-constants). |
| Which slot-B images touch more than one seat? | resolved (two, and both are **stagers**) | `disassembly` | PROT 0927 sweeps the enemy row addressed from `ctx[+1]`, and PROT 0966 sweeps the whole actor table from `ctx[+0]`; neither subtracts HP. They stage clips and seats and hand off to a tick that applies the damage - which is why 0927 reads as "can never kill" if only its own image is examined ([falsified](../re-do-not-re-walk.md#battle--arts--level-up)). See [`cast-module.md`](../../subsystems/cast-module.md#the-two-aoe-sweeps). |
| How do the `0x801CF56C` capture-class arms reach a spell body? | resolved (through a per-image trampoline in six of them) | `disassembly` | Arm `i` is a hard-coded `jal` into PROT `935 + i`, and in six images the landing site is a **trampoline** of 88..204 bytes that re-reads `caster[+0x1DF]` and dispatches one body per spell id - PROT 0955 is the extreme case, a 20-word jump table serving six spells from one cell. Two power constants that looked missing are register reuse: PROT 0918 carries `0x12` at `0x801F8798` and PROT 0949 carries `0xC0` at `0x801F72F8`. See [`cast-module.md`](../../subsystems/cast-module.md#the-trampolines-are-their-own-port-and-one-cell-holds-six-spells). |
| What calls `FUN_801E91E8`, and what reads its result? | resolved (one `jal`, and half the result is unconsumed) | `disassembly` | Its single call site is `0x801EE2C0` inside `FUN_801EC3E4` (PROT 0898), and the routine contains zero data words. The unconsumed half of what it returns is `ctx[+0x269]` - staged and never read on the capture leg. Row in [`functions/battle.md`](../functions/battle.md). |
| Does the battle tutorial's wait state draw anything? | resolved-negative (kind `0xD` draws nothing) | `disassembly` | The tutorial's wait record is widget kind `0xD` at `0x801F75F0`, and `FUN_80031D00` dispatches on the kind alone - kind `0xD` has no draw arm. The pause between tutorial beats is a timer with no picture. |
| What are placement-record `+0x0E`/`+0x0F` and `+0x10`? | resolved (`+0x10` is the widget **kind**, `+0x0E`/`+0x0F` the frame **style**) | `disassembly` | `+0x10` reaches `FUN_8003541C` as `a1` from `0x801D8E8C` and lands on node `+0x1C`, the kind the widget dispatcher switches on; `+0x0E`/`+0x0F` land on node `+0x1D` and thence `gp+0x14C`, which `FUN_8002C69C` reads to pick a frame style from `{0x31, 0x33, 0x34, 0x35}`. No image reads `+0x11..+0x13`. Detail in [`battle-action.md`](../../subsystems/battle-action.md#0x0e0x0f-is-the-frame-style-and-0x10-is-the-kind). |
| What is the battle attack angle `ctx[+0x6D2]`? | resolved (a bounded framing term, zeroed after the first hit) | `disassembly` | It holds `[0, 0x800]`: term `0` frames the strike face-on and the from-behind case contributes `atk / 32`. The battle SM zeroes it at `0x801EC888` once the first hit lands, so it shapes the approach and not the exchange. |
| Which spells are `0x27` and `0x81`? | resolved (`0x27` Tail Fire, `0x81` Gimard / Burning Attack) | `disassembly` | The two ids live in different spaces. `0x27` is the enemy special *Tail Fire*; `0x81` is the first player Seru-magic id, whose loader-B call `FUN_8003EC70(0x81 - 0x79)` pages extraction PROT 903. |
| Where is the Spirit AP-halving flag? | resolved (character record `+0xF8`, bit `0x800`) | `disassembly` | It is accessory passive `0x2B` (*AP Used Down*) in the persistent per-character ability bitfield, tested by the queue builder `FUN_801EED1C` at `0x801EF364` and by the status panel `FUN_801D33D8` at `0x801D4520`. Not a per-battle actor bit ([falsified](../re-do-not-re-walk.md#battle--arts--level-up)). |
| What is battle `ctx[+0xD]`? | resolved (two independent bits, not an enum) | `disassembly` | Bit 0 adds `0x800` of camera yaw; bit 1 adds `0x80` of pitch **and** drops `TR.y` by `0x100`. Nothing in either arm writes a Z angle. See [`battle-action.md`](../../subsystems/battle-action.md#the-three-movers). |
| Does an Attack x2 consume the Super Art starter? | resolved (no - the starter survives) | `disassembly` | The compare is `bne v0,a1` with `a1 = 1` at `0x801E3A4C`: the build loop marks an accepted art's starter `1` at `0x801EF788` and the Super tail-replace `FUN_801EF9E4` marks `4` at `0x801EFBA8`, so a doubled ordinary attack tests against the wrong constant and leaves the Super starter standing. See [`battle-action.md`](../../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4). |
| What are battle `ctx[+0x17]`, `ctx[+0x18]` and `ctx[+0x19]`? | resolved (a once-per-action teardown latch, a HUD element id, and a per-battle Spirit latch) | `disassembly` | `ctx[+0x17]` is the action teardown's once-per-action latch and `ctx[+0x18]` the id of the HUD element that action raised, both at `0x801E2F50`. `ctx[+0x19]` is read at exactly one site, `0x801E61CC`, which unloads HUD elements `0x0F` / `0x52` - so it latches "Spirit was used this battle" and nothing else. |
| What is battle `ctx[+0x270]`? | resolved (`FUN_801D5854`'s second camera ramp) | `disassembly` | Written over `0x801D5960..0x801D59B8` and read by one consumer, case 8's death re-frame. The clamp is `sb`-truncated, so values above a byte wrap rather than saturate - which is the behaviour to port, not the intent. |
| What does the dome's contest restore (`FUN_801D0ED8`) do? | resolved (one-shot per contest, and it leaves the accessories alone) | `disassembly` | The caller `FUN_801CEA6C` tests `_DAT_8007BAC0` at `0x801CEB58` and jumps past the `jal 0x801D0ED8` at `0x801CEBF0` when it is already non-zero, so a contest refills once when it opens and a leg boundary never does. It refills party slot 0's HP / MP / SP to their maxima (`+0x104` / `+0x108` / `+0x10C`), and its stripped arm zeroes only `+0x196` / `+0x197` / `+0x198` / `+0x19A` - the Seru-lock byte `+0x199` and the accessories survive. Detail in [`functions/minigames-debug.md`](../functions/minigames-debug.md#801d0ed8). |
| Do the dome's hub screens run on frame counts? | resolved (no - they are fade / hold / fade envelopes) | `disassembly` | `DAT_801D1A80` and its siblings are `0..0x80` **brightness** levels, not durations, and the two holds end on `pad & 0xF4` rather than on a timer. A frame-count port ends the screens at the wrong instants and cannot be ended early by a button. See [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md#the-hub-screens-are-envelopes-not-frame-counts). |
| Which dome chip seats does the AP-cost re-centring apply to? | resolved (both `x` seats, `+0x2` and `+0xA`) | `disassembly` | The term is `(cost - 30) * K[slot] / 2` with `K = DAT_8007B650 = [2, 1, 1, 0]` and truncate-toward-zero halving, applied to each of the record's two seat-x halfwords; anchors are `176 / 216 / 216 / 256` from `0x80076BBC`. `K` makes the arm chip grow away from the D-pad glyph between the pair. |

### The battle hit tint - what a landing hit writes and how it reaches the pixel

*Status:* resolved. Evidence: `capture` (the triple, its ease rate and the pixel law read off two
consecutive save states; every write and gate also in disassembly).

A landing hit stamps a three-word tint triple on the struck actor, selected by the acting record's
`+0x7A` (party melee / arts) or the move-power `+0x0A` (monster specials). The presentation SM eases
it back, and the draw stages the words as GTE far colour + `IR0`, so the texel is modulated, not
replaced.

- **Stamp.** `FUN_801EC3E4` `0x801EE3D4..0x801EE43C`: `+0x04 = 0x801F53D4[sel - 1]`, `+0x21F = sel`,
  `+0x0C = 0x1000`, with `sel = record[+0x7A]`, a `beq zero` past the arm and `sltiu v0,v0,0x6`
  bounding it. No exit precedes the arm, so every connecting swing (a Stone-absorbed one included)
  reaches it.
- **The bound is a route, not a table guard.** Six archive entries carry `6`, and `6` is the tint-less
  Curse arm (`0x801EE690`, a 1-in-4 `+0x16E |= 0x1000` roll). Pin: `battle_afterimage_gate_real.rs`.
- **Other stampers.** `FUN_801E09F8` `0x801E15AC..0x801E15EC` is the monster-special twin off
  `move_power[+0x0A]`, at each arm's impact phase (`ctx[+0x24E + i] == 3`). The clip-`0x18` arms of
  `FUN_8004CE2C` stamp the same three words (`0x8004D1D4..0x8004D1E4`, `0x8004D28C..0x8004D29C`).
- **Ease.** `FUN_80050120` arm 0 (`0x800501A4..0x80050210`): the word eases to `0x20080200` at
  `1 * dt * 8` lane units per frame, then `+0x0C` drains by `dt << 5`, then `+0x21F` clears. The jump
  table at `0x8001532C` routes states `1` / `3` / `4` / `6..=10` to fixed colours with `+0x0C = 0x1000`,
  `2` to the capture fade, `5` to a hold.
- **Capture.** `battle_gimard_tail_fire_b` -> `_a` hold Vahn at `+0x21F = 1`, `+0x0C = 0x1000` with the
  red lane at `0x35F` then `0x31F` - eight frames of the ease. `battle_melee_hit_spark` (Vahn's
  Somersault landing on Gimard) holds Gimard at the neutral word, `+0x0C = 0`, `+0x21F = 0`: a
  selector-0 record tints nothing.
- **Disc census.** Every player-file basic entry and every Somersault-class art is selector `0`; only
  five Vahn art records (selector `1`) and two Gala records (selector `2`) tint their target.
- **Pixel.** `FUN_8004A908` packs the lanes into the render node's `+0x74` and copies `+0x0C` into
  `+0x78`; `FUN_80048A08` stages them as far colour + `IR0` (`gp[0x9D8]` / `gp[0x9DC]`). The struck
  Vahn reads red `160..248` over green / blue `8..80` across his texture - modulation, not a flat fill.

Port: `engine-vm::battle_formulas::tint_sm_step` (the SM), `World::arm_impact_tint` at the three hit
seats, `MonsterAnimation::impact_class` carrying `+0x7A`; both hosts stage `IR0 = blend / 0x1000`
toward the unpacked lanes (`battle_impact_fx::tint_ir0`).

Owning page: [battle.md](../../subsystems/battle.md#how-the-tint-words-reach-the-pixel).

### Battle end - the results sequencer's timeline, and who strikes the pose

*Status:* resolved. Evidence: `disassembly` + `capture`.

`FUN_8004E568` runs every frame the battle-end signal is `0xFE`, the battle exits on
`ctx[+0x6CE] >= 0x43`, and the **leader** strikes the pose.

- `FUN_80046A20` stops stepping the action SM once `DAT_8007BD71 == 0xFE` (`0x80047040`) and calls
  `FUN_8004E568` every frame (`0x800470D0..0x800470E8`); the exit test is `slti v0,v0,0x43` on
  `ctx[+0x6CE]` (`0x80046DAC`).
- `_DAT_8007BD2C` doubles as the sequencer's phase word (jump table `0x800152FC`): `0 -> 2 -> 4 -> 5`
  through two CD loads for a victory; a wipe's `5` lands on the annihilated arm directly.
- The results frame stages the pose into `+0x1DA` of the seat `ctx[+0x13]` names; no store in the
  battle overlay writes a seat there.
- Timeline on `rim_elm_gimard_victory` (PCSX-Redux poll, N = 1): signal v322, results v402, exit fade
  v657, exit v723.
- The three-member `noa_levelup_banner` state reads `ctx[+0x13] == 0` with seat 0 carrying pose `0x14`
  while Noa levelled.

Port: `world::battle::victory`. Owning page:
[battle.md](../../subsystems/battle.md#battle-end-retails-way---the-results-sequencer).

### What a normal party attack sounds like

*Status:* resolved. Evidence: `disassembly` throughout; the live gate by `capture` (N = 4, probe
`autorun_w4d_melee_grunt_gate.lua`).

An ordinary swing emits **neither** cue. The `XA30` grunt belongs to a strike that commits the
defender's `+0x1F3` reaction; the `0x10C` sting plays only when the effect handle `_DAT_8007BD84` is
non-null.

- **The fork** (`FUN_801EC3E4`, on `_DAT_8007BD84`). Zero takes `FUN_8003D53C(0x1D, chan, dur)` at
  `0x801EEB44` (per-character `(0,0x26)` / `(4,0x2E)` / `(6,0x1A)` off `DAT_8007BD10[seat]`) and the
  re-read at `0x801EEB60` skips the cue. Non-zero branches over the grunt (`0x801EEAC8`) into
  `FUN_8004FE5C(0x10C, seat)` at `0x801EEBE8`, whose voice leg is further gated on
  `FUN_8003DE7C(1) == 0` (`0x8004FE9C`). The grunt's seat is `s6` (`0x801EEA70`), the sting's `s4`.
- **The gate ahead of both.** `0x801EEA88` / `0x801EEAA0` require `s7 != 0` and
  `s7 == actor[s4][+0x1F3]`, `s7` being the staged pose byte committed to `+0x1DA` at `0x801EEC6C`;
  only one of the fourteen definitions reaching the compare loads `+0x1F3`. The `+0x1F3` reaction sits
  behind `sltu s0,s1` at `0x801EC878`, whose threshold is not pinned.
- **Third gate.** `0x801EEAB8 slti v0,v0,0x2` reads `_DAT_8007BC20`, the executable's own `xa_flag`
  debug counter (XA-drive state: `FUN_80016B6C` prints it at `0x80016EB8..0x80016EC0`, `FUN_8004DA00`
  zeroes it on five arms) - not a character level.
- **`_DAT_8007BD84` is an effect-instance handle**, not a mode word, and the same cell the damage
  finisher reads as the enemy-defender halve. An all-forms sweep (the `gp` form `0xA6C(gp)` yields
  zero) over SCUS, all 1233 PROT entries and the overlay images finds exactly three stores:
  `0x8004D658` and `0x80056080`, both `sw zero` (battle start, round reset), and one non-zero at PROT
  0940 file `+0xCA0` = `0x801F7678`, the Cort "Mystic Shield" stager saving the `FUN_80021B04` handle
  it spawned. `FUN_8004CE2C` dereferences it (`0x8004D548`; `+0x56` / `+0x72` written, `+0x10` read)
  and consume-releases it at `0x8004D658` alongside cue `0x10D`. An ordinary swing pages in no capture
  module, so the cell reads null.
- **Banks** (disc demux). `XA27` is an eight-channel stereo sting bank, `XA30` a ten-channel mono
  grunt bank.
- **Live.** All four captured party swings skip the gate: `s7` comes from the `+0x1EF` / `+0x1F0` /
  `+0x1F1` reaction-pose loads, never `+0x1F3`, and neither `FUN_8003D53C` nor `FUN_8004FE5C` is
  called.

Port: `World::fire_melee_impact_cue` + `read_battle_xa_clip_bank`.

### `FUN_8003EAE4` is a seek plus bookkeeping - and the driver is not untraced

*Status:* resolved. Evidence: `disassembly`.

`FUN_8003EAE4` seeks and raises `gp+0x908`; a lone call streams nothing. The CD-callback sequencer it
does **not** arm is `FUN_8003D764`.

- **What it does.** Cancels any in-flight read, positions the drive on the clip file's record
  (`FUN_8005C160(2, 0x801C6ED8 + slot*8, 0x8007BC10)` - the argument is the record pointer, not the
  slot index), issues CD command `0x15` (`li a0,0x15; jal 0x8005C034` at `0x8003EB68`), and stores `1`
  at `gp+0x908` / `gp+0x910` and the slot at `gp+0x890`.
- **The driver.** `FUN_8003D764` (dumped; `functions/script-vms.md`) dispatches solely on the state
  ring `gp+0x928`, whose only writer is `FUN_8003D53C` (`0x8003D6F4`, `0x8003D724`), which also
  registers the callback. `FUN_8003EAE4` never writes `gp+0x928`; it reads it as an entry gate that
  makes the whole routine a no-op while a clip is armed.
- **Its three cells.** Only `gp+0x908` has a reader (a "streamed clip busy" level); `gp+0x910` and
  `gp+0x890` are write-only across `SCUS_942.54` and all 1233 PROT entries.

### Formation species limit - what the battle setup does with 3 distinct monster ids

*Status:* resolved. Evidence: `capture` (write watchpoints + cell readback + heap walk; the rebuild
loop also read in disassembly).

A formation `[a,b,c]` of three distinct species loads as `[c,c,a]` on half of all rolls and verbatim
(three streamed blocks) on the other half.

- **The loop.** `FUN_80055B6C` (`0x80055C80..0x80055D2C`) counts copies of `cells[0]` and holds "the
  other species" in a **single register**, then - behind a 50% coin flip (`FUN_80056798() & 1`) -
  rebuilds the cells as `[other x n, first x m]`. For two distinct species that is an exact multiset
  swap (the species-order variety shuffle); retail authoring never exceeds two.
- **Three species.** The third overwrites the register: the middle id vanishes and the last is
  duplicated.
- **Capture.** `autorun_formation_cell_writers.lua` (write PCs `0x80055D14/18`): installs of
  `[133,151,94]` / `[94,133,151]` / `[151,133,94]` / `[32,34,14]` all read back `[c2,c2,c0]` at battle
  main, seat 1 sharing seat 0's record pointer, in both the map03 and rikuroa contexts.
- **Verbatim side.** All three distinct blocks stream, which turns an over-budget trio into a
  probabilistic battle-load hang (freeze captures: the heap-budget section of
  [`battle.md`](../../subsystems/battle.md)).
- **Randomizer.** The unconditional battle-load safety pass `enforce_species_limits`
  (`crates/patcher/src/encounter.rs`) caps every random formation at 2 distinct species and at the
  disc's authored heap-cost maximum.

### Attack-band damage pacing - the hit-event model

*Status:* resolved. Evidence: `disassembly` (`FUN_801EC3E4` head `0x801EC41C..0x801EC494`, apply arm
`0x801EE984..0x801EEA78`, epilogue `0x801EECDC..0x801EECE8`; `FUN_80047430` call sites `0x800478A0` /
`0x80047BF0` and the bit-1 cut `0x80047900..0x80047948`; `FUN_8004AD80` `0x8004B064`) + `capture`
(two PCSX-Redux hit-event timelines, a plain Somersault and a Tri-Somersault Super).

The anim tick calls the damage kernel every frame a battle clip plays, the clip's `+0x10..+0x13`
beats fire the hits, and live HP moves once per combo.

- **Who calls it.** The strike loop of the action SM (`FUN_801E295C` state `0x1E`) stages one byte
  into `+0x1DA` and sets `+0x1DC` bit 1; it never calls a damage kernel. The kernel runs from the anim
  tick with the cursor frame in `a2`.
- **Head guard (is this frame a hit).** `ctx[7] != 0x5A`; the committed entry's byte 0 in
  `0x0C..=0x1F`; the per-clip hit index `+0x1F4 < 4`; `entry[+0x10 + idx] != 0`;
  `frame + 1 >= entry[+0x10 + idx]`.
- **An admitted hit** resolves with `entry[idx]` as its power byte, adds the damage to the target's
  combo word `+0x0` and its HP-bar word `+0x10` (`0x801EDB40` / `0x801EDB58`) and bumps `+0x1F4`; every
  commit zeroes the index.
- **The HP write (`s2 = 0` mode).** The hit that lands after the strike loop has parked the cursor at
  `0xFF` (`ctx[+0x15]`, `0x801EE9A4`) and is its clip's last listed beat
  (`entry[+0x11 + idx] == 0 || idx == 3`) subtracts the whole accumulator from `+0x14C` and zeroes it
  (`0x801EEA10..0x801EEA74`).
- **The other two `s2` arms** (`0x801EDEE4..0x801EE130`, monster-attacker copy
  `0x801EE790..0x801EE980`). `s2 != 0`: a look-ahead over every remaining hit of the action that finds
  none able to connect with the target's `+0x1E` size class applies the total at once. `s2 = 0xFF`: the
  War God Icon's *Attack x2* (ability bit `0x0D`) with `ctx[+0x16] < 2` withholds it so the pair lands
  as one.
- **Bit-1 cut.** The tick commits the next staged byte once `entry[+0x10] + 2 < frame` with
  `entry[+0x76] == 0`, which chains one swing into the next mid-clip; art records carry `+0x76 = 1`
  and play to their natural end.
- **Loop window.** The loop-window arm re-zeroes `+0x1F4` on every rewind for a party slot on `0x11`
  with a latched id `>= 0x2B` (`0x80047840..0x80047878`), so a windowed Hyper / Super clip re-fires its
  hits per cycle.

Captures (N = 2, `scripts/pcsx-redux/autorun_hit_event_timeline.lua` over
`party_basic_attack_vs_gobu_gobu` and `battle_vahn_tri_somersault_super`):

- **Plain three-arrow Somersault.** Swing `0F` (`p0 = 0x18, e0 = 7`, lock `0`) commits and hits at
  frame 6; swing `0E` (`p0 = 0x13`) commits 8 vsyncs later at frame 10 (the bit-1 cut) and hits at
  frame 6; the `0x19` starter (slot `0x10`, `e0 = 0`) plays with no events; `0x27` installs at slot
  `0x11` (`e0 = 5`, lock `1`) and hits at frame 4 while the SM sits in `0x20`, and the target's HP
  falls 76 -> 23 in that one frame.
- **Tri-Somersault Super.** Seven hits across `0F`, the two-hit `0x1F` (`p = [0x17, 0x17]`,
  `e = [8, 19]`), `0E` (`p0 = 0x1D`), the `0x1A` SpecialStarter (slot `0x11`, loop window
  `[13, 14] x 5`, solo byte `1`) and three `0x2B` clips; HP lands 3542 -> 2318 on the last one.
- **Probe caveat.** `*(actor + 0x22C)` alternates between two draw nodes on consecutive vsyncs, so a
  per-vsync cursor sample can read the node the tick did not advance (three frames behind); the
  in-sync stream satisfies the head guard on every hit.
- **Port timeline** (`legaia-engine play-window --battle 4`, `RUST_LOG=legaia_engine_core=debug`,
  N = 2 runs) reads the same shape. A two-swing Auto queue commits `0C`, hits at frame 6 of its
  `e0 = 7` entry and accumulates, commits `0D` at the boundary and lands the total on its hit. A typed
  `↑↓↑` becomes `0F 0E 1A 27`: both swings hit at their beats, the starter parks `[13, 14] x 5`
  with no hit, and the Somersault entry (`p0 = 0x18, e0 = 5`, lock `1`) hits at frame 4 and lands the
  total once.

Port: `legaia_engine_vm::battle_action::hit_event` (the head), `World::tick_battle_hit_events` /
`land_melee_hit` / `apply_combo_total` (`engine-core`). Owning page:
[battle-action.md](../../subsystems/battle-action.md#a-tactical-art-is-an-ordinary-attack-band-action).

### The battle HUD's per-phase surfaces are the sub-draw script table

*Status:* resolved. Evidence: `disassembly`, corroborated by the handle lists of sixteen states.

Which readouts a battle frame shows (roster card, full-width pill, plaque, AP plate, target plaque,
move name) is not computed: it is a table of per-step element lists.

- The menu SM `FUN_801D0748` runs `FUN_801D388C(step)` on every `ctx[+0x06]` edge (`0x801D0EE4`,
  `0x801D109C`, `0x801D13F0`, `0x801D14C8`, `0x801D1658`, ...).
- `step` indexes `PTR_DAT_801F4D34`: fifty `[count][anim][panel]` + `count` x
  `(placement record, mode)` records in the battle overlay's rodata, walked at
  `0x801D4BA4..0x801D4CBC`. `anim = 1` hard-resets the handle list first, so the pairs are the whole
  screen.
- `FUN_801D8DE8(record, mode)` takes the record straight into `0x80076C10 + id * 0x18`. Mode bit 0
  picks the spawn seat (`+0x02/+0x04` or `+0x0A/+0x0C`) and the glide runs to the other; bit 1
  suppresses the glide (`0x801D92E0..0x801D93DC`).
- The action SM's openers are the `0x0C` seed (`0x801E2F24`: bar for a party target byte `+0x1DD`,
  panels for `8`) and the Item / Spirit pre-arm `0x3C` (`0x801E3DA0`); every close is in the `0x51`
  band (`0x801E6170..0x801E6364`).
- The item window's `0x64` "target strip" is record 7 again (step `0x12`: `07/0`, `34/1`), and its
  packet-pinned pens are the ring bar's seat for seat - the port draws the bar there and the item
  window keeps only its breadcrumbs.
- Each `mednafen` state's `ctx[+0x1074]` walk agrees element for element; the disc-gated
  `crates/engine-core/tests/battle_hud_subdraw_disc.rs` holds the port's constants to the table's
  bytes.

The resulting rule (card at the round prompt and while a window is browsed, pill in the ring and the
target steps, nothing during a party attack on a monster, the target's pill during a monster cast) is
on the owning page:
[`battle.md`](../../subsystems/battle.md#the-per-phase-rule---what-the-sub-draw-script-builds).

### The ring's element chip reads `-` or the Ra-Seru, and the gate is the equipment byte

*Status:* resolved. Evidence: `disassembly`, corroborated across twenty-nine states.

- **The chip.** `FUN_801D8DE8`'s case for record 10 (`0x801D8EC8..0x801D8F2C`) writes the chip's
  string pointer as `0x801F4B9E + char_id * 10` (the run `Meta` / `Terra` / `Ozma` at indices `1..=3`,
  a lone `-` at index 4) when `ctx[+0x25F + member]` is set, and the `-` entry when it is clear.
- **The gate's one writer** is the party battle-actor init `FUN_80053CB8` (`0x800541D0..0x80054270`):
  `lbu v0,0x761(v0)` off the `0x80084140` display alias, i.e. the live character record's `+0x199`,
  the Ra-Seru slot of the eight equipment bytes at `+0x196` - or `+0x760` (`+0x198`) on the arm
  character id `2` (Noa) takes.
- **States.** In every catalogued state the three gate bytes equal `(+0x199 != 0)` for the seated
  members: `0` in the sparring fight, `1` for Vahn from the Meta capture on, `0` for Noa beside Terra,
  all `1` at the level-99 Rage state.

Port: `engine-core::battle_hud::battle_magic_chip`.

### The combo counter's glide is placement record 80's

*Status:* resolved. Evidence: `capture` (the live glide record); the stepper by `disassembly`.

- The `HIT` / `TOTAL` / `DAMAGE` cluster hangs off screen-element record 80: seat A `(328, 170)`,
  seat B `(168, 170)`, opened at mode 0 and closed in the `0x51` band (`FUN_801D8DE8(0x50, 1)` at
  `0x801E6360`, gated on the damage finisher's `_DAT_8007BD14`).
- `battle_melee_hit_spark` carries the glide record live at `ctx[+0x11B4 + slot * 0xC]`: total `0x10`,
  elapsed `0x0C`, start `(328, 168)`, target `(168, 168)`, handle at `x = 208` - the `+40` every packet
  of the cluster shows against the settled seats of `player_steal_skeleton_banner`.
- `FUN_801D9BBC` steps it linearly (`start + (target - start) * elapsed / total`, snap on arrival), so
  the cluster slides 160 px in over sixteen frames.

Port: `engine-vm::battle_value_readout::combo_slide`.

### The chrome `kind` byte is an index into the widget-class table

*Status:* resolved. Evidence: `disassembly` + `capture`.

A [screen-element](../memory-map.md#0x80076c10---one-table-three-names) record's `+0x0E` byte is a
**record index into the widget-class table** at `0x800732A4`; the record is the sprite the element is
framed in.

- **Dispatcher.** `FUN_8002C69C` (`ghidra/scripts/funcs/8002c69c.txt`, the `POLY_FT4` / `SPRT`
  emitter) reads the kind out of `gp+0x14C`, multiplies by `0x0C` (`sll`/`addu`/`sll` at
  `0x8002C7A0..0x8002C7AC`) and adds `0x800732A4`.
- **Exact join.** Every kind byte, high and low, on all 103 initialised placement records names a real
  widget record.
- **It is a pair.** `+0x0E` and `+0x0F` are both indices; they are equal on all but five records,
  where the pair reads `(gold, blue)`.
- **Chain field.** Widget `+0x02` is a **signed** hop (`lb v1, 0x2(s7)` at `0x8002FF00`, added into
  the index and re-entered at `0x8002C780` unless zero), which is why one kind draws a whole readout.
  Walking the `0x2B` chain against the bar's own pen `(16, 192)` reproduces `(80, 194)`, `(136, 188)`,
  `(192, 194)`, `(240, 188)` - the four seats the packet walk measures.

| Kind | Widget record | What it is |
|---|---|---|
| `0x01` | `(192, 0)` 16x20, sub-palette 4, class 3 | blue plate body - the command chips |
| `0x02` | `(192, 64)` 16x20, sub-palette 12, class 3 | carved-gold plate body - the name plaque |
| `0x03` / `0x04` / `0x44` | class 0, tile-set 0 | the rectangular gold 9-slice window |
| `0x07` | chain `0x07 → 0x08 → 0x09` | a roster panel: `HP` row, `MP` row, 102x48 plate |
| `0x2B` | chain `0x2B → 0x2C → 0x2D → 0x2E → 0x2F` | the active-actor bar's two label / separator pairs, then its plate |
| `0x33` / `0x34` / `0x35` | the panel chain plus the status marker | the three roster panels, one kind per party slot |

The panel placement records (6, 78, 79) carry kind `0x07`; `0x33` / `0x34` / `0x35` are the sibling
kinds that add the level / status marker, special-cased at the head of `FUN_8002C69C` into
`FUN_8002C2E4(slot)`.

Parser `legaia_asset::ui_widgets`, disc oracle `crates/asset/tests/ui_widgets_real.rs`. Owning page
(layout, classes, chains, palette decode):
[`battle.md`](../../subsystems/battle.md#the-widget-class-table---where-every-chrome-sprite-comes-from).

### The element-badge palette selector

*Status:* resolved. Evidence: `disassembly` + `capture`.

The selector is the badge record's own palette byte.

- Each badge is a widget record (`0x8B..=0x92`, `20 x 12` at a 32-texel pitch from `u = 6` on row
  `v = 192`) whose `+0x03` palette byte is `0x40 + index`.
- Bit 6 of that byte switches both draw routines onto a second CLUT decode,
  `fb_y = 498 + ((b & 0x3F) >> 2)`, `fb_x = 896 + (b & 3) * 16` - a 4-wide by 2-tall block of
  sub-palettes (low two bits pick the column, the next two the row):

```text
badge i -> palette 0x40 + i -> CLUT ( 896 + (i % 4) * 16 , 498 + i / 4 )
```

- That reproduces all four captured pairs from the disc alone: `u = 6` with `(896, 498)`, `38` with
  `(912, 498)`, `166` with `(912, 499)`, `230` with `(944, 499)`.
- The `v = 208` "winged" strip is a **separate** set of eight records (`0x94..=0x9B`), `28 x 12` from
  `u = 2`, on the second CLUT block (`0x48 + index`, rows 500 / 501). Record `0x9B` reads `v = 192`,
  so the eighth wide badge samples the square-framed art on the plain row while its seven siblings
  sample the winged row; the winged eighth badge exists in VRAM and no record selects it.

### The status-element badge sheet (`0x18..=0x20`)

*Status:* resolved. Evidence: `disassembly` + `capture`.

The nine ids `FUN_8002C2E4`'s exclusive ladder emits are widget records `0x18..=0x20`: 48x16 cells in
a two-column block on the resident system-UI sheet (VRAM page `(896, 256)`), each on its own row-511
sub-palette, drawn by `FUN_8002C488` at the caller's seat with no bias. The decoded art is a **word
tag**, not an icon, which confirms the ladder's per-bit assignment independently of the
accessory-guard derivation
([`accessory-passive-table.md`](../../formats/accessory-passive-table.md#status-guard-clear-masks)).

| Sprite | Mask | Cell | Sub-palette | Reads |
|---|---|---|---|---|
| `0x18` | `0x0001` | `(0, 48)` | 9 | `Venom` |
| `0x19` | `0x0002` | `(48, 48)` | 10 | `Toxic` |
| `0x1A` | `0x0004` | `(48, 80)` | 16 | `Stone` |
| `0x1B` | `0x0078` | `(48, 112)` | 14 | `Rot` |
| `0x1C` | `0x0380` | `(0, 96)` | 17 | `Rage` |
| `0x1D` | `0x0400` | `(0, 64)` | 11 | `Numb` |
| `0x1E` | `0x0800` | `(0, 80)` | 15 | `Sleep` |
| `0x1F` | `0x1000` | `(48, 64)` | 13 | `Curse` |
| `0x20` | HP `== 0` | `(48, 96)` | 18 | `Faint` |

- The zero-HP arm reading `Faint` shows the KO test and the bit ladder are one selector over one
  sheet.
- The block's tenth cell (`(0, 112)`) is other art; there is no tenth badge.
- Row 511's sub-palette strip is wider than the sixteen the chrome plates use: these badges alone
  reach index 18, i.e. VRAM x 288.

### Battle-intro tile shatter - the side-face shade page

*Status:* resolved. Evidence: `capture`.

The shade page is a resident field asset, not a transition upload.

- **The page.** The 4bpp page at VRAM `(448, 0)` under the shatter's four semi-transparent side faces
  is the top-left `64 x 64` texel corner of `legaia_asset::field_char_textures` **entry 0** (PROT 0874
  §2), a `256 x 256` 4bpp TIM declared at `(448, 0)`, uploaded at field init and resident for the
  whole field session.
- **CLUT and blend.** `clut 0x7641` = `(16, 473)`, CLUT index 1 of the same entry's 16-CLUT block (a
  `256 x 1` black-to-bright, STP-set ramp on row 473). `tpage 0x0027` carries ABR mode 1, so the side
  faces **add** the ramped texels over their opaque siblings.
- **Capture.** `scripts/pcsx-redux/autorun_tile_shatter_page.lua` walks `karisto_sol_pre_encounter`
  into a random encounter, exec-breaks on the style-2 tick `FUN_801D0D24`, saves states on shatter
  frames 1 / 8 / 24 and logs every `LoadImage` / `MoveImage` rect. The `(448, 0)` rect and the row-473
  CLUT are byte-identical to the pack entry before the encounter, mid-shatter and across two field
  scenes; **no upload touches them in the transition window**.
- **Emitter runtime inputs (same capture).** The per-tile view matrix at scratch `0x1F8003C8` is
  identity rotation with zero translation from the second shatter frame on; frame one still holds the
  field camera's value, so retail's first frame draws no tiles (`_DAT_8007B6CC`, the "not the first
  frame" flag, is that signal). The FT4 near cutoff `0x1F80037E` reads `0x10`; `ZSF4` is `0x400`, so OT
  depth is the plain four-corner SZ average.

Owning page (full spec + engine wiring):
[`cutscene.md`](../../subsystems/cutscene.md#what-style-2s-emitter-builds).

### Battle-intro transition length - `DAT_801D2458`

*Status:* resolved. Evidence: `disassembly`.

132 display frames for every style, 252 for the swirl.

- The intro overlay's init stores `0x84` unconditionally (`addiu v0,zero,0x84` at `0x801CED14`,
  `sw v0,0x2458(v1)` at `0x801CED2C`), two instructions before the `sltiu v0,a0,0x5` style switch on
  `DAT_801D2460`.
- One arm overrides it: jump-table slot `4` (the swirl; table `0x801CE840`, body `0x801CEFEC`)
  re-stores `0xFC` at `0x801CEFF4` / `0x801CEFFC`.
- Read off PROT 0979's instruction words at load base `0x801CE818`; the decompiled `FUN_801ce8cc`
  reaches past that dump's window, so the C alone does not settle it.
- It is the denominator of the whole transition (fade leads, ready bits at `- 0x1E` / `- 6`, shatter
  records held until `delay < elapsed * 0x3C` with `delay = rand() % 5000`): the grid needs ~84 frames
  merely to finish starting, and a shorter window leaves part of it at its seeded pose throughout.

Port: `engine-vm::battle_intro_styles::intro_duration_frames`. Owning page:
[`cutscene.md`](../../subsystems/cutscene.md#how-long-a-transition-runs-dat_801d2458).

### Battle ground grid depth cue - the far colour

*Status:* resolved. Evidence: `capture` (control registers read at the grid draw).

The grid's far colour is the backdrop's staged far colour; there is no separate grid base.

- **The emitter.** `func_0x801d02c0` runs `DPCS` per projected lattice vertex with `IR0 = SZ >> 2`
  loaded bare (`srl` + `mtc2`, so no saturation - past `SZ = 0x4000` the blend extrapolates until the
  DPCS output clamp bounds it) and contains **zero `ctc2`**: the far colour it consumes is whatever
  the control file holds on entry. A save-state read therefore cannot attribute the value to the grid
  pass (snapshots read `(0, 0, 0)` or `(4096, 4096, 4096)` depending on what ran last).
- **The probe.** `scripts/pcsx-redux/autorun_grid_far_colour.lua` sets exec breakpoints on the emitter
  entry and its first `DPCS` site (`0x801d061c`) and dumps control regs 21-23 at the draw.
- **The law.** Every battle hit shows `FC` = the backdrop far-colour staging word at `0x8007BB48`,
  times 16 into the 28.4 control registers.
- **Settled values.** `(0x40, 0x40, 0x40)` on ordinary stages (two town01 battles, stage ids `0x15` /
  `0x0C`) and `(0xFE, 0xFE, 0xFE)` on an overworld battle (stage id `0x55`, on the 13-id
  `DAT_80078C1C` outdoor table at SCUS file `0x6941C`). Both are the neutral base `0x808080` through
  `FUN_80050120`'s two derivation arms: `>> 1` indoor, `(c - 0x010101) * 2` outdoor.
- **Intro ramp.** The battle-intro fade ramps the staged word up from near-black at `+0x020202` per
  frame for ~28 frames before it settles; a Queen Bee sample reading `(6, 6, 6)` is frame 1 of that
  ramp, not a per-stage ambience.
- **Other constants.** `DQA = -64` / `DQB = 320 << 16` at every hit. A field state never enters the
  emitter: the aliased field-overlay code at the same VA idles at the white `(4096)^3` field FC.

Port: `legaia_engine_vm::battle_ground_grid` carries the laws (`grid_ir0`, `grid_far_colour`,
`OutdoorCueTable`, the ramp constants), with the SCUS table pinned by
`crates/engine-vm/tests/battle_grid_cue_scus_real.rs`. `engine-render` stages a per-draw `DrawCue`
(retail sets the DPCS inputs per drawn object) and the play-window grid draws under the `SZ >> 2` ramp
toward the per-stage far colour. The browser play page draws the same grid from the same table:
`play_battle_ground_cue_json` hands the page the engine-resolved far colour and the page renderer
attaches it as a per-draw cue.

### Who calls the battle on-screen test `FUN_8005126C`?

*Status:* resolved - exhaustive negative. Evidence: `disassembly`.

**Nobody** calls it, and the same holds for two of its neighbours; there is no draw-pass consumer of
its verdict.

- **The sweep.** A five-form sweep of `SCUS_942.54`, all statically based overlay images and the raw
  bytes of every extracted `PROT.DAT` entry finds no reference to `0x8005126C`: no literal address
  word (so no dispatch table and no actor template), no `jal`, no `j`, no PC-relative branch, no
  `lui`+`addiu` materialisation
  ([`address-reference-scan.md`](../../tooling/address-reference-scan.md)).
- **Same result** for the passive-name draw `FUN_80035274` and the angle tween `FUN_80050D40`. Each
  follows a clean `jr ra` epilogue whose delay slot closes the previous frame, so they are entry
  points rather than interior labels. Two of the three open frames of their own; `FUN_80050D40` is a
  frameless leaf.
- **`FUN_80025054`** is the same finding one level up: it is a template tick, and the template record
  `0x80070614` that would install it is what nothing materialises. Its table was swept whole (a record
  reached as `base + index` is not a materialisation pair): the head `0x800705FC` is named once, in
  the field overlay, and handed straight to the allocator as one record rather than indexed.
- **Positive controls** the scan reproduces: the template word for `FUN_8004DA00` at `0x800767FC`, the
  21 `jal` sites of the billboard projector `FUN_800195A8`, an intra-function branch target inside
  `FUN_8005126C` itself, and the menu overlay's sub-screen pointer table at `0x801E4F40`.
- **Limits.** An LZS-compressed PROT entry would hide a reference (overlay *code* is stored raw, so
  this does not affect the images callers live in), and an address assembled in more than two
  instructions is not a `lui`+`addiu` pair.
- **The one positive.** `FUN_8004DA00`'s spawner is the battle scene-loader `FUN_800513F0`.

Port: `battle_on_screen` stays inert on purpose. The three rows are unreachable retail code, settled
under the ignore list's `unreferenced` section
([`worklist-classification.md`](../../tooling/worklist-classification.md#the-reachability-claim)).
Write-ups:
[`battle.md` § Unreferenced SCUS entry points](../functions/battle.md#unreferenced-scus-entry-points).
The same sweep over every disclosed-inert port anchor separates rows waiting on wiring from rows
waiting on nothing; the closed list of the latter, SCUS and overlay alike, is on
[`address-reference-scan.md`](../../tooling/address-reference-scan.md#the-retail-unreachable-set).

### Action-SM state `0xFF` treated as battle end by the port

*Status:* resolved. Evidence: `disassembly` (retail half); the port path is live-reachable and fixed.

Retail `0xFF` is the **round boundary**, not the battle's end: its only writer is the non-wipe arm of
the `0x5A` end-of-action gate, and wipes signal through `DAT_8007BD71 = 0xFE` without writing a state
byte. A port that maps `0xFF` to `battle_end(BattleEndCause::MonsterWipe)` grants a spurious victory
(loot and XP) after one round with both sides standing.

- **Why a live battle reaches it.** The counter is the action context's **turn cursor** `ctx[+0x1A]`
  (`BattleActionCtx::turn_cursor`), not a field of the actor record; `Begin` seeds it from the
  formation-advantage byte and never from `ctx.queued_action`. The `0x5A` gate's non-wipe arm bumps it
  and compares against `party_alive + monsters_alive`, so the threshold is reached once each living
  combatant has acted - the ordinary end of a round.
- **Dispatch.** The gate runs whenever a driver leaves the SM parked at `EndOfAction` across a tick
  (after a folded monster spell cast and after a Sleep / Stone skipped turn), so the boundary is
  reached in the first round of a battle that leaves both sides standing.
- **Port.** The state is `ActionState::RoundEnd`: its handler clears every actor's acted counter and
  hands control back through `EndOfAction` (the state the arming driver keys the next turn on). The
  retail `0xFF` body (`ctx[+0x28A]` round bump, `FUN_801F45A4` settle) runs host-side in
  `engine-core`'s live loop. `battle_end(..)` fires only from the paths that raise retail's `0xFE`
  signal: the `0x5A` wipe arms and the escape teardown `0x66`.
- **Regressions.** engine-vm `full_round_with_both_sides_alive_does_not_end_the_battle`; engine-core
  `round_boundary_state_is_not_a_spurious_victory`.

Owning page:
[battle-action.md](../../subsystems/battle-action.md#0xff-is-the-round-boundary-not-the-battles-end).

### Endless camera orbit - the `0x19` attack-approach park

*Status:* resolved; a one-word disc fix ships as `legaia-patcher --approach-softlock-fix`. Evidence:
`capture` (the fingerprinted scenario `battle_gaza2_park_0x19`, caught from ordinary play under the
poll-only hunter `autorun_gaza2_park_hunter.lua`; interpreter replay
`autorun_gaza2_range_wedge.lua`; RAM-table read of the parked save) + `disassembly`
(`overlay_battle_action_801e295c.txt`, `0x801E31F4..0x801E32DC`).

The community-reported "endless camera orbit" (Gaza rematch; JP exhibit too) is the battle-action SM
parked in state `0x19` while the idle camera azimuth sweep (`FUN_801D0748`) keeps orbiting. The orbit
is a symptom.

- **The park.** State `0x14` (attack approach setup), finding the target out of range, looks up the
  walk animation (action tag `0x20`) in the acting monster's table via `FUN_80050E2C`. A table without
  one - bosses generally; Gaza's 12 tags read `[00 01 02 03 04 05 0B 0E 13 0C 23 23]` - takes the
  fallback: stage the tag-`1` "Move" float loop and drop into `0x19`, the range re-poll, **whose arm
  has no movement code and no timeout** (its not-in-range edge only bumps `ctx[+0x6D4]`, read solely
  by the arms-resolver roll).
- **Why it usually works.** The fallback normally still approaches during `0x19` (~19 units/vsync,
  driven by the staged Move clip's playback, not the SM). In the caught parks the drive dies ~12
  vsyncs in (anim pair back to `0/0`), frozen beyond reach.
- **Trigger.** A summon immediately followed by the boss's melee (scenario
  `battle_gaza2_park_0x19_summon_melee`); the stale field is actor `+0x1DC` bit 2
  ([details ↓](#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2)). The disc fix is
  indifferent to it.
- **Stale sentinels.** The `+0x1DD == 8` targets on the idle party actors are stale all-target
  sentinels; the round is stuck on the boss's action alone.
- **The sibling `0x51` HP-settle park** is a fully decoded mechanism that remains
  **injection-only**: a three-capture retail campaign (twelve Lost-Grail revives, no harness HP
  writes) measured out both of its candidate generators
  ([re-do-not-re-walk.md](../re-do-not-re-walk.md#battle--arts--level-up)), and the `0x19` class
  explains the community exhibits without any HP desync. Whether any retail sequence can produce a
  `0x51` park is unproven either way; nothing observed requires it.

Owning page (anatomy, fix, engine-port note):
[battle-action.md](../../subsystems/battle-action.md#the-0x19-attack-approach-park---a-second-distinct-softlock-class).

### The in-fight action framing and the yaw-counter ladder

*Status:* resolved. Evidence: `capture` + `disassembly`.

Of `FUN_801D5854` case 6's two arms, the `0x801D64C4` arm films every ordinary action - party and
monster alike - for the whole of a running fight.

- **The fork byte** `DAT_8007BD71` (`0x801D5CEC..0x801D5CF4`) is the battle-end signal and reads `0xFF`
  until a wipe or an escape. Not a party / monster split:
  [re-do-not-re-walk.md](../re-do-not-re-walk.md#the-case-6-party-arm-is-the-battle-over-framing).
- **The pose.** `pitch 0`, `yaw = (ctx[+0x6DA] − actor[+0x46]) & 0xFFF`,
  `TR = (0, 0x500, ctx[+0x6D0])` (the depth prescaled by `FUN_801D829C`), focus the negated
  `actor[+0x34/+0x38]` pair with the height left at zero; then the `ctx[+0xD]` style tweaks (`1`/`3`
  add a half turn; `2`/`3` set `TR.y = 0x400` and add `0x80` of pitch) and the character-`4` override.
- **Yaw-base seeds** (`ctx[+0x6DA]`, each read off the instruction): `sh zero,0x4(s7)` in the
  round-begin arm (`0x801E2B40`); `li 0x800` in the `0x0C` seed arm (`0x801E2CF8`); `li 0x200` beside
  the `ctx[7] = 0x14` store (`0x801E2F20`); and `FUN_8004E13C`'s `(rand() % 2) << 11 + 0x280`
  (`0x8004E288..0x8004E2B0`) under a three-way gate - argument `2`, `ctx[+0x243] != 2`,
  `ctx[+0x13] < 3` - whose argument is the committed clip's header byte `+0x87` from `FUN_8004AD80`
  (`0x8004BE18..0x8004BE2C`).
- **Per-frame advance.** The prologue adds `max(1, 4 × frame_step / 3)` (`0x801E29E4..0x801E2A24`).
  `battle_melee_hit_spark` reads `ctx[+0x6DA] = 0x298` mid-art: `0x280` plus 24 frames of it.
- **Captures.** Three PCSX-Redux `.sstate` captures parked in `ctx[7] == 0x19` with Gaza (seat 3)
  acting: `TR (0, 1280, 5324)` = `prescale(0xD00)` with `ctx[+0x6D0] = 0xD00` in all three; focus
  `(−433, 0, −291)` / `(785, 0, −39)` / `(0, 0, −1490)` against Gaza's `+0x34/+0x38` of `(433, 291)` /
  `(−785, 39)` / `(0, 1490)`; the `ctx[+0xD] == 2` capture at pitch `0x80` over `TR.y = 0x400`; and in
  each the live yaw eight units behind `(ctx[+0x6DA] − actor[+0x46]) & 0xFFF` (`1657` vs `1665`,
  `1412` vs `1420`, `574` vs `582`) - the per-pass re-arm chasing a counter that moves.

Port: `legaia_engine_vm::battle_cam_script` - `ActionFraming::battle_over` names the fork byte,
`action_framing` drops the focus height, and `BattleCamera::observe_action_state` applies the seed
ladder on the action-state edges. The swing-clip commit is stood in by the edge into `0x1E`, since the
engine's animation player does not expose the clip header byte. Both hosts feed the same
`action_state`.

### The Done band framing and the two orbit writers

*Status:* resolved. Evidence: `capture` + `disassembly`.

After an action resolves, `FUN_801E295C` sits in `0x50` / `0x51` for the `ctx[+0x6D8]` tail (`0x3C`
display frames by default). The camera there is on a **per-action framing chosen by category**, not
the far framing; the Begin/Run prompt orbits at `−2` yaw units per display frame (`−4` per two-frame
camera step).

- **The selection.** Both Done arms read `actor[+0x1DE]` before the framing call
  (`0x801E5E90..0x801E5EF4` in `0x50`, `0x801E5FC0..0x801E6018` in `0x51`), re-armed every pass:

| Condition | Framing |
|---|---|
| category `5` (Run) | none; runs the yaw orbit |
| category `3` (Attack) | `li a1,0x8` |
| a party slot whose target's live HP `+0x14C` is zero | `li a1,0x8` |
| anything else | `li a1,0x6` |

- **The far framing** returns at the end-of-action gate `0x5A`.
- **Done-band capture.** `zora_glare_petrify_post` (mednafen, `ctx[7] == 0x51`, slot 3 acting after a
  spell, `ctx[+0x6D0] = 0xC00`, `ctx[+0x6DA] = 1966`, Zora at `(649, −47)` facing `3297`) reads pitch
  `0`, yaw `2735`, `TR (0, 1275, 4820)`, focus the negated `(624, 0, −40)`: case 6's in-fight pose
  `(0, 0x500, 4915)` on the caster's seat with the tween one step short, and yaw
  `(1966 − 3297) & 0xFFF = 2765` being chased.
- **Between-actions capture.** `evil_medallion_rage_battle` (`ctx[7] == 0x0A`, flow byte `0xFF`) reads
  pitch `32`, `TR (0, 1280, 7920)`, focus at the origin - case 9 over `±825` seats, i.e. the far
  framing.
- **Orbit writers.** `scripts/pcsx-redux/autorun_battle_cam_orbit.lua` on `battle_gaza2_prompt` (240
  vsyncs, `ctx[+6] == 0x1E`, `ctx[7] == 0x00`) with Exec breakpoints on both `sh v0,0x2(a0)` stores:
  the SM's at `0x801E2A6C` never fires; the battle tick's at `0x801D07CC` fires once per tick and the
  yaw drops `2 × DAT_1F800393` each time (`−6` per 3 vsyncs under the interpreter's `step = 3`, i.e.
  `−2` per vsync).

Port: `legaia_engine_vm::battle_cam_script::done_band_phase` over `DoneBandInputs` (category / party
seat / dead target), filled by both hosts; `DONE_STATES` is split from `ACTION_END_STATES`; the orbit
rate is `ORBIT_STEP = 4` per camera step.

### The summon-then-melee park trigger - the stale field is `+0x1DC` bit 2

*Status:* resolved. Evidence: `disassembly` (the driver pair `FUN_80047430` / `FUN_8004AD80` in
`ghidra/scripts/funcs/80047430.txt` / `8004ad80.txt`; the flinch staging at `0x80042124..0x80042170`
in `800402f4.txt`; the SM's `0x14` fallback stores at `0x801E32B0` / `0x801E32D4` in
`overlay_battle_action_801e295c.txt`) + `capture` (causal control / experiment replay on the parked
save `battle_gaza2_park_0x19_summon_melee`, probe
`scripts/pcsx-redux/autorun_gaza2_stale_flag_repro.lua` with write-watchpoints on `+0x1DA` / `+0x1D9`
/ `+0x1DC` logging writer PCs).

The field the summon staging leaves stale is the battle actor's anim event-flag byte `+0x1DC`, bit 2
(mask `0x4`), the **stage-idle-at-clip-end** flag - not a frame cursor or a clip-length latch.

1. The summon's hit stages Gaza's light flinch with `+0x1DC |= 4|1` (exit-to-idle + commit-now,
   `FUN_800402F4`); the flag is normally consumed at the flinch's natural end.
2. The boss's melee follows immediately: state `0x14`'s walk-less fallback stages the Move clip with
   `|= 1` first.
3. The tick's **event-path** commit clears only bits 0-1 (`andi 0xFC`; the natural-end path clears
   0-2, `andi 0xF8`), so the Move clip installs with bit 2 set.
4. At the Move cycle's first natural end (5 frames, rate 2, speed scale 8 → ~12 vsyncs) the tick sees
   bit 2 and stages idle over the queued clip (`sb zero,0x1da` at `0x80047B44`) instead of re-looping:
   pair `0/0`, idle per-tick speed 0, and `0x19` re-polls forever.

- **Control / experiment.** With the flag clear the bounce (state → `0x14`) loops the clip across its
  natural end (pair stays `1/1`) and arrives ~21 vsyncs later; re-arming bit 2 first reproduces the
  kill write from `0x80047B44` at exactly the first natural end, 12 vsyncs after engage, position
  frozen thereafter.
- **Visibility.** The park save reads `+0x1DC == 0` because the killing commit consumed the flag; it
  is only visible in flight.

Owning pages:
[battle-action.md](../../subsystems/battle-action.md#the-stale-field-0x1dc-bit-2-the-exit-to-idle-anim-event-flag)
(mechanism); [monster-animation.md](../../formats/monster-animation.md#playback) (driver + flag byte).

### Super / Miracle Arts trigger chain

*Status:* resolved - matcher, tables, builder chain and runtime effect all pinned. Evidence:
`disassembly` + `capture`.

- **Preseed.** `FUN_801DA34C` copies the saved chain from the char record `+0x76F` / `+0x77F` verbatim
  (`lbu +0x76F → sb +0x1DF`): the char-record chain uses the queue-space encoding directly
  (`0x0C/0x0D/0x0E/0x0F` = L/R/D/U, `0x1A` starter, `0x1B..0x32` art constants).
- **Builder.** `FUN_801EED1C` (battle overlay 0898, ActionSeed state `0x0C`) rewrites arrow runs to
  art constants and applies the Miracle replacement inline, then delegates the Super find →
  tail-replace to `FUN_801EF9E4`. Miracle-before-Super is structural.
- **Super tables.** Keyed on `(actor slot, char index)`: find cells `[len][bytes]` at
  `0x801F6524 + char*65 + row*13`, replace at `0x801F65E8 + char*80 + row*16`, first match wins.
- **Queue.** Exactly 16 bytes, `actor[+0x1DF..+0x1EE]`; `+0x1EF..` is neighbouring data.
- **Verification.** The resident find / replace tables match the modeled
  `crates/art/src/{miracle,super_art}.rs` byte-exact, and all 15 Supers are live-executed: the
  applier-entry injection probe `scripts/pcsx-redux/autorun_super_art_queue_inject.lua` breakpoints
  `FUN_801EF9E4`, writes the target Super's `find` bytes into the queue, retargets the char-index
  register, and reads the tail-replaced queue back at the return site - 15/15 byte-exact, with the two
  hand-driven combos (Noa's Miracle, Vahn's Tri-Somersault) as positive controls. One post-applier
  library state per character is re-checked by `crates/pcsxr/tests/super_art_queue_replace.rs`.

Owning page:
[battle-action.md](../../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4).

### Super Art connectors and physical inputs - the tokenizer keeps the leading arrows

*Status:* resolved. Evidence: `disassembly` (`FUN_801EED1C` `0x801EF2EC..0x801EF858`) + `capture`
(reproduces the in-the-wild Tri-Somersault queue byte-exact) + an independent table (fourteen of
fifteen walkthrough inputs agree).

The `0F` / `0E` "connector" directions in a Super's `find` pattern are the leading arrows of the
arts themselves: the queue builder does not compact a matched arrow run down to `19 <art>`.

- **Normalisation loop.** On a full match it writes the starter over the run's **last** arrow
  (`sb v1,0x1df(v0)` at `0x801EF6F8`, `v1` = `FUN_801EFBFC`'s verdict `+ 0x18`), shifts the tail up
  one slot (`0x801EF708..0x801EF750`) and inserts the constant after it (`0x801EF7A0`). The leading
  arrows stay.
- **Walk order.** The outer walk runs tail-first (`s8` from 15 down, `0x801EF848`) and restarts at
  `s8 + 1` after every match, so runs overlap.
- **Examples.** `↑↓↑` alone tokenizes to `0F 0E 19 27`. Tri-Somersault's `19 27 0F 19 1F 0E 19 27` is
  what the seven-arrow input `↑↓↑↑↑↓↑` tokenizes to (Somersault 0..2, Cyclone 1..4, Somersault 4..6);
  the capture's leading `0F 0E` are Somersault's own first two arrows. Laying the three arts end to
  end tokenizes to four arts and never triggers.
- **Derived inputs.** Every retail Super's pattern derives to a unique shortest input, all 7..=9
  arrows (`legaia_art::tokenize::derive_super_input`, searched over the chain arts' directions). The
  curated walkthrough table agrees on fourteen; the fifteenth (Dragon Fangs, printed as six arrows in
  the walkthrough) drops one and never performs Swan Driver, so the curated entry carries the
  seven-arrow form.
- **Downstream.** `--show-super-arts` draws each Super Art's real arrow string: sixty bytes for all
  fifteen, against the ~330 a concatenation of chain strings costs.

Port: [`legaia_art::tokenize`](../../../crates/art/src/tokenize.rs). Owning page (table + citations):
[art-data.md](../../formats/art-data.md#super-arts).

### The runtime art-record chase - the `+4` and the `- 0x10` grid origin

*Status:* resolved - `record(c) = *(*(DAT_801C9360[char]) + 0x58) + 4 + (c - 0x10) * 0xD0`, with
`+0x10` the name field and `+0x24` the power bytes.

- **The chase** (`0x8004B6FC..0x8004B718`): `lui/addiu` builds `DAT_801C9360`, `lw` the per-character
  block, `lw +0x58` the array pointer, `addiu s0,v0,0x4`.
- **What `s0` is the base of.** `FUN_8004AD80`'s own three uses of `s0` each multiply the action
  constant by `0xD0` with the identical `sll 1 / addu / sll 2 / addu / sll 4` chain and then subtract
  a constant:

| Site | Expression | Resolves to | Consumed as |
|---|---|---|---|
| `0x8004BBE8..0x8004BC10` | `s0 + 0xD0*c - 0xCF0` | `record + 0x10` | the art's display-name pointer (`sw` into `0x8007634C`/`0x80076344`) |
| `0x8004BC60..0x8004BC80` | `s0 + 0xD0*c - 0xCDC` | `record + 0x24` | the per-strike power bytes |
| `0x8004BCA0..0x8004BCC4` | `s0 + 0xD0*c - 0xCF6` | `record + 0x0A` | a byte handed to `FUN_8002B28C` |

- `0xD0 * 0x10 = 0xD00`, so each constant is `0xD00 - field_offset`: the array is indexed from
  constant `0x10` and `s0` is the base of record `0x10` itself.
- The `+0x10` name and `+0x24` power offsets are the same ones `super_art_power` edits off the decoded
  `record0`, so the runtime base and the file-side `art_block_base` are one address in two spaces.
- `--show-super-arts` chases the name through this chain; its planner re-proves it per disc by
  requiring all fifteen Super Art records to carry their own names at `+0x10` before writing anything.

### Character-record HP/MP/AP pair order

*Status:* resolved. Evidence: `disassembly`.

`+0x104` / `+0x108` / `+0x10C` are the effective **maxima**; `+0x106` / `+0x10A` / `+0x10E` are the
**currents**.

- **Clamp triple.** The stat aggregator's closing sequence at `0x80042CE4` is `lhu v1,0x104(s0)` /
  `lhu v0,0x106(s0)` / `sltu` / `sh v1,0x106(s0)`, repeated identically for `0x108`/`0x10a` and
  `0x10c`/`0x10e`: it clamps the *second* halfword of each pair to the first.
- **Cap ladder** (`0x80042C0C..0x80042C50`) is per-field, not a flat 999: `+0x104` → 9999, `+0x108` →
  999, `+0x10C` → 100, `+0x110` → 280, then 999 for five more. A 100-cap on `+0x10C` is the AP
  maximum, which corroborates the pair order.
- **Walk-regen** `FUN_801D0B90` bumps `+0x106`, clamping at `+0x104`.
- **The aggregator** rewrites `+0x104` per frame from base stats plus %-passives; a per-frame
  recompute cannot be current HP.
- **GameShark "Infinite HP"** codes write `+0x106` at every character stride - they pin the current.
- A fresh save cannot distinguish the orders, because `cur == max` at seed.

Port: `legaia_save::HpMpSp` and every consumer carry the `(max, cur)` order; the status AP gauge reads
the AP current at `+0x10E`.

### Effect-VM pass-1 "state token algebra" (`FUN_801E0088`)

*Status:* resolved + ported. Evidence: `capture`.

The "state" bytes are 5.3 fixed-point **wait counters**, not opcodes: two countdown-driven cursor
walks (master spawn cadence over 14-byte pack1 records; child anim / motion over 6-byte pack0 frames).

Port: `Pool::tick_retail` executes the algebra operator-for-operator (pass 2 = `Pool::child_billboards`),
disc-verified over all 33 `efect.dat` scripts. The engine's live path runs it:
`engine-core::World::tick_effects` sweeps `tick_retail` per retail frame and `active_effect_sprites`
maps `child_billboards` one-for-one; dev debug spawns live outside the pool.

Owning page: [effect-vm.md](../../subsystems/effect-vm.md#the-extracted-pass-1-state-algebra).

### Battle face-stamp issuing site

*Status:* resolved. Evidence: `capture`.

The facial-texel overwrite is the per-frame **facial animator `FUN_8004C7B4`**, called from the
render-node update with the clip's frame cursor (Terra skipped).

- Action-entry facial tracks at `+0x8C` (eyes) / `+0x98` (mouth) select frames from static
  per-character SCUS tables, stamped by `MoveImage` every frame.
- Pinned live across a battle entry (`karisto_sol_pre_encounter` + the MoveImage trace probe).
- `FUN_8004CCD4` is **not a stamp**: it is the equipment mesh-variant swap (same caller + guards,
  re-run per ghost by the arts trail renderer), driven by the entry's third track at `+0xA4`; retail
  windows Noa-only.

Owning page: `battle-data-pack.md` § Facial animation tracks + § Equipment-variant track.

### Spine flag `0x482` (Drake mist-wall) writer

*Status:* resolved - writer-less. Evidence: `capture`.

**No writer ever fires**: the `map01` P2[34..36] C1 spawn-block never latches, and wall despawn is not
flag-driven.

- Byte write-watch on `0x800857E8` (`autorun_flag_writer_watch.lua`) across the whole post-Zeto beat
  (battle exit, mist-clear FMV, `map01` arrival): the only write to the byte is the SET helper
  re-latching `0x484` (store `FUN_8003CE08+0x28`, `ra 0x801E3598`); `0x482` never flips.
- Every catalogued state through the Karisto era holds it clear (neighbours `0x484..0x487` at `0x0F`),
  and all 37 census sites stay `DESYNCED`.
- Only an engine-side C1-latch-on-fire (pad-walk into a wall) could give the flag a role.

### Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer

*Status:* resolved - script writers exist. Evidence: `capture`.

- Under the retail-frame scene windows the census shows eight **clean** sites: Set / Clear pairs in
  the rikuroa post-Caruban variant MAN (PROT 0157 P2[29]/[30], op `0x56`/`0x66`), rikuroa2 (PROT 0122
  variant), retockin (PROT 0281 P2[7]/[8]) and edretoin (PROT 0800 P2[7]/[8]).
- These are late-game beats, so the vell/vozz `C1=[0x63A, 0x7]` spawn-block passes for the whole first
  visit.
- Retail states corroborate: `0x63A` reads clear through the Karisto era while its bank byte already
  holds `0x0C` (`0x63C`/`0x63D` set).
- The byte for `0x63A` is `0x80085758 + (0x63A >> 3) = 0x8008581F` (not `0x800858C7`).

### Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer

*Status:* resolved (disc writers + engine port + oracle). Evidence: `capture`.

The story writers are **two** records, not the six an arm count reports.

- **Writers.** rikuroa's post-victory `P2[50]` in the streaming variant MAN (PROT 0157), whose own C1
  gate is `0x142` itself (the self-latching one-shot shape), and dolk2's carrier `P1[0]`, which
  re-asserts it.
- **Not writers.** The other four clean `51 42` / `61 42` arms are **developer flag-menu** rows:
  rikuroa `P1[10..12]` is a nine-flag Set ladder with its mirrored Clear ladder, and dolk2 `P1[1]` /
  dolk `P1[26]` sit in the same shape. An opcode census cannot tell a beat from a menu row; both
  decode clean.
- **Live.** Firehose-caught (`ra 0x801E3598`), and the resident script heap byte-matches the carrier.
  The census must walk the streaming carriers to see it; pins `man_variant_carrier_census_disc.rs`.

Port: the engine sets it organically - rikuroa `P2[50]` executes from its own script bytes on the
Battle-to-Field edge (`organic_beat_records_disc.rs`).

### Drake Castle deep interiors (`jouinc`/`jouind`) depth decode

*Status:* resolved - door-choreography families, not story gates. Evidence: `capture`.

- **`jouinc`**'s 58-record `C1=[0x00F]` P2 family is a **busy-mutex door family**: each record SETs
  `0x00F` first and CLEARs it last, so the C1 gate is a mutual-exclusion lock, and the bodies are
  per-door walk-through choreography.
- **`jouind` `P2[10..13]`**'s `0x4BE..0x4C2` band is **per-visit door / lift state**, cleared by
  `jouina P1[0]` on entry - not a later-chapter revisit gate pair.
- **`jouinb P2[6..8]`** is the interior beat band (`0x44E..0x450` latches plus the jouinb-local
  `0x461` state flag).
- The decode depends on the disassembler's whole-nibble operand widths for `0x4C` nibbles 9/A/C/D/F.

Owning page: [script-vm.md](../../subsystems/script-vm.md) § door-choreography record families.

### cave01 P2[16] spawner - the slot-counted interact chain

*Status:* resolved. Evidence: `capture`.

The ungated `0x15D` setter `P2[16]` is spawned by `P2[12]` once a slot counter reaches 8, and each
creature interaction bumps the counter. PROT cites are the extraction frame (cave01 = PROT extraction
38).

| Record | Role | Bytes / offsets |
|---|---|---|
| `P2[16]` (global `0x1E`) | sets `0x15D`, ungated | `51 5D` at body `+0x22`, MAN `0x3C10` |
| `P2[12]` (global `0x1A`) | bumps slot 0, gates and spawns `P2[16]` | opens `4C CB 00 01 00` (slot 0 += 1); gate `4E 00 50 08 00 06 00` at `+0x15`; `44 1E` at body `+0x1C`, MAN `0x35B9` |
| `P1[3..7]` | the five creature-interact scripts; each spawns `P2[12]` once per interaction | `44 1A` at the first-interact branch tail: `P1[3]` `+0x2CC` = MAN `0x1CE4`, siblings at `0x1FCC` / `0x22B6` / `0x259E` / `0x2888` |
| `P1[2]` | lead-NPC ladder record testing `0x15E`/`0x15D`/…/`0x157`; zeroes slot 0 | `4C CA 00 00 00` at `+0x0C` |

- **The gate** is an op-`0x4E` **sub-5 slot-table compare**: while slot `0x801C6460[0]` < 8 it skips
  forward past the spawn (to the `0x166`→`0x167`→`0x168` progressive counter at `+0x20`); at 8 it
  falls into the `44 1E`.
- **Why the count reaches 8.** The per-NPC talked latches `0x161..0x165` are re-cleared inside the
  interact scripts (`P1[3]` `+0x82..+0x8A`), so interactions repeat.
- Every op-`0x4E` sub-op `0..9` is a compare:
  [the 0x4E details](../re-do-not-re-walk.md#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare).

### NPC dynamic facing - two laws and an execution order

*Status:* resolved. Evidence: `disassembly`.

How a facing changes after spawn (the spawn heading itself:
[below](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis)).

- **Walking snaps.** Every walk kernel - the `0x47` tail in `FUN_8003774C` and the directional /
  wander steps in `FUN_80038158` - quantises the frame's step to the eight-entry compass LUT at
  `0x80073F04` (`entry[i] = i * 0x200`, `0` = -Z) and writes `+0x26` outright. Retail has no walk-turn
  interpolation.
- **Rotate ops ramp.** The four dedicated rotate ops (`0x38` / `0x4C`, `0x04` / `0x0D`) step
  `arc * speed / frames_remaining` off the live heading over a budget the op carries, with an exact
  snap on the terminal frame.
- **Priority is execution order, not a field.** `FUN_8003BC08` runs the dialog SM, then
  `FUN_8003774C`, then `FUN_80038158`, then the anim consumer; an actor running both a scripted leg
  and an ambient stream ends the frame facing wherever the ambient stream put it.
- **Case bodies.** Op `0x38`'s case body is `0x800379FC`; only `0x4C` lives at `0x80037DE0`. The jump
  table at `0x80010EE0` settles all 22 slots.
- **The LUT** is eight entries of `0x200`, not sixteen of `0x100`.
- **`0x4C` sub-modes** `0x85` / `0x8E` / `0x8F` all take one arm; `0x8F` alone forces the direction.
- **`+0x16`** is the terrain-conform angle sampled from the scene grid by `FUN_80019278`, not a
  facing. The yaw is always `+0x26`.
- **Live.** A cold-boot `town01` sample off the static recompilation reads every field actor's
  `+0x26`: all on-field headings are multiples of `0x200` with all eight points present, the only
  exceptions being actors parked on the `(0x7F, 0x7F)` sentinel tile.

Owning pages: [field-locomotion.md](../../subsystems/field-locomotion.md#npc-dynamic-facing) +
[motion-vm.md](../../subsystems/motion-vm.md#how-an-actors-facing-changes).

### 0x4C 0x51 byte +3 reconcile - facing wins; no motion-bytecode synthesis

*Status:* resolved. Evidence: `disassembly`.

- **`4C 51` case-1** (dispatcher `overlay_0897_801de840.txt`, case 5 sub 1) consumes byte `+3`
  **only** as `[bit7 -> actor render flag 0x1000000 (special model) | low nibble -> +0x26 =
  heading LUT 0x80073F04[b & 0xF]]`. The op carries **no speed operand**: byte `+4` is the
  move-anim id written to `+0x5C` (consumed by the anim-stream stepper `FUN_800204F8`);
  non-player targets also get the `+0x8C/+0x8D` current-tile bookkeeping, and the trailing
  `FUN_801D81E0` is an active-list relink (the unlink/relink pair `FUN_800204A4` /
  `FUN_80020454`), not a bytecode builder.
- The `depth & 7` base-step selector belongs to the **walk-kernel op `0x47`'s own third
  operand**: `FUN_8003774C` case `0x47` computes `4 << (b & 7)` (per-frame step
  `0x80 * dt / that`) with the high nibble an approach-mode selector; ops `0x37`/`0x41` encode
  their base step as `(op0 >> 5 & 4) | (op1 >> 6)` of their own two operand bytes
  (`ghidra/scripts/funcs/8003774c.txt`).
- There is **no motion-bytecode synthesis step**: the field-VM yield-class ops
  `0x37`/`0x41`/`0x47` (and `0x38` with a nonzero duration) park the current instruction
  pointer at actor `+0x94`, zero the progress cursor `+0x54` and set actor flag `0x400`
  (dispatcher cases `0x37/0x41`, `0x38`, `0x47`), and `FUN_8003774C` interprets the record
  bytes **in place** - it even resolves the field VM's `0x80` extended-target convention
  (`0xF8` player / `0xFB` world-map entity / placement id vs actor `+0x50`).

Port: `placement_glide_speed` derives the base step from the real `0x37`/`0x41`/`0x47` yield operands
(`placement_yield_step`) and the tail-section-1 wander ops (`placement_wander_step`), with the
facing-nibble reading kept only as a documented last-resort heuristic; `4C 51` byte `+3` sets facing +
the special-model flag only (`placement_initial_facing`). Owning page:
[field-locomotion.md](../../subsystems/field-locomotion.md) § NPC initial facing / § NPC glide speed.

### kor-family op-0x49 flag window [0x138..0x13F] - Uru Mais warp-pad picker

*Status:* resolved. Evidence: `disassembly`.

Each flag is one destination row of the Uru Mais dream-shrine **teleport-pad picker**.

- **Pad records** (kor `P2[17..20]`, kor3 `P2[9..12]`, kor4 `P2[4..7]`; extraction PROT 483/492/501)
  clear the whole window, pre-set **their own row** (kor pads -> `0x138..0x13B`, kor3 ->
  `0x13C`/`0x13D`, kor4 -> `0x13E`/`0x13F`), run the `FUN_801EF014` picker, then dispatch an 8-way
  `0x71` test ladder in which each arm clears `0x612`, fades, stops the BGM and executes a **named
  `0x3F` SceneChange** (kor `P2[17]` body `+0x8D..+0x1C6`):

| rows | destination |
|---|---|
| 0..3 | `KOR` entries `(0x0E,0x35)` / `(0x1E,0x35)` / `(0x2E,0x35)` / `(0x3E,0x35)` |
| 4..5 | `KOR3` entries `(0x70,0x25)` / `(0x0D,0x36)` |
| 6..7 | `KOR4` entries `(0x27,0x27)` / `(0x1E,0x3E)` |

- **Picker behaviour.** State 0 cursors to the pre-set bit (= "you are here") and clears the window;
  confirming a **different** row sets `base + selection`; picking the current row or cancelling sets
  nothing, so the test ladder falls through to the stay-put arm (`+0x1C9`: clear `0x136`/`0x137`, fade
  back, park).
- **Widget semantics** (`ghidra/scripts/funcs/801ef014.txt`): descriptor `+2` `default` = **first
  visible row**, `+3` `rows` = visible row count. The paired descriptors are the full 8-row menu
  (selected by state flag `0x136`) vs the rows-4..7 chambers-only menu (`0x137`; kor3/kor4 carry
  per-pad record pairs, one per variant).
- **`0x137` and the kor pads.** The kor `P2[17..20]` records set `0x136` exclusively and never
  `0x137`; neither flag should be read as reachable from any pad.
- **Menu pixel height `rows * 16` reaches the renderer as a height.** The `sll v0,v0,0x4` at
  `0x801EF160` stores to `0x801F2B98 + 0x196` = record 14, field `+0xE`, of the `0x1C`-stride
  window-descriptor array (stride from `FUN_801E9B3C`'s `sll 3 / subu / sll 2`); the picker's command
  list at `0x801F3304` is `0001 000E`, window id 14; and `FUN_80032434` moves `+0xE` into the live
  window's `+0x10` (`0x80032484` → `0x800325D4`), the field the menu-side creator `FUN_800326AC` fills
  from its descriptor's `h`.
- **Cells.** System-UI plates from the widget table at SCUS `0x800732A4`: record `0x4F` is the "B1F"
  plate, `0x50..0x56` the digits 1..7, `0x57` the "F" suffix, `0x58..0x60` the same set marked in
  yellow. The legend is the Cross / Circle icons (`0x37` / `0x38`) and the field-overlay strings at
  `0x801CF0B8` / `0x801CF0C4` (records checked against a mednafen state's VRAM).

Port: `engine-core::field_submode_flag_window` hosts slot `0x23` on the field path, drawn on both
hosts.

### Encounter MAN sub-section layout

*Status:* resolved

`FUN_8003AEB0` is fully decoded: the MAN header chains **six** sections, and the per-scene encounter
section is read straight from disc bytes.

- Header: `+0x22`, `+0x24`, `+0x26` are signed-16 **record counts** of 3-byte records (assembled from
  `lbu` pairs then `sll 16` / `sra 16` at `0x8003B04C..0x8003B098`); `+0x28` is a **u24**
  (`0x8003B108..0x8003B120`). They are not four signed-16 section offsets.
- Region table (section 3): the count-prefixed array of 18-byte records at `_DAT_801c6ea4 + 0x4`.
  `byte[0]` = kind selector; `bytes[1..4]` = tile-space box `[minX, minZ, maxX, maxZ]` queried by
  `FUN_801dba20(tileX, tileZ)` with `tile = (player_pos - 0x40) >> 7`; `bytes[5..17]` = a per-region
  camera preset (three mode-keyed splits on `byte[5] >> 4` into the camera globals
  `0x8007B607..0x8007B627`, consumed by the camera-param builder `FUN_801dab90`, the arrival handler
  `FUN_801dbec4` and camera-config `FUN_801dbc20`).
- Mask-kind records side-copy `bytes[1..4]` to scratchpad `0x1F8003E8..EB` (mirrors
  `0x801F2778..84`): the [visible tile window](../../formats/encounter.md#the-scratchpad-window-0x1f8003e8eb).
- Port: `legaia_engine_core::encounter_man::scene_encounter_from_man` builds per-scene
  `EncounterTable`s for the standalone towns and the kingdom-bundle scenes (the `count = 6` MAN form
  resolves through `find_bundle`); `legaia_engine_core::field_regions::zone_query` (`FUN_801dba20`,
  with the `FUN_80017fbc` `.MAP` region scan + `FUN_800180ec` attribute refresh) drives
  `World::refresh_field_regions` per tile crossing.
- Outside this thread: the world-overview actor-placement section consumed by `FUN_8003A1E4`
  (world-overview threads).

Owning page: [`encounter.md`](../../formats/encounter.md#man-section-3-the-camera-region-table).

### Seru-magic summon visual (e.g. Tail Fire)

*Status:* resolved - player visual wired; enemy "Fire Tail" characterized

A player Seru-magic summon renders as its **namesake `battle_data` creature** through the ordinary
rigid TRS-keyframe battle draw. The per-spell slot-B overlay is a **spawn stager** of move-VM
scene-graph records, not the creature's draw path; PROT 0900 is a screen-effect + top-view-grid
overlay that does not run during a summon. The enemy "Fire Tail" boss move is a single live move-VM
part-actor.

**Dispatch and stager assignment**

- Battle SM `FUN_801E295C` state `0x29` resolves spell id `0x81..0x8b` via `PTR_801f6734[id-0x81]` +
  `FUN_8003EC70(id-0x79)`; `FUN_8003EC70(param)` loads extraction entry `param + 0x37F`
  ([`prot.md` § In-RAM TOC](../../formats/prot.md#in-ram-toc)), so `0x81..=0x8b` → PROT `903..=913`
  (Gimard `0x81` → 0903, spell `0x83` → 0905). Overlays timeshare the slot-B buffer at link base
  `0x801F69D8` (`*DAT_80010390`).
- Capture: one mid-cast state per spell (the `gimard_summon_*` + `<seru>_summon_mid_cast` scenarios in
  `scripts/scenarios.toml`) holds the loader-B current-id `0x8007BC4C` at exactly `spell_id - 0x79`
  for all eleven ids, `0x81` Gimard → 903 through `0x8B` Nova → 913.
- Stager blocks (`legaia_asset::summon_overlay`): `PLAYER_SUMMON_STAGER_PROT` 0903..=0913;
  `EVOLVED_SUMMON_STAGER_PROT` 0914..=0923 (`0x8c..=0x95`, same `(id - 0x81) + 903` run; 8 of 10 legs
  capture-pinned, `0x90` / `0x91` by arithmetic); `HIGH_SUMMON_STAGER_PROT` 0927..=0934;
  `ENEMY_BOSS_STAGER_PROT` = the six Cort stagers 0938 / 0940 / 0944 / 0961 / 0962 / 0966.
  Ordinary-enemy casts bind the same way: the Delilas brothers → 0958 / 0959 / 0960, Zeto → 0946
  (`enemy_stager_binding`).
- Entry 0907 (Nighto) heads with the ASCII title `Hell's Music` + a normal MIPS prologue. The title is
  the attack's display name (SCUS spell-table string `Hell's Music|Kill or confuse enemy.`;
  `summon.dat` lists it among the attack-name records, parallel to Gimard's `Burning Attack`). Not a
  dance-minigame dual use: the dance overlay (0980) has zero slot-B loader callsites in any form
  (jal / tail-call / pointer-word / lui+addiu); its only loader-reaching call is the SCUS
  `FUN_80025BA0` wrapper (ids 5 / 6 → the 0900 / 0901 move-FX pair) and its music is sequenced BGM
  via the sound streaming loader.

**Stager records**

- A stager spawns each part through the SCUS part-stager `FUN_80021B04` (`a1` = world pos, `a2` =
  record pointer, `a3 = 0x1000`): `actor[+0x48]` = record move-buffer base, `actor[+0x70] = 2` (PC),
  then `jal FUN_80023070` ticks the move VM on `record+4`. High and enemy stagers spawn mostly
  through the pool wrapper `FUN_80050ED4` (→ `FUN_80021B04`, pool `DAT_801C90F0`).
- Record: `[i16 model_sel][u16 reserved][move-VM bytecode @+4]`, variable length, passed by absolute
  pointer (`lui 0x8020 / addiu`). `model_sel` dispatch in `FUN_80021B04`: negative → transform /
  pivot node (dominant; the mesh is bound by the move-VM anim-bank ops); `0x4000` / `0x4001` →
  render-mode node `+0x5A = 3` / `5`; otherwise `DAT_8007C018[model_sel + gp[0x754]]`. The base
  `gp[0x754]` is one per-battle value, `party_count + 2` (see
  [Per-spell magic power](#per-spell-magic-power--multiplier)).
- PROT 0905: 22 `FUN_80021B04` sites → 17 part records, all transform nodes, at file
  `0x180C..0x1E00` (runtime `0x801F81E4..`). The addresses resolve only under link base
  `0x801F69D8`; under `0x801F0000` they fall past the `0x5800` file. Parser
  `legaia_asset::summon_overlay::parse` scans the spawn calls (direct + pool wrapper); disc-gated
  `summon_overlay_real`, `summon_overlay_block`, `enemy_stager_real`; CLI `asset summon-overlay`.
- **Trim before parsing.** A stager's extraction `.BIN` over-reads past the next entry's start LBA;
  the real footprint is `(next_start_lba - start_lba) * 0x800` (`unique_content_len`), a boundary
  the Cort mid-cast saves pin byte-exactly against the slot-B resident image. Trimmed, record first
  words across the whole corpus are only `-1` / small library indices / `0x4000`. Untrimmed reads
  show `0x1000` / `0x8000`-class "sentinels" and extra mesh records that are neighbouring stagers'
  offsets dereferenced in the wrong file window.
- Mesh-bearing records are rare. Gimard's stager (0903) is a pure transform rig (every record `-1`),
  so its draw list is legitimately empty; Nighto (`0x85` → 0907) carries one mesh record.
  `summon_scene_real` drives Gimard for the tick path and Nighto for the draw path.
- `0x4000` render-mode records live in five stagers, all player casts: Palma 0928 (4), Mule 0929,
  Jedo 0931, and the evolved casts 0916 (`0x8e`, 4) and 0921 (`0x93`, 6).
- No catalogued state holds a live `0x4000` node: the Cort enemy states' pooled part-actors all
  carry `-1` records (`+0x56 = 4` / `+0x5A = 2` after move-VM rebinding), and the three Sim-Seru
  player-cast states hold no live stager part at all (zero RAM references to any stager record while
  the stager is byte-resident at slot B). An enemy casting a Sim-Seru creature would seat one
  (`crates/mednafen/tests/summon_render_mode_node.rs`). The node's behaviour is decoded statically
  (next block).

**Part render modes (`FUN_80021DF4`)**

- The per-part tick `FUN_80021DF4` dispatches `+0x5A` into six modes: `2` / `6` parameter / colour
  tween; `3` (the `0x4000` node) moving particle (`FUN_80019D50`); `4` VRAM-blit beam (`LoadImage` /
  `MoveImage` / `StoreImage` = `0x8005842C` / `0x80058490` / `0x800583C8`); `5` (the `0x4001` node)
  a 3D positional **sound** emitter (range / volume + SE trigger, not a visual node); `7` matrix
  transform + billboard; otherwise transform pivot. Reference:
  [`move-vm.md` § Part render-tail](../../subsystems/move-vm.md#part-render-tail-the-0x5a-render-modes-fun_80021df4).
- Port: the classification `engine-core::summon::RenderMode` (`from_model_sel`,
  `// PORT: FUN_80021B04`), consumed by `SummonScene::special_render_nodes` / `part_draws` to split
  the audio-only node off the mesh draw path (tests `render_mode_classifies_only_the_sentinel_nodes`,
  `special_render_nodes_are_split_from_the_mesh_draw_list`); the move-VM call gate is
  `move_vm::actor_tick`. `FUN_80021DF4`'s per-mode GP0 / SPU / VRAM emit paths are documented, not
  ported.
- 239 field-resident prescript render-mode nodes (a resident-overworld mednafen read) are the
  non-summon validation source for the integration.

**PROT 0900 - the slot-B screen-effect + top-view-grid overlay**

- Residency: file `0x0640..0x2660` is byte-resident at `0x801F7018..0x801F9038` in
  `battle_gimard_tail_fire_a` (pin: `0x801F8000` ↔ file `0x1628`); function bodies are
  instruction-identical to the dance / Baka Fighter dumps.
- `FUN_801F811C` is the **screen-mask (iris) widget** handler, not a part transform: its four
  tweened channels are the edges of a screen rect and its four quads are the black border bands
  (GP0 `0x28` flat quads). Mid-tween the latched current values do not move - each frame
  re-interpolates from the fixed start, latching only at `+0x9C == +0x9E`.
- It is kind 1 of a four-kind 2D widget family (scripted sprite `FUN_801F7A9C`, mask `FUN_801F811C`,
  image panel `FUN_801F849C`, letterbox `FUN_801F8A34`) controlled from field-VM sub-ops inside
  `FUN_801DE840`; the ten ending scenes drive it through field-VM op `0x43`. Handler descriptors,
  control APIs, call sites and channel offsets:
  [`move-vm.md` § screen-effect widget family](../../subsystems/move-vm.md#screen-effect-widget-family-prot-0900).
- Port: `engine-core::screen_fx` (mask / sprite / panel / letterbox + the 4-mode `FUN_801DE4C8`
  interpolator; disc-gated `screen_fx_disc`).
- Apparent references to these handlers from stagers 0910..0915 are VA aliasing: in-file
  `FUN_80021B04` part records at coincident addresses under the shared slot-B base.
  `summon::apply_translation_update` keeps the tween shape as an interpreted glide; the faithful port
  of the routine is `screen_fx::MaskWidget`.
- The matrix code in PROT 0900 is the **top-view grid-instance renderer** `FUN_801F7088` plus a
  parallel second-cluster sibling (`RotMatrixX/Y/Z` x6, GTE `MVMVA`). Per grid cell:
  `TR = R_cam * cell_pos + TR_cam` and `R = R_base * Rx(rec+8) * Ry(rec+0xa) * Rz(rec+0xc)`, with
  `R_base` from the camera Euler `_DAT_8007B790/2/4`, per-axis skipped by record flags
  `0x80` / `0x100` / `0x200`; model `DAT_8007C018[rec+0x10 + base@0x8007B6F8]` into cluster-A
  `FUN_80043390`. It is not the summon or move-FX path (0 calls during a player summon, below).

**The player summon is a battle creature**

- PCSX-Redux capture of a player Gimard "Burning Attack" cast (Vahn solo; scenarios
  `gimard_summon_start` / `gimard_summon_visible` / `gimard_burning_attack`), exec-breakpoint counts
  across all three phases: `FUN_801F7088` = 0; move VM `FUN_80023070` = 2-3; part-stager
  `FUN_80021B04` = 1; battle per-actor draw `FUN_80048A08` = 35-64 per frame. Draw path:
  `FUN_80048A08` → per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`,
  each object's Euler composed by `RotMatrixX/Y/Z` (ported in `crates/engine-vm/src/anim_vm.rs`).
  Probes `autorun_summon_rotation.lua` + `autorun_summon_path_reconcile.lua`; RAM dumps under
  `captures/summon_rotation/`.
- Creature identity, from the fingerprint-verified frame-0 RAM of `gimard_summon_visible` (`8aa0...`):
  battle actor table `DAT_801C9370` has slot 0 = Vahn (HP 196) casting `0x81`, slot 3 = a Gobu Gobu
  enemy (HP 76, 13 parts / ~10 actions; actor `0x8008350C`, `+0x5a = 3`, 13-group mesh table at
  `+0x44`, archive `0x800B2694` at `+0x4C`, `+0x88` self-ptr → `+0x8C`, 13x18 = `battle_data` id 4
  action 0), and a distinct 11-part / 2-action entity whose idle (`0x800BBB20`, 11x40) byte-matches
  `battle_data` id 10 "Gimard" action 0.
- Spell → creature map `0x81..=0x8b` (by name; the `"$2"` / `"$3"` enemy variants excluded): Gimard
  `0x81` → 10, Theeder `0x82` → 25, Vera `0x83` → 28, Gizam `0x84` → 55, Nighto `0x85` → 49, Zenoir
  `0x86` → 64, Viguro `0x87` → 74, Swordie `0x88` → 86, Orb `0x89` → 83, Freed `0x8a` → 92, Nova
  `0x8b` → 95 (`legaia_engine_core::summon::summon_creature_id`, disc-gated
  `summon_creature_map_real`).
- Evolved block `0x8c..=0x95`, pinned by mesh identity (each `summon.dat` group's actor-record TMD
  byte-matches an archive record by longest common prefix, 8-17 KB each, for all of `0x81..=0x95`):
  Gola Gola `0x8c` → 98, Mushura `0x8d` → 101, Aluru `0x8e` → 80, Barra `0x8f` → 141, Kemaro `0x90` →
  144, Spoon `0x91` → 147, Slippery `0x92` → 150, Iota `0x93` → 153, Puera `0x94` → 156, Gilium
  `0x95` → 159. Map `legaia_asset::summon_creatures::SUMMON_CREATURES`, disc-gated
  `summon_creature_tmd_map_real`.
- High block `0x99..=0xA0` (Juggernaut / Palma / Mule / Horn / Jedo / Meta / Terra / Ozma) matches no
  archive record: those summons carry a bespoke mesh in the `summon.dat` group's raw part-pool slot
  (the same oracle asserts the non-match).

**Enemy "Fire Tail"**

- In the two mid-cast frames `battle_gimard_tail_fire_a/_b` (disc + library gated
  `firetail_movefx_liveness`) the slot-B occupant is PROT 0900 itself (loader-B id `5`), not a
  per-spell stager, and its widget family is dormant: an effect-actor-list walk finds zero live
  widgets.
- The live effect is one move-VM part-actor in pool `DAT_801C90F0`, ticked by the SCUS actor tick
  `FUN_80021DF4` (→ `FUN_80023070`; bound at `actor[+0xC]`). Its record (`actor[+0x48]`) lives in
  battle-overlay (0898) resident data at `0x801F5xxx`, below the slot-B base; `model_sel` reads `-1`
  (transform node) / `5` (library mesh).

**Assets and engine**

- The CLUT band is byte-identical across the two animation-distinct frames (motion is geometric, not
  palette cycling). Flame texture = PROT 870 (three 64x256 4bpp TIMs → battle VRAM
  `(320/384/448, 0)`, CLUT rows 474..476); flame mesh = PROT 871 (`etmd.dat`, 30-TMD pack) at
  `DAT_8007C018[26]`.
- Flame-atlas loader `FUN_80020050` (SCUS): uploads PROT entry `0x366` into VRAM twice via
  `FUN_8001fc00` (→ `FUN_8003e8a8`), VRAM region set up by `FUN_80017888` / `FUN_8001e54c` (param
  `0xf000`); gated on `_DAT_8007b868 == 0` (the gate `FUN_801dbe9c` reads); independent of the
  `FUN_800520F0` battle-bundle path (which pulls `0x367..0x36d`).
- Engine assets: PROT 871 → `World::global_tmd_pool[3..=32]`; the flame atlas uploads on battle
  entry and the static flame renders with the row-478 CLUT (`GIMARD_TAIL_FIRE_MODEL_INDEX = 26`).
- Summon render: a cast (`spell_id` in `0x81..=0x8b`) calls `World::request_summon_spawn` from both
  cast paths (`World::fold_battle_event` on `BattleEvent::SpellAnimTrigger`, and
  `cast_spell_on_slots`); the host drains `World::take_pending_summon_spawn` and
  `spawn_summon_creature` seats the creature, drawn through `monster_archive::battle_render_mesh` +
  `MonsterAnimPlayer` + `tmd_to_vram_mesh_posed_rot` (mesh, texture and animation all from PROT 867).
- Battle-actor animation pipeline: `legaia_asset::monster_archive::idle_animation` (action 0, the
  `+0x8c` 9-byte TRS stream) → `legaia_engine_core::battle_anim::MonsterAnimPlayer` (8.8 fixed-point
  loop cursor producing a `legaia_anm::PoseFrame`) → `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`
  (`R*v + T`, `Rz*Ry*Rx`). `enter_battle_render` attaches the clip per actor and
  `World::tick_battle_animations` advances it; disc-gated `battle_anim_real` (monster 1 = 28 frames x
  15 parts).
- Stager scene-graph: `engine_core::summon::SummonScene` seeds one move-VM `ActorState` per parsed
  part (PC = 2 → `record+4`) and ticks it through the move VM (`World::spawn_summon` / `tick_summon`
  / `active_summon_part_draws`; `summon::summon_stager_prot_entry` maps id → entry; `play-window` `G`
  debug-spawns the Gimard stager). Every Gimard part runs without an unimplemented opcode (disc-gated
  `summon_scene_real`). It is the stager-record driver and the model for move-VM part effects, not
  the creature draw.

### The party cast trigger is a params stager

*Status:* resolved

`FUN_801DBF9C(party, spell_id)` runs at the end of state `0x29`'s wait for a party caster and only
writes the caster's action-parameter stream; no store touches HP, MP or a target. The outcome is the
streamed module's (`overlay_battle_action_801dbf9c.txt`).

- Two arms on `sltiu v0,a1,0x25`. At or above `0x25`: `actor[+0x1E0] = 9`, `+0x1E1 = 0x12`,
  `+0x1E2 = 0xFF`, return (`0x801DC064..0x801DC09C`). Below: index `0x801F4E64 + id - 1` selects an
  8-byte anim-pair list at `0x801F4EDC`, copied into `+0x1E0..` up to the `0xFF` terminator.
- Every player Seru id is `>= 0x25`, so every player cast - healing included - takes the summon arm.
  State `0x29` reads the sub-route back (`lbu v1,0x1e0(s3); li v0,0x9; bne` at `0x801E45EC`).
- The staged `0x12` is a cast-effect id, not a clip: the argument the summon band hands
  `FUN_801DC0A0` each frame while the caster's `+0x1D9` stays `9` (capture `gimard_summon_start`).
- Port: `BattleHostImpl::spell_anim_trigger` (engine-core, `// PORT: FUN_801DBF9C`) arms the stager
  on the summon arm; the `< 0x25` pair lists are read off the disc into
  `BattleState::spell_anim_pairs`.

### Player-summon presentation

*Status:* resolved

Read off the capture corpus and the summon band's disassembly:

- **No spell-name label for a party caster.** State `0x28`'s label block is skipped for an acting id
  `< 3` (`sltiu v0,v0,0x3; bne v0,zero,0x801e4460` at `0x801E43D8`); the mednafen `*_summon_mid_cast`
  display crops show the acting-actor plaque, the caster close-up or the flash's white and the
  additive burst, and no label. The party readout follows the hide: absent in every `0x33` / `0x34`
  crop and in `gola_gola` (`0x35`, all seats hidden), back under the caster in `vera` (`0x35`,
  stager phase 3, the caster's `+0x21C` cleared while the other seats stay `0xFF`).
- **The hide.** `0x34` zeroes the prim word and sets `+0x21C = 0xFF` on every party seat and every
  living monster (`0x801E4B30..0x801E4B6C`); the PCSX `gimard_summon_visible` / `_burning_attack`
  states read exactly that on slots 0..3 while the creature at slot 7 stays drawn; `0x36` restores.
- **The creature seat.** Slot 7 at `x=185, z=-2272` (caster `82, -542`) with the caster's facing on
  the idle clip at stager phase 6, then `z=-1606` on clip `1` at phase 11: it walks in from behind
  the party toward the target.
- **The two flashes.** Flash-in at `0x33` (additive, delay `0x14`, ramp `0x14` black → white, hold
  `-1`, id `1`) plus cue `0x63`; flash-out at `0x34` (additive, ramp `0x78` white → black, hold `1`).
  The `visible` state carries the flash-out actor finished (`+0x10 & 8`, hold counted below 0).
- **The duck.** `_DAT_8007B910` reads `161` against level `215` in both mid-cast states - the
  `75/100` floor of `0x35`, exactly.

Port: the summon band (`engine-vm::battle_action::summon`), the stager and the hide / fade seams
(`engine-core::world::battle::cast_band`), both hosts' draw gates and `fade_prim` composition.

### The fade actor's hold word

*Status:* resolved

A hold of `-1` means **hold the landed colour until the actor is killed** (`FUN_80020C14`,
`80020c14.txt`).

- After the duration goes negative the tick sets `actor[+0x62] |= 0x100`, then
  `lh v0,0x1e(a1); bltz v0,0x80020cd4`: a negative hold skips the countdown and the tick goes on
  ramping and drawing.
- A non-negative hold counts down and, on expiry, sets `actor[+0x10] |= 8` (finished) and returns
  `-1` (draw nothing).
- Consequences: the escape white-out persists until the battle unloads; the summon flash-in persists
  until `0x34` kills it.
- Port: `engine-core::fade::FadeState::step`.

### `summon.dat` / `readef.DAT` side-band streaming

*Status:* resolved (entries + format)

The two `0x10800`-slot battle streaming files are extraction PROT 893 (`summon.dat`) and 894
(`readef.DAT`), both decoded. Parser `legaia_asset::summon_readef`, disc-gated `summon_readef_real`.

- **Entries.** `FUN_800558FC` ignores its path string in retail (`_DAT_8007B8C2 != 0`, verified live)
  and consumes its 4th argument as a raw-TOC index: `summon.dat` = `0x37F`, `readef.DAT` = `0x380`
  (raw - 2 = extraction, the same offset as the overlay loaders' `param + 0x381`). The footprints
  divide into exactly 103 / 78 slots of `0x10800`. In `battle_gimard_tail_fire_a` the stream buffer
  at `*0x8007BD74` equals entry 894 slot 1, and slot 0's CLUT row / texture page match VRAM
  `(0,488)` / `(512,0)` byte-for-byte.
- **Slot selection.** `FUN_801E295C` case `0x32` maps action id → base slot: `3*(id-1)` for
  `id < 0x9A`, else `4*id + 0x63`; bit 7 selects the file. The applier `FUN_801F12D0` streams slots
  `base..base+3` (readef groups stop after `base+1` unless `base == 0x36`) and uploads CLUT rows +
  texture pages; `FUN_801F19EC` installs the final slot as the summon creature (via `FUN_80055468`).
  Summon group 0 (spell `0x81`) carries the "Burning Attack" record.
- **Per-turn seed.** `FUN_801DABA4` seeds the group base per turn (party `3*(char-1)`; enemy
  `3 * monster_record[+0x1C]`); the battle-end arms request `3*char+2` directly.
- **Aux slots.** The eight aux slots of readef groups 0..3 (slots `3c+1` / `3c+2`, c = Vahn / Noa /
  Gala / Terra) are the player art-animation `"ME"` stream archives, consumed by `FUN_8002B28C` out
  of the `*0x8007BD74` buffer (parser `legaia_asset::me_archive`); the main-vs-base pick is per battle
  phase (turn staging vs battle-end win-pose staging). Loader `FUN_801F17F8` (raw TOC `0x380`, slot
  `* 0x10800`) has a single staging `jal` at `0x801F17A0`; the cast band originates no request of its
  own. Higher groups' aux slots are textures, the four ME archives, or a never-staged actor record
  (the aux-slot row in this area's table). The ME read is gated to party seats `0..2`, and a sweep of
  `DAT_8007BD10` over the state corpus finds char 4 at seat 1 in one battle, so group 3's archives
  are reachable.
- **Unenumerated:** the full `actor+0x1DF` id ↔ enemy-special mapping (the `map[actor+0x1df]`
  128-byte band). The Tail Fire capture is consistent with action id 1 → readef group 0.
- **CDNAME number space.** `#define` numbers are raw-TOC indices, a uniform -2 to extraction: every
  byte-pinned loader constant for a dev-named file equals the same-named define (`summon.dat` /
  `readef.DAT` `0x37F` / `0x380` = `bat_back_dat 895/896`), and semantic scoring over decidable
  blocks is 217/225 at -2 vs 209/225 at 0 (`scripts/asset-investigation/cdname_shift_analysis.py`).
  `legaia_prot::cdname::block_for_extraction_index` gives the retail-space name; the identities and
  exceptions are tabulated on the owning page.

Owning pages: [`summon-readef.md`](../../formats/summon-readef.md),
[`battle-data-pack.md` § "ME" stream archives](../../formats/battle-data-pack.md#me-stream-archives-readefdat),
[`cdname.md` § numbering space](../../formats/cdname.md#numbering-space).

### Monster steal item (Evil God Icon)

*Status:* resolved - static SCUS table `DAT_80077828`

What the player steals with the Evil God Icon equipped comes from a static `SCUS_942.54` table, not
from the PROT 867 monster record.

- Table `DAT_80077828` (file offset `0x68028`), indexed by 1-based monster id: entry at
  `DAT_80077828 + id*2`, a 2-byte `[steal_chance_pct, steal_item_id]` pair. **Chance first, item
  second** - the reverse of the monster record's `[item, chance]` drop fields.
- Capture: a live player-steal RAM read of Skeleton (id 13) gives `1e 8a` = 30% Incense, matching
  the on-screen banner; the table is byte-exact against the complete published steal table (item
  and chance) across every resolvable monster id.
- Not in the record, disc-measured: for the 185 monster ids both populated in PROT 867 and stealable
  in the SCUS table, no offset carries the pair in either field order - not in the 13,030,964 bytes
  of LZS-decoded monster blocks (every offset, full block length), nor in the 15,155,200 raw bytes
  of the `0x14000` slots. Best agreement in any layer is 2/185 for `[chance,item]` and 2/185 for
  `[item,chance]`.
- Single-byte offset `0x48` scores 31/185, but `0x48` is `drop_item`, and steal and drop draw from
  the same 39-item consumable pool; none of the 31 also agree on chance at `0x49`. The best non-drop
  offset is 7/185.
- Monster ids `187..190` are stealable in the SCUS table but have no archive slot (PROT 867 is 194
  slots of `0x14000`, 186 populated).
- Parser `legaia_asset::steal_table`; randomizer `legaia_patcher::steal`. `enemies.toml` `steal` is
  ground-truth labelling; the SCUS table is authoritative.

Owning page: [`steal-table.md`](../../formats/steal-table.md).

### Per-spell magic power / multiplier

*Status:* resolved - mechanism decoded, kernels ported and wired

A summon's "power" is **caster / summon battle-state-derived**, not a static per-spell scalar (SCUS
spell-table `+5..+8` are zero). Damage runs the three-stage chain `FUN_801dd0ac` roll →
`FUN_801dd864` scale → `FUN_801ddb30` finish; the per-move power table at `0x801F4F5C` belongs to
monster special attacks, not to magic or party arts.

**Where the magnitude is applied**

- Each module's **tick body** applies it, not its stager: the `actor+0x14c` write lives in the
  `ctx+0x279` phase machine the `0x801CF4EC` table arms, a different routine in the same image.
  PROT 0903, 0910 and 0913 are damage modules; per-module classification (PROT 0909 included) is on
  [`cast-module.md`](../../subsystems/cast-module.md).
- Not the 7-entry jump table `FUN_801f2d68` reads (`jr *(0x801F69D8 + state*4)`): it resolves to
  PROT 0900 file offset 0, five staggered entry points into one per-frame routine that lerps move-VM
  anim banks (`FUN_8003ce9c` / `ce64` / `ceb8`) and emits GPU packets into scratchpad `0x1F800314` -
  zero `mult` / `div`, no `actor+0x14c` write, no power read.
- Damage bodies call `FUN_801dd0ac` (`a0` = per-summon move-type const `0x10..0x12`, `a1` = a baked
  `7` on the player half, `a2` = target slot), clamp in one of
  [three shapes](../../subsystems/cast-module.md#the-three-clamp-shapes), accumulate the popup at
  `actor+0x10`, then subtract.
- Two bodies heal, each a closed form over the caster's per-magic **level** byte (the 32-slot search
  matching `actor[+0x1DF]` against the id list at `+0x705`, reading the parallel byte at `+0x729`):
  PROT 0905 (Vera) restores `record[+0x729 + slot] * 0x20 + 0xe0`, clamped to the `+0x14C` / `+0x14E`
  pair, skipped at `HP == 0` or `+0x16E & 4`, pushed to `+0x10` as a negated popup at `0x801F7D0C`;
  PROT 0911 (Orb) restores `(level << 6) + 0x1c0` over the party row. Both unlock a status cleanse
  at level `>= 3`. Neither is `(power_byte << 5) + 0xe0`.

**The roll (`FUN_801dd0ac`, `overlay_battle_action_801dd0ac.txt`)**

- Summon path (`param_2 == 7`):
  `roll = rand % (INT@+0x168 + 1) + HP@+0x14c + DAT_801C9370[ctx+0x13]_INT * 2`; returns
  `roll - defender_mitigation`.
- Non-summon path reads the 26-byte-stride move-power table at `0x801F4F5C` (static battle-overlay
  data, PROT 0898; parser `legaia_asset::move_power`) through the 128-byte id → index map at
  `0x801F4E63` (`param_1 = map[actor[+0x1df]]`).
- The whole 26-byte record is decoded on [`move-power.md`](../../formats/move-power.md): `+0` power,
  `+0x0a` impact-effect selector, `+0x0b` trail texture page, `+0x0d` sound cue, `+0x12` / `+0x16`
  effect-id lists, and `+0x0c` an unused `C` / `E` / `G` designer tag with no runtime reader.
- The move-id space is the spell-table id space, and the map covers 44 ids: idx `0x10..=0x2b` = the
  named monster special attacks (`0x25..=0x74`), idx `0x01..=0x0f` = unnamed internal enemy-attack
  tiers (`0x04..=0x07` / `0x12..=0x1F`). The basic-attack / art bands `0x08..=0x11` and `0x16..=0x18`
  are unmapped (live capture: Vahn's Somersault `0x0F` is unmapped and would roll against zero-power
  record 0). Party arts take damage from the per-strike art-record power byte (`art_strike.rs`);
  `apply_basic_attack` uses the flat `art_strike_damage_default` for a no-art generic hit.

**Scale and finish**

- `FUN_801dd864`: 8x8 element-affinity matrix `0x801F53E8` (PROT 0898 file `0x26BD0`, parser
  `legaia_asset::element_affinity`) + status bits + the summon magic-power tail
  `roll += roll*(power-1)>>3`. Orientation is `matrix[attacker][defender]`; values are a +/-4%
  nudge (diagonal 96, opposite pairs 104, default 100), not a x0 / x2 weakness table.
- Per-character element table `0x801F5480`: Vahn = fire, Noa = wind, Gala = thunder, Terra = wind.
  The enemy element is read record-direct: `lbu ...,0x1d(record)` with `record = 0x801C9348[slot-3]`
  (the per-enemy record-pointer table, not a copied live-actor field), i.e. `MonsterRecord::element`
  (`+0x1D`) - the same record the victory-spoils path reads `+0x44/+0x46/+0x48` from.
- Per-caster summon power-percent table `0x801F5468` (index `(char_id-1)*8 + summon_element`; PROT
  0898 file `0x26C50`, `ElementAffinity::summon_power`): own element 100, opposed 40, Gala dark 60.
- `FUN_801ddb30`: equipment elemental-resistance halving, guard halve, `rand%9+8` no-damage floor,
  summon power-% scale, 9999 cap, spirit gauge, MP drain, per-element stat debuffs.

**Port**

- Kernels in `legaia_engine_vm::battle_formulas`: `summon_attacker_roll` / `summon_defender_roll` /
  `summon_predamage` / the `apply_*` helpers / `heal_summon_amount` / `damage_finish` /
  `spirit_gauge_fill`. The finisher's state-mutating tail (damage-popup accumulator, AI revenge
  table, MP drain, per-element stat-debuff switch) lives in the live battle context.
- Monster special attacks: the table loads onto `World::tables.move_power`
  (`engine-core::move_power::MovePowerCatalog`) and `cast_spell_on_slots` overrides a damaging
  monster cast's magnitude with `arts_physical_predamage_lazy` seeded by the move's `+0` power
  (`World::enemy_move_predamage`: INT from `battle.accuracy`, defense terms from
  `battle.defense_split`). The attacker x2 + defender x1 `rand()` draws are taken up front and the
  bonus pair lazily, only when the bonus arm fires, so the RNG cursor advances by exactly three or
  five draws in `FUN_801dd0ac`'s call order.
- Player summon roll: `World::player_summon_predamage` - summon-body HP / INT from the namesake
  `battle_data` creature record, caster INT from `battle.accuracy`, the caster magic-power byte from
  the character record's spell list (`+0x13D` ids / `+0x161` levels, the `FUN_801dd864` search),
  then the `FUN_801ddb30` finisher with the `0x801F5468` percent. Modelling limits: the slot-7
  actor's HP at roll time is the creature record's spawn HP (a damaged mid-battle summon is not
  modelled), and status / guard default to none.
- Affinity: the monster path scales by `matrix[enemy_element][party_member_element]`
  (`World::enemy_affinity_pct`, applied inside the roll before the conditional bonus-arm threshold,
  as retail orders scale → bonus); the player Seru-magic path scales by
  `matrix[summon-creature element][target element]` (`World::cast_affinity_pct`; attacker element
  from `World::summon_attacker_element`, defender from `World::battle_slot_element`), post-roll with
  the RNG untouched.
- All of these are gated on the tables being installed, so a disc-free battle reproduces the
  no-table baseline bit-identically (magnitude + RNG stream).

**Move-FX spawn records (the `+0x12` / `+0x16` lists)**

- Auxiliary tables: `EffectAuxTables` for the `0x801F6324` prototype-pointer and `0x801F6418` SFX
  tables; `parse_impact_effect_table` for the `+0x0a` selector's `0x801F53D4` entries, which are
  packed `u32` config words, not pointers.
- Each `0x801F6324` entry is an overlay VA to a variable-length move-VM scene-graph record in the
  summon-part format (`+0x00 i16 model_sel`, `+0x02 u16 reserved`, `+0x04` bytecode), packed, not a
  fixed `0x20` stride; spawned by `FUN_80050ed4` → `FUN_80021B04`. List bytes with the high bit
  (`0x80`) route to the 2D `efect.dat` pool instead (`FUN_801dfdf0` → `EffectCatalog`, ported as
  `spawn_by_ui_id`).
- Model-library base `gp[0x754]` (global `0x8007BA6C`): `0` with no battle effect-model library
  resident, else **`party_count + 2`** - `3` for the 1-member training party, `5` for the 3-member
  party (two fixed pool slots + the live party meshes precede the library). One per-battle value
  drives both move-FX and summon-part spawns; `model_sel` is library-relative. Evidence: PCSX-Redux
  exec-bp on `FUN_80021B04` (probe `autorun_summon_model_base`; chain
  `FUN_801e09f8 → FUN_80050ed4 → FUN_80021B04`, `ra = 0x80050F08`, `a3 = 0x1000`, prototype table
  `0x801F6324` + effect-list id `0x22` in registers) plus `0x8007BA6C` against the party count
  `0x80084594` across the mednafen corpus; pinned by `crates/mednafen/tests/summon_model_base.rs`.
- Engine: `World::spawn_move_fx` parses the records (`MoveFx` via
  `MovePowerCatalog::fx_for_move_id`), stages them as a `SummonScene` at the library base (PROT 0871
  registered at a fixed `DAT_8007C018[3..]`) and ticks them through the move VM (`tick_move_fx` /
  `active_move_fx_part_draws`; `play-window` `H` debug-spawn). The part draw transform is the
  engine's anim-bank-derived interpretation; the retail one is the `FUN_80021DF4` render tail.
- Presentation fields: trail texpage (`+0x0b` → `0x7700 + id`) on
  `World::active_move_fx_trail_texpage()`; sound cue (`+0x0d`) via
  `World::take_pending_move_fx_cue()`, resolved by `legaia_engine_audio::classify_cue` → `CueDispatch`
  (the `FUN_8004fcc8` dispatch decode) in `engine-session::battle_fx::spawn_pending_move_fx`, which
  both play hosts enqueue. The voice arm's third argument is a **read span**, not a pitch:
  `FUN_8003D53C` range-checks it against `0x2A31` at `0x8003D5C8` and touches no pitch register.
- SFX bank: the cue's `program` / `tone` (static `DAT_8006F198` table,
  [`sfx-table.md`](../../formats/sfx-table.md)) index the per-scene music VAB the BGM sequencer
  already has open (`FUN_80065034` reads the libsnd current-bank globals; byte-identical to the disc
  `music_01` VAB for that scene) - `SfxBank::play_one_shot(spu, scene_vab)`, no separate bank.
- Afterimage: the streak pass `FUN_801e1ab0` is `legaia_engine_ui::afterimage::build_afterimage_quad`
  (jittered semi-transparent `POLY_FT4`: per-corner `rand` wobble, brightness band, UV / CLUT /
  texpage layout). Its corner projector `FUN_800195a8` (camera-coupled GTE billboard: view-space
  MVMVA centre, +/-half-size corner fan-out, rotation + translation reset, RTPT x3 + RTPS;
  [detail](../functions/renderer.md#800195a8)) is `legaia_engine_ui::billboard::project_billboard`,
  with the `FUN_801e1ab0` call shape (`+0x120` Y push, dynamic half-width `state+0x6c6 - 0x200`,
  half-height `0x100`) as `afterimage::project_streak_corners`. The `RotMatrix*` sin / cos LUT is
  `trunc(4096*sin)` (disc-gated `gte_sin_lut_real`).

Owning pages: [`battle-formulas.md`](../../subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8),
[`move-power.md`](../../formats/move-power.md), [`cast-module.md`](../../subsystems/cast-module.md).

### Stat growth-rate source

*Status:* resolved + validated + wired (core + opt-in jitter)

The per-character stat-grant source is static `SCUS_942.54` tables read by the level-up applier
`FUN_801E9504`.

- Parameter block `DAT_80076918`: per character (stride `0x3C`), 8 contiguous 6-byte sub-records
  `{u16 start, u16 max, u8 jitter, u8 row}`. `start` = base stat (Gala matches the new-game template
  on all 8); `row` selects one of 3 curves at `DAT_800769CC`.
- Per-level gain =
  `max(1, (max-start) * curve[row][level-1] / 0x24C0 + rand() % (2*jitter+1) - jitter)`, then caps.
  `0x24C0` is the curve normalizer: each curve sums to `0x24C0`, so growth accumulates to exactly
  `max-start` by L99.
- Capture: a single-level step (Noa L2 → L3, the `noa_levelup_*` saves) has all 8 deltas within the
  core +/- jitter band. Multi-level corpus observations (`noa/gala_4_level_jump`) are unreliable for
  this check. Not the "Seru struct `+0x74`".
- Parser `legaia_asset::level_up_tables::GrowthTables::{char_params,level_gain_core}` (disc-gated).
- Engine: `StatGain` carries HP / MP + the six battle stats; `LevelUpTracker::with_growth_tables` +
  `BootSession` install per-character curves from the user's SCUS, and `apply_to_record` grows the
  record-side window then mirrors to live (disc-gated boot test pins Noa's L2 → L3 core).
  `LevelUpTracker::with_level_up_jitter(seed)` drives a PSX BIOS-rand LCG (`BiosRand`), one `rand()`
  per stat per level on the unfloored core before the `max(1, ...)` floor; it is off by default so
  determinism oracles stay bit-identical (bit-exactness needs the runtime BIOS-rand seed).
- The slots-1/2 XP-threshold correction is documented on the owning page.

Owning page: [`level-up.md`](../../subsystems/level-up.md#stat-gains).

### Monster stat-record archive source

*Status:* resolved

The monster archive is **PROT entry `0867_battle_data`** (extended footprint; the 15.9 MB archive
lives in the entry's trailing-gap sectors). Retail-semantically it is the `monster_data` block: the
define `monster_data 869` names extraction entry 867 under the raw-TOC -2 correction
([`cdname.md`](../../formats/cdname.md#numbering-space)).

- `FUN_800542C8` streams per-monster `0x14000` LZS slots at `(id-1)*0x14000`, each
  `[u32 dec_size][LZS]`, decoding to a block whose head is the `FUN_80054CB0` stat record.
- Record: name `@0x00`; battle-model TMD offset `@0x04`; HP `@0x0C`; MP `@0x10`; stat u16s
  `@0x0E/0x12/0x14/0x16/0x18/0x1A` (ATK / UDF / LDF / INT / SPD / AGL); rewards (XP / drop) inline at
  `@0x44..0x49`; magic count `@0x4A`; spell-ptr array `@0x4C`.
- Capture: live-battle PCSX-Redux watchpoint (`autorun_monster_record_source.lua`) - relative seek
  `(id-1)*40` sectors + `disc_read` CdlLOC → PROT.DAT `0x38AF000` = entry 867; three records match
  live actor stats byte-for-byte.
- Parser `legaia_asset::monster_archive`; bridge
  `legaia_engine_core::monster_catalog::catalog_from_monster_archive`, wired into
  `enter_field_scene`.

### Monster mesh + texture pool

*Status:* resolved

The monster's 3D battle model is a [Legaia TMD](../../formats/tmd.md) embedded in each PROT 867
archive block at the offset in stat record `+0x04`; its texture / CLUT pool is at record `+0x08`.

- The TMD is installed at battle-actor `+0x230`; the `0x1C`-stride records `FUN_80049858` /
  `FUN_800495C8` walk are its object table. 186 of 194 slots parse cleanly.
- Pool layout (from the battle loader `FUN_80055468`): a `0x1E0`-byte region of fifteen 16-colour
  CLUTs, then a 4bpp page (always 256 rows tall, 128 or 256 texels wide; palette = `cba & 0x3F`).
  Byte-exact against pool sizes.
- The on-disc CBA / TSB are nominal defaults the loader relocates per slot, so the raw pool does not
  appear verbatim in a battle VRAM dump; the loader layout is the ground truth.
- Parser `legaia_asset::monster_archive::{mesh, MonsterMesh::texture}`; CLI `--obj` +
  `--texture-png`; WASM `monster_mesh_*` + `monster_texture_*` accessors drive the enemy-table site
  page's per-row WebGL viewer.

### Terra slot-3 / story-flag overlap

*Status:* resolved

The character-record base is `game+0x3C8` (live RAM `0x80084708`), with the display name at record
offset `+0x2A7`; `0x66F` is the *name* field, not the header size. Terra's record needs no special
case.

- Six in-game RAM captures: mid-game stats at `record+0x104` / `+0x11C` read back the expected
  per-character HP / MP for all four slots.
- The four-slot array runs into the global region: slot 3's tail (record offset `>= +0x2BC` =
  `game+0x12C0`) aliases the story-flag bitmap and inventory. Terra's meaningful fields (name, live
  stats, RecordStats) sit before that boundary. Terra is the New Game template's fourth roster entry
  (HP 400) but never a savable battle-party member, so the aliasing is benign.
- Code: `RETAIL_CHAR_RECORD_HEADER_SIZE = 0x3C8`; `legaia_save::CharacterRecord::name()` /
  `set_name()` at `NAME_OFFSET` (`+0x2A7`); `Party::from_retail_sc_block` reads a populated save's
  stats from the right fields (checked by synthesising an SC block from a live RAM dump and
  comparing the parsed HP).

### Battle party meshes = **assembled from the player battle files** (PROT 1204 = Baka Fighter / default-equipment sibling)

*Status:* resolved (static chain + byte-verified)

A main-game battle renders each party member from a **merged TMD assembled at battle setup** out of
that character's player battle file (`data\battle\PLAYER<n>`, extraction 0863..0866), one section per
equipment slot selected by the equipped item ids (char record `+0x196..+0x19A`). Textures and the
palette come from the same file. PROT 1204 is the Baka Fighter / default-equipment sibling, not the
runtime source, and battle does not reuse the field pack 0874 section 0.

**Assembly chain**

- `FUN_80052770` case 4 (section select: an equipment-id-matched entry or the `id == 0` separator =
  unequipped default) → `FUN_80052FA0` (assembler, blob at `ctx+0x50`) → `FUN_800536BC` x5 (object
  splice; `nobj += section_nobj`, bone-id byte per object, surplus objects tagged = equipment visual
  meshes) → `FUN_80053898` (retag 200 / 201 / 100+, attach bones at `blob+nobj`, sort) →
  `FUN_800513F0` registers `blob+0x18` into `DAT_8007C018[slot]`.
- `nobj` 15 → 17 is the weapon + Ra-Seru sections' extra objects (not `FUN_8001EBEC`, which only
  toggles a pose transform).
- Byte-verified in the full-party Gobu Gobu save: `DAT_8007C018[0] = ctx+0x50+0x18` exactly,
  `nobj = 17`, bone bytes `[0..14,200,201]`, attach `[5,8]`, and all 17 vertex pools found in
  PLAYER1's sections with equipment-selective matches. Only 12 of Vahn's 17 pools match 1204 (the
  default-equipment geometry, byte-shared); the 5 equipped-variant objects (Hunter Clothes body x2,
  Survival Knife piece + extra, the Ra-Seru piece) appear only in the player-file sections.
- Baka Fighter loads PROT 1204 (`overlay_baka_fighter` loads `data\field\other5.lzs` + PROT
  1205 / 1206, debug `"OTHER5 %d %d"`): the same characters with default equipment. Field-pack
  distinctness: `battle_char_pack_real::battle_pack_is_distinct_from_field_pack`.

**Install into `DAT_8007C018[0..=2]`**

- Static SCUS, through the generic registrar `FUN_80026B4C` (store `0x80026BA8`), from two battle
  state handlers reached by indirect dispatch (so a static cross-reference on `0x8007C018` finds no
  writer): `FUN_800513F0` (lead / active actors - `tmd_register(*(actor+0x50)+0x18, 0)` in a
  `while<3` loop over the active-actor table `0x801C9360`, right after the `FUN_80052FA0` palette
  decode) and `FUN_800542C8` (additional members - loop bounded by `*(rec+0x4a)`,
  `tmd_register(*(*rec+4), 0)`).
- The battle loader `FUN_800520F0` registers PROT `0x36a` (`etmd.dat`) into the *effect* window
  `DAT_8007C018[3..]`, not the party slots.
- Capture: write-watchpoint on `DAT_8007C018[0..2]` across the auto-starting Queen Bee field →
  battle transition
  ([`autorun_battle_party_mesh_install.lua`](../../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua)):
  all three installs fire at `game_mode 0x15`, and the pointers byte-match the battle form (Vahn →
  `0x80165F48`). Dumps `funcs/800513f0.txt` / `800542c8.txt`; trace in `funcs/8002541c.txt` /
  `800198e0.txt` / `800520f0.txt`.

**Player-file load**

- `"data\battle\PLAYER1"` is a dev-tree label, not an ISO9660 file: the retail open `FUN_800608f0`
  is a `trap` stub, so `FUN_800558fc` always takes its debug branch → `FUN_8003e8a8(char+0x360)`
  reads `toc[idx+2]` (in-RAM PROT TOC `0x801C70F0`) as a sector offset into PROT.DAT.
- Raw `0x361..0x364` (Vahn / Noa / Gala / Terra) = PROT.DAT `0x36E8000` / `0x3791000` / `0x3828800`
  / `0x3897800` → extraction 0863..0866 (raw index - 2). Extraction 0861 / 0862 are 1-sector stubs
  whose over-read tail begins Vahn's file `0x1000` in, so a match against the "0861 window" is the
  same bytes.

**Textures and relocation**

- Texels upload from the player files' per-section texture pools at the static rect table
  `0x800775B8` (`FUN_80052FA0` → `FUN_80053B9C` LoadImage front-end): >= 99.6% band reproduction
  against clean full-party battles. The 1204 atlases match only 73-98%; the shortfall is the
  equipped-variant texels. See
  [`battle-data-pack.md` § Texture-pool VRAM placement](../../formats/battle-data-pack.md#texture-pool-vram-placement).
- At battle entry every prim's TSB + CBA is rewritten into a packed per-slot runtime band; the CBA
  column is preserved and a character's two disc rows collapse to one runtime row (one 256-colour
  palette per character):

  | Char | Disc pages, rows | Runtime pages, row |
  |---|---|---|
  | Vahn | (640,0) / (704,0), rows 490 / 491 | (512,256) / (576,256), row 481 |
  | Noa | (640,256) / (704,256), rows 492 / 493 | (640,256) / (704,256), row 482 |
  | Gala | (512,0) / (576,0), rows 494 / 495 | (768,256) / (832,256), row 483 |

- Disc sub-CLUT use: Vahn 0,1,4,5 / 0,1,7,8; Noa 0,1,2,5,6,7 / 0,3,4,8; the two auxiliary 1204
  meshes use row 496 page (448,256) and row 497 page (512,256). The disc TSB / CBA are the authoring
  layout Baka Fighter uses directly; walked as-is in a normal battle the mesh renders incoherently.
  Evidence: the runtime TMD dumped from a clean battle save (`flags=1`, absolute pointers; convert
  `p → p - base - 12`) renders the correct characters from the save's VRAM.
- The texpage → CLUT-row table `0x8007BEC0` (32 x u16, written by `FUN_800198E0` as
  `table[texpage & 0x1f] = clut_row`) is the **scene** renderer's; rows 490..497 hold scene
  environment palette shared by a scene's field and battle modes, not party palette.

**Palette**

- A battle-allocated resident block per character, DMA'd to rows 481 / 482 / 483: contiguous at
  `0x800ebee8` / `0x800ec0c8` / `0x800ec2a8` (Vahn / Noa / Gala) in a clean full-party save, stride
  `0x1E0` = 15 x 16-colour sub-CLUTs, one per disc mesh object. It sits at `arena_base + 0x4048`;
  the work arena is zeroed at load by the `sw $zero` loop at SCUS `0x80055F14`
  (`base = *(0x8007BD3C)`, `0x1e8d` words).
- Character-intrinsic and produced at battle load: absent in name-entry / front-of-Tetsu /
  load-initiating saves, present as a single copy once the battle is up, byte-identical between the
  Tetsu and Drake fights.
- Assembler `FUN_80053B9C` (per-colour store `sh a0, 0x894(v0)` at `0x80053C6C`; write-watchpoint
  `autorun_battle_palette_writer.lua`, clean Tetsu fight): copies a source CLUT struct
  `[u16 base][u16 count][BGR555]` to `dst = arena + slot*0x1E0 + (base+idx)*2`, OR-ing `0xFFFF8000`
  (STP / bit 15) onto every non-zero colour. Source pointer
  `s0 = *(*(0x801C92F0)+8) + per-char-off`, a transient `0x800Dxxxx` buffer filled by the LZS decoder
  `FUN_8001A55C` (write-watchpoint on the struct header `0x800D6C98`).
- So the disc form is LZS-compressed and bit-15-**clear** (`0x1D40...`) while the runtime form is
  bit-15-**set** (`0x9D40...`): the palette exists nowhere verbatim, compressed or not (`lzs-decode
  find` over every PROT entry, SCUS and `init_data`; 6372 strict TIMs; LZS output windows to 24 KB).
- Source records: the player file is self-describing relative to `record[0]` (header offsets,
  `[id, running_a, size]` descriptor entries, `id == 0` = section separator), with 5 sub-records
  scattered on disc at `sec_base = rec0 + align_up(recbase - rec0, 0x2000)` (a `0x1000` alignment
  fits Vahn and Noa but lands Gala's subs on a zero-padded `0x7000` block). The `0x2000` stride seen
  in RAM is only the loader's staging buffer. Layout on the owning page.
- Decode: `FUN_8001A55C`'s first argument is an **output-byte budget** (decremented per literal and
  per match-copied byte; loop `while budget > 0`). One `legaia_lzs::decompress(stream, budget)` per
  record, with `record[0]` + the 5 staged sub-records decoded into one work buffer as `FUN_80052FA0`
  does, reproduces Vahn's palette byte-exact in all 3 bands: `base=0x00` = `record[0]`'s CLUT B,
  `base=0x40` = sub#0's trailing CLUT, `base=0x70` = sub#4's trailing CLUT. A budget-less decode runs
  into the next record (29/32 with 3 diffs).
- Noa = PROT 0864, Gala = PROT 0865, by matching each `record0` CLUT against full-party battle VRAM:
  Noa → row 482 98%, Gala → row 483 100% (the 1-2% misses are equipment patches in late-game
  captures). Gala's bands at `0x00` / `0x30` / `0x50` / `0x80` cover all mesh columns. Party order
  from the names at `0x80084708 + n*0x414 + 0x2A7`. Probe `autorun_clut_decode_capture.lua` captured
  the 5 sub-record streams.
- Distinctness: only 10 of Vahn's 130 battle-novel colours (0 of Noa's / Gala's) are in any
  field-pack CLUT; 146 of Vahn's 256 runtime colours are in no CLUT the 1204 pack ships (bundled
  row-492 CLUT vs retail row 492 = 0/256).
- Parser `legaia_asset::battle_char_palette` (`find_record0` + `parse_record` + `collect_palette`:
  record0 CLUT A / B + each separator's `id=0` trailing CLUT + the final record, filtered to the
  columns the mesh samples; STP bit set on upload); disc-gated `battle_char_palette_real` (byte-exact
  against extraction 0863 with `record0` at file offset 0) and
  `noa_gala_collected_palettes_cover_mesh_columns`.

**Port**

- Assembly `legaia_asset::battle_char_assembly`; palette `legaia_asset::battle_char_palette`; PROT
  1204 `legaia_asset::battle_char_pack`. The web viewer's `battle_char_mesh_cba_tsb` returns the
  nominal disc CBA, which is the right pairing for the 1204 authoring-layout (Baka form) render.

**GPU upload path and battle-init reads**

- Scene TIM upload: `FUN_800520F0` (battle loader) → `FUN_800198E0` (per-TIM uploader) →
  `FUN_800583C8` (PsyQ `LoadImage`) → `FUN_8005A1C0` (GPU-queue enqueue, op-type 8 = `FUN_80059BD4`
  via handler table `0x80078D0C`) → ring `0x801C9590` → `FUN_8005A4A0` (once-per-frame flush) →
  `FUN_80059BD4` (GP0 `0xA0` / DMA2; `a0 = RECT{x,y,w,h}`, `a1 = src_ptr`). See
  [`functions.md`](../functions.md).
- Battle-init disc reads are party-independent (PCSX-Redux, Vahn-only vs full-party: byte-identical
  raw-TOC index set; raw → extraction is -2): monster `0x365` → 867; conditional stream + `etim` +
  `etmd` `0x367/8/9` → 869 / 870 / 871; `efect` `0x36B` → 873; `readef` `0x380` → 894; overlay
  `0x384` → 898; `0x37A` → 888; music raw 1016; field-scene re-read `0x5A` → 88. The player files
  stream earlier and no character-CLUT read fires at battle entry.
- Scene-side facts from the same captures: a map01 battle uploads CLUT rows 488 / 490 / 495..499 and
  character image pages 512 / 576 / 640 / 704 / 768 / 832 / 864 / 960 at y=0 through `FUN_80059BD4`;
  the row-490 source is the resident field-scene buffer (`0x800E9690`). `map01` / `map02` sec0 carry
  row 490 as a flag-`0x80000008` 256x1 TIM, whose reserved high bit makes `parse_strict` reject it.
  Per-scene row-49x 16x1 CLUTs (35 scenes incl. town01) are field-actor palettes.
- Not the party palette source: scene-bundle sec0 CLUTs (0400_doman rows 488-492, 0061_dolk, PROT
  1200 `other4` rows 490-494 are field-form / other-pack palettes), the 1204 bundled CLUTs
  (1204 / 1205 / 1206 are uncompressed copies of the same authoring defaults), or a `town0c`
  scene-bundle LZS stream at `0x23430` (that write is the scene bundle decompressing into the shared
  work arena: `0x800ebee8` held `0x7965481F`, not the Vahn palette `0x409d...`).
- Capture tooling:
  [`autorun_clut_upload_hook.lua`](../../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) and
  [`autorun_clut_upload_watch_live.lua`](../../../scripts/pcsx-redux/autorun_clut_upload_watch_live.lua)
  (per-upload `(rect, src)` capture),
  [`autorun_clut_uploader_pc.lua`](../../../scripts/pcsx-redux/autorun_clut_uploader_pc.lua)
  (read-watchpoint that pinned `FUN_80059BD4`),
  [`autorun_find_clut_decode.lua`](../../../scripts/pcsx-redux/autorun_find_clut_decode.lua)
  (LZS-output scanner),
  [`autorun_battle_char_clut_source.lua`](../../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua)
  (disc-read logger) + [`map_clut_disc_reads.py`](../../../scripts/pcsx-redux/map_clut_disc_reads.py).

Owning pages: [`battle-data-pack.md`](../../formats/battle-data-pack.md),
[`character-mesh.md` § Battle form](../../formats/character-mesh.md#battle-form---assembled-from-the-player-files).

### MP-cost ability-bit priority (half vs quarter)

*Status:* resolved (dump-confirmed)

Half wins over quarter, and both subtract a right-shifted copy rather than floor-divide
(`overlay_battle_action_801e295c.txt`).

- The block is inlined twice, byte-identical: `0x801E4568` in state `0x28` (right after that state's
  capture-archive `jal 0x8003EC70` at `0x801E44EC`) and `0x801E3D0C` in state `0x3C` (right after
  that state's Pomander `+0x1DF == 0xFE` case at `0x801E3C4C`).
- Priority: `andi 0x20; bne <half>` then `andi 0x10; beq <none>` =
  `if (bits & 0x20) {half} else if (bits & 0x10) {quarter}`.
- Formula: half = `cost - (cost>>1)` (rounds up on odd costs); "MP-quarter" = `cost - (cost>>2)` =
  pay 3/4, not `cost/4`.
- Port: `battle_formulas::mp_cost_after_ability_bits`, shared by all three cast paths (the two SM
  blocks + `cast_spell_on_slots`); `MpCostModifier::from_ability_flags`. MP cost consumes no RNG.

### Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg)

*Status:* mostly

The opening Tetsu fight is launched by a **dedicated MAN-placed field entity** (the sparring
partner), dialogue-driven, and the engine reaches Battle from a new-game cold boot by walking to the
partner, talking and accepting. One step is staged by the test rather than played: the cold boot
skips the opening sequence's carrier reposition.

- **Launch** (`FUN_801DA51C` + corpus RAM): on reaching SM state 1 the carrier copies its
  `entity[+0x94]` formation into cell `0x8007BD0C` and, via the `case 2/3` fall-through, writes
  `_DAT_8007B83C = 8` (the battle handoff). It is not scene-entry-driven and not an inline arm op: an
  opcode-aware walk of town01's partition-1 scripts finds zero `[1][0x4F]` arm sites.
- **Formation install:** the standard field-VM scripted-battle op `3E FF 04` in `P1[10]` at record
  offset `+0x7F7` (MAN body `0x01B67`) points `actor[+0x94]` at town01 MAN formation index 4 = the
  lone monster archive id `0x4F` (Tetsu), `EncounterRecord::rim_elm_training()`. Same case-`0x3E`
  direct-install arm as garmel's Zeto (`3E FF 09`) and rikuroa's Caruban (`3E FF 11`). It sits in the
  post-"Come at me!" branch (`WaitFrames 16` + flag sets ahead of it; the adjacent `Test 0x227` /
  `JmpRel` targets land on op boundaries). Test
  `rim_elm_sparring_carrier.rs::town01_p1_10_carries_the_tetsu_3e_ff_04_install`.
- **Carrier placement:** town01 P1 placement at tile (76, 65), model `0x6A` - its post-tutorial
  village spot, in a sub-area not walk-reachable from the spawn (BFS: 2855 reachable sub-cells,
  carrier not among them).
- **Opening reposition:** `P1[10]` (`start 0x01370`) carries, twice, at record offsets `+0x1D` /
  `+0x28` (MAN body `0x0138D` / `0x01398`), the op `4C 51 15 0E 07 22` = `MenuCtrl` nibble-5
  `NpcRun { x_enc: 21, z_enc: 14, depth: 7, move_id: 0x22 }` (`field_disasm`
  `MenuCtrlKind::Nibble5NpcRun`). Tile `(21,14)` → world `(21*128+64, 14*128+64)` = `(2752, 1856)` =
  `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS`; `P1[10]` is the unique record NpcRun-ing there. The two
  identical ops are the story-flag two-branch scene-entry prologue. The dialogue-accept capture's
  `actor[+0x90]` resolves to the `(76,65)` / `0x6A` record. Not op `0x23 MOVE_TO`: its only hits are
  false decodes in the desyncing dialog region.
- **Frames:** the runtime actor frame equals the MAN placement frame - `FUN_8003A1E4` spawns at
  `tile*128 + 0x40` via `FUN_80024C88` with no anchor, and the player cold-spawn `0xA40` is
  `tile 20*128 + 0x40`.
- **Talk and Yes / No:** talking is a button press, not a field-VM opcode (op `0x3E` with
  `op0 < 100` is the scripted-battle install). The Yes / No is an MES-embedded option picker inside
  the NPC's inline `0x1F` dialog segment: a `0x29` menu-open followed by an `N*2`-byte signed
  relative-jump table (handler `FUN_80038050`, the `FUN_80039B7C` dialog-SM family), with
  `new_pc = (open + 1 + index*2) + i16_LE(entry[index])`. There is no read-and-compare opcode, which
  is why these records desync under linear disassembly. Port `legaia_mes::Picker::jump_target` +
  `InlineDialogueRunner::last_choice` (`crates/engine-dialog/src/inline_dialogue.rs`).
- **Engine:**
  - Carriers derive from the scene MAN (`man_field_scripts::derive_field_carriers` +
    `World::install_field_carriers_from_man`) and tick through `tick_field_carriers`.
  - `World::tick_field_interaction_probe` (`FUN_801cf9f4`) runs the `DAT_801f2254` facing probe: a
    radius-64 compass point ahead of the player's facing, box-tested at +/-72 against the talkable
    NPCs' placement positions (`World::npcs.positions`). On the action button it talks to the match
    and turns the player toward it.
  - Talking arms the engage (`World::carriers.slots` → `World::carriers.pending_engage`); accepting
    the prompt (the `0x4C` n5 sub-4 dialog dismiss) engages it.
  - `World::nav_step_toward` walks a BFS route over the real collision grid.
- **Tests:** `training_battle.rs` (formation index, carrier SM, dialogue-accept,
  `training_reaches_battle_via_interaction_probe`);
  `v0_1_playthrough.rs::v0_1_battle_leg_reaches_battle_from_new_game` (`BootSession::begin_new_game`
  seeds Vahn at 180 HP - the new-game state is retail's pre-fight state) and
  `v0_1_battle_leg_walk_talk_accept` (walk → talk → accept, no teleport; it places the carrier at
  `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` first, a ~6-tile reachable hop from the spawn).

### The battle-intro enemy-name banner

*Status:* resolved - the question had a false premise.

No placement record raises it: the flow-`0x0A` composer `FUN_801D9D3C` (sole reference in the corpus:
the `jal` at `0x801D0DFC`) lays the labels out itself and hands each to the text-actor spawner
`FUN_8003541C` with immediate geometry.

- One label per distinct monster group: id = group index `0..=3`, class `0`, kind `3`, pen
  `(laid-out x, 48)`, box `measured width x 12`.
- The only placement-table field the intro touches is record 67's `+0x14` string-pointer cell (the
  back-attack / pre-emptive line), drawn under id `4` at pen `(16, 12)`, 288 wide, as literals.
  Record 67 proper is opened only afterwards, by the post-intro sub-draw, under its `+0x01` id
  `0x2B`. The disc `w = 0` width overwrite belongs to record 68 (spawned at the measured name
  width), not to the intro.
- Capture (`scripts/pcsx-redux/autorun_battle_intro_banner.lua`, breakpointing the spawner and the
  teardown sweep `FUN_800355F0`, walking the text-actor list at `gp[+0x148]`): an ambush raises
  `Queen Bee` at `(176, 48)` 55 wide, `Killer Bee * 3` at `(78, 48)` 79 wide and `Ambushed!` at
  `(16, 12)` 288 wide, all from `$ra` inside the composer, held for 120 frames; an ordinary
  encounter raises two labels for 90 frames and no line. Nothing glides - the composer calls neither
  `FUN_801D8DE8` nor the glide `FUN_801DB7B0`.
- One monster suppresses the banner: monster-slot-0 id `0xB5` (evolved Cort) skips the composer and
  parks `ctx[+0x06] = 0x0C`, which the flow ladder at `0x801D0C84` has no arm for; the boss stage
  module moves it on (see [The evolved-Cort flow park](#the-evolved-cort-flow-park)).

Owning page: [`battle.md`](../../subsystems/battle.md#the-battle-intro-enemy-name-banner) (full law,
naming rule and seat arithmetic).

### What a port owes the slot-B module band

*Status:* resolved as a per-address verdict. Evidence: `disassembly`

Of the 65 catalogued addresses in PROT 0903..0966:

| Count | What |
|---|---|
| 58 | the module's `0x801F6734` move-VM spawn-stager entry |
| 6 | cast tick bodies reached from the module's own `0x801CF56C` trampoline |
| 1 | a framed routine nothing references |

By content:

- 45 are pure spawn choreography - an arm switch whose arms only call `FUN_80021B04` /
  `FUN_80050ED4` / `FUN_801DFDF0` / `FUN_80024E80` with a module-resident record and a scale literal
  - expressible as the spawn-record data layer.
- 13 carry game logic: two *stagers* apply damage (PROT 0927 through `FUN_801DD0AC(0x12, 7, seat)`,
  PROT 0966 through `FUN_801DD4B0(0x100, ..)`, each with the HP `+0x14C` clamp); four more write
  actor or `ctx` state.
- 7 are empty (`jr ra; nop`).

Owning page: [`cast-module.md`](../../subsystems/cast-module.md#the-band-as-a-port-worklist)
(per-address table).

### readef groups 19..21 and monster record `+0x1C`

*Status:* resolved-negative - nothing names them. Evidence: `disassembly` + disc census

Monster record `+0x1C` is the readef animation-group index, and no shipped record selects groups
19..21.

- Reader: the initiative scheduler `FUN_801DABA4` reads it record-direct (`lbu v1,0x1c(v0)` at
  `0x801DB098` / `0x801DB0C8`) and seeds the streaming applier's base slot with `3 * group`.
- Census over the 186 populated records: the byte stays in `0..=25` and never takes 1, 2, 5, 12, 19,
  20, 21 or 23; group 0 is the default.
- Second reader: the AI spell picker `FUN_801E9FD4` (`0x801EBB90`) compares the first enemy seat's
  byte against `0x17`; no shipped record satisfies it.
- Groups 19..21's duplicated actor records are therefore unreachable through the enemy path (slots
  58 / 59 / 71 and 64 / 65 / 68 are byte-identical).
- Parser field `readef_group` in `legaia_asset::monster_archive`.

Owning page: [`summon-readef.md`](../../formats/summon-readef.md#which-monsters-name-which-readef-group).

### The dome panel-still arming

*Status:* resolved - a generic battle-end teardown, not a dome condition. Evidence: `disassembly`

- `ctx[+0xC] = 1` is written at `0x800474CC` in the per-frame anim-node tick `FUN_80047430`, for an
  enemy node (`node[+0x5A] >= 3`) under `gp[+0xA48] & 0x80` and `gp[+0x9F4] != 0xB5`, together with
  `node[+0x10] |= 8`.
- `ctx[+0xC]` is a three-value teardown machine: `1` frees the actor table and writes `2`; `2` ticks
  `FUN_80025358`, which stages PROT 0978 into the freed space - from three sites (`0x8004E65C`
  escape, `0x8004F82C` victory tail, `0x80056428` in `FUN_80056208`).
- `ctx[+0x7] == 0x67` is only the successful-escape hold written by case `0x66` of `FUN_801E295C`.
- Open: what *draws* the still.

Owning page: [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md#what-arms-the-load).

### The Miracle marker is an equipment byte

*Status:* resolved - there is no input recognizer. Evidence: `disassembly` (+ `capture` for the card
corroboration)

- `ctx[+0x25F + slot]` has exactly one store in the dump corpus: `sb v1,0x25f(v0)` at `0x80054270`
  inside the SCUS party battle-actor seeding routine `FUN_80053CB8`.
- The value is the acting character's Ra-Seru equipment byte: record-relative `+0x199`, or `+0x198`
  for roster character id `2` (`beq v0,a3` at `0x800541E4`; the weapon-index table `_DAT_8007B42C` =
  `2, 3, 2` names the *other* member of the pair).
- The marker gates all four special art records at `0x801EF4C8`, not only the Miracle.
- Memory cards: the byte is small and per-character banded, zero for a member not yet bonded.

### Two battle-context bytes read wrong

*Status:* resolved. Evidence: `disassembly`

- `ctx[+0x26]` is the **level-up banner's UI element id** (`0x65`). Its reader `0x801E61B4` passes
  the byte as `FUN_801D8DE8`'s element-id argument in a run of sibling unloads with literal ids;
  writers `0x801E723C` (the only assignment) and `0x801E6D3C` (an increment); cleared at
  `0x801E2CFC`. It drives the Done band's `0x50` seed override (`0x96` for `0x3C`) and the `0x51`
  banner skip (59 unskippable frames, then a press).
- `ctx[+0xD]` is the **per-action battle-camera angle variant** `0..=3`: a four-way switch at
  `0x801D6510` / `0x801D6698` / `0x801D689C` inside `FUN_801D5854`, seeded `rand() % 4` at ActionSeed
  and narrowed per category from jump table `0x801CF144`.

### Stone and Curse in the SCUS effect applier

*Status:* resolved. Evidence: `disassembly`

- `FUN_800402F4`'s first-level jump table at `0x80014FA0` (132 entries, guard `sltiu 0x84`) sends
  class `9` to the Stone arm and class `10` to Curse.
- Stone makes three stores after the accuracy roll: `+0x16E |= 4`; a refund of the target's reserved
  item through `FUN_800421D4`, gated on `+0x1DE == 1 && +0x16C != 0` (`+0x16C` is the initiative key,
  not a cooldown); then `+0x1DE = 0`. Curse sets its bit only.
- No item-effect record (`0x800752C0`, 130 records) carries class `9` or `10`, so both arms are
  reachable only from the streamed capture-class modules.

### Attack x2, the swing class and the apply-mode arms

*Status:* resolved. Evidence: `disassembly` + measurement

- `ctx[+0x16]` has two writers, both in the strike loop and both keyed on the War God Icon (record
  `+0xF4 & 0x2000`):
  - the end-of-stream refill at `0x801E3A20..0x801E3A64` runs only while the counter is zero (`bnez`
    at `0x801E3A18`), lifts it to `1`, rewinds the strike cursor and rewrites every marked queue slot
    to `0x19`;
  - the stage site's tail bumps it at `0x801E37AC..0x801E37BC`, only when it is already non-zero
    (`beqz` at `0x801E37B4`), so the second pass's first stage lifts it to `2` - the value that ends
    the damage kernel's carry arm
    ([`battle-action.md`](../../subsystems/battle-action.md#the-war-god-icons-per-stage-bump)).
- Monster record `+0x1E` is the limb-vs-height **swing class**, read through `0x801C9348` by
  `FUN_801EED1C` (class `2` gets one low swing) and by `FUN_801EC3E4` twice: its head miss gate
  (`0x801EC488..0x801EC554`: a party hit of the wrong class does nothing but raise `ctx[+0x263]`) and
  its apply-mode look-ahead (class `2` connects only with power bytes `0x01..=0x10`, class `3` only
  with `0x11..=0x15`).
- Across the archive's 186 records the class reads `0` x127, `1` x1, `2` x52, `3` x6, so both arms
  are ordinary play. Both kernel copies gate everything on a monster target.

### How a cast reaches its slot-B module

*Status:* resolved. Evidence: `disassembly`

Both PROT 0898 tick dispatchers key the same 64-entry band.

| Dispatcher | Key | Table → PROT |
|---|---|---|
| `FUN_801F1ED4` | queued action id | `0x801CF4EC` row `id - 0x81` = PROT `903 + row` |
| `FUN_801F2160` | spell record's `+0x01` byte | `0x801CF56C` row `sub` = PROT `935 + sub` |

- `FUN_801F2160` has a single call site, `0x801E50C8`, in battle phase `0x70`'s hold.
- Data half: the spawn records the modules pass `FUN_80050ED4` / `FUN_80021B04` are recovered with
  the shared `summon_overlay` reader into an engine pool (`legaia_asset::cast_effect_pool`), staged
  at both retail seams.
- Code half: module tick bodies are ported in `engine-vm` (`cast_module_ticks`, `cast_arm_ticks`,
  `cast_seru_ticks_a` / `_b`, `cast_module_camera`); which modules' bodies and camera arms remain is
  listed per module on [`cast-module.md`](../../subsystems/cast-module.md).

### What an art body costs in AP

*Status:* resolved - computed, not disc data. Evidence: `disassembly`

`FUN_801EED1C` derives it:

- multiplier `11` / `10` / `6` by the builder's *visit* ordinal (`0`, `1..3`, `>= 4`) - not the
  arts-grid display index;
- halved before the multiply under the actor's `0x800` flag (`srl` at `0x801EF378`);
- times the art's command count;
- accrued into `actor[+0x224]` and spent from Spirit `actor[+0x170]` at `0x801E5D74`.

Port: `engine-core::ap_gauge::art_spirit_cost`.

### Tutorial prompt machine exclusivity

*Status:* resolved - the machine is byte-exclusive to 0967; the prompt pool is shared with 0968. Evidence: `disassembly`

The sparring-tutorial prompts are resident in stage overlay 967, so the battle SM alone never emits
them.

- Machine: `FUN_801F6B70` is entry 967 file `+0x198` (`0x801F69D8 + 0x198` reproduces the VA), a
  91-entry jump-table hook on `ctx[+0x06]` with nine live slots, each switching on `ctx[+0x28A]`.
- Exclusivity: five needles from 48 to 2316 bytes - one a 116-byte window free of `lui` / `j` /
  `jal`, which a relinked copy could not evade - return one physical copy across every PROT entry,
  SCUS and DMY.DAT (967 is stored raw).
- Pool: entry 0968 carries a byte-identical 852-byte prefix of the prompt pool at the same offset
  `0xCAC`, inside a `0x5D8`-byte run the two entries share; 0968 has its own 7-entry dispatcher and
  no copy of the machine.
- Port: `engine-core::battle_tutorial`.

[details ↓](../../subsystems/battle.md#the-sparring-tutorial-prompt-machine-overlay-967)

### The _DAT_8007BA78 census is closed

*Status:* resolved. Evidence: `disassembly`

A sweep in every reference form over SCUS, all 1233 PROT entries and the overlay images finds every
reference to `_DAT_8007BA78`:

- two stores: `0x801E30F4` = PROT 0897 `+0x148DC` (the `4C E2` op) and `0x801DDCE8` = PROT 0899
  `+0xF4D0` (the title tick);
- four loads, all in PROT 0970;
- one literal-word reference at PROT 0971 `+0x1238` = `0x801CFA50`, the debug menu's
  editable-globals pointer table - the static witness for dev-menu editing in the corpus states;
- zero `gp`-relative `0x760(gp)` references (the form an absolute-only scan cannot see).

Limits: a raw scan cannot see code inside an LZS section. A "PROT 0896 carries the same span shifted
by `0x9000`" reading is an entry-size over-read artifact.

Owning page: [cutscene.md](../../subsystems/cutscene.md).

### The 0xF8 halt-acquire handshake

*Status:* resolved - retail machine traced; the port's model diverges. Evidence: `disassembly`

`0xF8` resolves to the player object `_DAT_8007C364` (`FUN_8003C83C`: `li v0,0xf8` /
`lw v0,-0x3c9c(v0)`; inlined again at `0x800377A0` and `0x80037E04`).

- **ExecMove arms nothing:** `0x801DE998` writes only `+0x5C` / `+0x5E` / `+0x56` and calls
  `FUN_800204F8`.
- **The halt-acquire creates the wait object:** `0x801DF384` sets `+0x94` and ORs `0x400` into
  `+0x10` (plus the caller's when the target is the player, at `0x801DF404`); `0x801DF5AC jal
  0x801d25ec` spawns the glide actor and the release helper `0x801D5D60`, which polls
  `andi v0,v0,8` at `0x801D5DB4` and clears the halt at `0x801D5DD4` / `0x801D5DFC`.
- **The record parks at the next cross-context op,** in the prologue busy gate
  `0x801DE90C..0x801DE944`, whose unadvanced PC the run loop reads as "stop" (`0x8003CFF0`). The
  acquire itself advances 9 bytes (11 for sub-A/B), 0 on failure.
- No backward resume PC: the operand `+3` / `+5` halfwords are `FUN_801D25EC` tween arguments.

Owning page: [cutscene.md](../../subsystems/cutscene.md).

### Growing a scene TMD pack member

*Status:* resolved - rebuild is the only cost. Evidence: `disassembly`

Every external reference to a pack member is an index, so a member can grow if the pack is rebuilt.

- The mesh pool is the descriptor walk's registration order (`FUN_8001F05C` →
  `FUN_80026B4C(buf + offsets[i] * 4)`); placements name a pool slot; scene ANM records name a record
  number; the bundle's descriptors hold offsets into the bundle entry, not the separately streamed
  pack.
- Disc-wide, each of PROT 0639's members `106 / 107 / 108` word offsets occurs exactly once (in the
  pack's own table), the byte-offset form zero times, and the declared length `347476` zero times
  outside its chunk header.

Owning page:
[`man-relocation.md`](../../formats/man-relocation.md#the-same-question-for-an-assetpack-growing-a-mesh-member).

### What of a signature cast is data

*Status:* resolved (partially). Evidence: `disassembly`

A signature cast's spawn records are data; its lift is code.

- At all 15 / 41 / 24 `jal FUN_80050ED4` sites in PROT 0958 / 0959 / 0960 the record pointer `a2` is
  a module-resident constant and `a3` a scale literal; the records are the summon part-record shape
  the art path's `0x801F6324` prototypes use.
- Moving them into the art path is blocked by residency and capacity: the prototype table's 61
  entries are all populated, the module blocks sit above the slot-B base while the prototypes sit
  below it, and the battle overlay has 247 bytes of zero slack.
- The lift is `sb` into `+0x1DA` (16 / 6 / 8 sites) from the modules' phase arms, which no record
  can express.

Owning page:
[`cast-module.md`](../../subsystems/cast-module.md#what-of-the-choreography-is-data-and-what-is-code).

### Op `0x23` MOVE_TO keys on ctx identity

*Status:* resolved. Evidence: `disassembly`

The player arm is chosen by ctx *pointer* identity, not by the player-class bit.

- `0x801DEC7C bne s5,v0` compares the executing ctx against `_DAT_8007C364` (`0x8007C348 + 0x1C`);
  only that identity reaches the camera re-centre `func_0x80017EC8` (`0x801DEC84..0x801DECA8`).
- Every other ctx takes `0x801DECAC`'s facing + movement-init arm on its own actor.
- A spawned partition-2 record inherits the `0x1000000` player-class bit, so keying on the bit
  teleports the player to wherever the record seats its own actor (`keikoku`'s arrival record, once
  op `0x45` does not restart it).

### Dome ringside panel stills PROT 1221 and 1222

*Status:* resolved. Evidence: `disassembly`

Two 320x256 BGR555 stills in the dome's `other6` bundle; the pair is picked by a computed index, so
no literal names them.

- Streamed in four `0xA000` strips by `FUN_801F6B24` (PROT 0978) into VRAM `(384, 0)` (rect
  `0x801F735C`); `320*256*2` is the entry size exactly, the first five sectors a 16-line top pad.
- Selection: `addiu a0,s0,0x4c7` at `0x801F6C3C` with `s0 = (party slot 0 current HP < max / 2)`, so
  `int2.tim` is the below-half-HP variant. A literal sweep cannot see a computed index.
- Corroboration: the overlay's dev path strings `h:\prot\field\other6\tim\int.tim` / `int2.tim`,
  selected by the same `s0`.

Owning page:
[`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md#inttim--int2tim---the-ringside-panel-stills).

### The display object's sound-bank category

*Status:* resolved. Evidence: `disassembly`

`actor[+0x22C]` points at the actor's spawned display object, whose `+0x80` is the sound-bank
category.

- The pointer comes from `FUN_80024C88` → `FUN_80020DE0` and is installed at `0x800515E8` /
  `0x8005196C` in `FUN_800513F0`.
- `+0x80` is zeroed at spawn (`0x80020F50`) and set at battle setup to `7` (party arm,
  `0x80051548`) or `7` / `8` (the four enemy seats, loops at `0x80052238..` and `0x800522D0..`), each
  loop paired with `jal 0x8003E104` carrying the same literal in `a1` - the bank-load slot.
- The router copies it into the descriptor's category column (`0x8004FFD8..0x8004FFE4`,
  `0x80050070..0x8005007C`).
- The monster element byte is record `+0x1D`, a different field.

### The evolved-Cort flow park

*Status:* resolved. Evidence: `capture` + `disassembly`

The boss stage module, not the flow ladder, moves flow `0x0C` on.

- The `0x0A` arm writes `0x0B` unconditionally and overwrites it with `0x0C` for formation `0xB5`
  (`0x801D0DE0..0x801D0E14`); the ladder idles on `0x0C`.
- PROT 0968 (loader-B tracker `0x49`) runs a 7-phase intro cinematic off `ctx[+0x289]` (jump table
  at `0x801F69D8`) and writes `0x0B` back at `0x801F713C` (`ra = 0x800564A0`) when a dt countdown on
  its local word `0x801F73F8` expires - 3207 vsyncs later in the capture, with
  `0x0B → 0x14 → 0x1E` following at once.
- A ten-button sweep produces zero writes: no input shortens it.
- Probe `autorun_w4d_cort_flow_writer.lua`.

Owning page: [`battle.md`](../../subsystems/battle.md#flow-0x0c-is-the-boss-stage-modules-baton).
