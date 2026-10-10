# Settled reverse-engineering threads

Reverse-engineering questions about Legaia's runtime that have an answer, with
the evidence that answer rests on. Each thread is a question, its answer, an
evidence grade, a provenance citation (function or instruction address, PROT
entry, capture) and a link to the format or subsystem page that owns the
detail. This is the archive half of the register: the live hunts are on
[`open-rev-eng-threads.md`](open-rev-eng-threads.md), and the disproved readings
on [`re-do-not-re-walk.md`](re-do-not-re-walk.md).

This page is the index. The threads themselves live in one page per area under
[`re-settled-threads/`](re-settled-threads/battle.md):

| Area | Page | Covers |
|---|---|---|
| [World map / kingdom bundles](#world-map--kingdom-bundles) | [`world-map.md`](re-settled-threads/world-map.md) | Overworld ground, fog and decoration rendering; kingdom bundle slots |
| [Battle / arts / level-up](#battle--arts--level-up) | [`battle.md`](re-settled-threads/battle.md) | Battle flow, HUD, camera, damage and status rules, arts, casts and summons, spoils |
| [Field / locomotion](#field--locomotion) | [`field.md`](re-settled-threads/field.md) | Movement, collision, doors, scene scripts and story flags, field camera, menus, shops, minigames |
| [Text / fonts / dialog](#text--fonts--dialog) | [`text-dialog.md`](re-settled-threads/text-dialog.md) | Dialog boxes, pickers, fonts, the pause Items / Magic screens |
| [Animation](#animation) | [`animation.md`](re-settled-threads/animation.md) | ANM records, clip playback, move-VM animation ops |
| [Audio](#audio) | [`audio.md`](re-settled-threads/audio.md) | BGM sequencing, SFX bank routing, SPU pitch and reverb, XA streams |
| [Title / boot / overlays](#title--boot--overlays) | [`title-boot-overlays.md`](re-settled-threads/title-boot-overlays.md) | Boot and title flow, game modes, overlay identity and load bases, new-game seed, save data |
| [Rendering / camera](#rendering--camera) | [`rendering-camera.md`](re-settled-threads/rendering-camera.md) | Primitive dispatch, lighting, depth cue, ordering table, camera roll |
| [Measurement + corpus](#measurement--corpus) | [`measurement-corpus.md`](re-settled-threads/measurement-corpus.md) | What the dump corpus, coverage instruments and audits do and do not show |

Read a row before starting work that depends on it. `resolved` is a claim, not
a warranty - which is what the evidence column is for.

## The evidence column

Every row is graded by what its own stated evidence actually rests on. Where a
row cites more than one kind, it is graded by the **weakest load-bearing**
claim, because that is the one that breaks the conclusion if it is wrong.

| Grade | The row cites |
|---|---|
| `disassembly` | Instructions, addresses, opcode encodings, branch or store sequences. The strongest grade. |
| `capture` | A runtime capture, save state, probe, firehose, or disc-derived oracle. |
| `decompiled-C` | Ghidra's C output, a `FUN_x(...)` call signature, a Ghidra label or plate comment, or a claim about store order / store count / a boolean operator with no instruction behind it. |
| `inference` | Reasoning from surrounding facts, corpus absence, or analogy, with no direct evidence cited. |

**`decompiled-C` is the re-audit bucket, not a wrong-answer bucket.** It marks
a claim nobody has confirmed against instructions. Most are probably right;
the point is that none of them has been checked, and the claims that re-audits
have falsified were of this grade. Three shapes carry most
of that risk - evidence citing a `FUN_x(a, b)` call signature or a
`funcs/<addr>.txt` dump rather than instructions; any claim about store
*order* or store *count*; and any claim about which boolean operator a
predicate uses. The artifact catalogue is
[`ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

`inference` is not weaker than `decompiled-C` so much as differently exposed:
an inference row usually says so out loud ("the structural rule supersedes the
snapshot", "no image claims them as functions"), and its failure mode is a
missing counter-example rather than a misread instruction.

## How a thread is laid out

Each area page opens with a short summary and a list of its detailed write-ups,
then one table of threads: `Thread | Status | Evidence | Answer`. A thread whose
answer needs more than a table cell keeps its one-liner in the table and links
to a `###` section further down the same page via **[details ↓]**. Such a
section carries a *Status:* line, the answer, the load-bearing addresses and
constants, the port location where one exists, and the owning page.

The sections below list each area's detailed write-ups. The anchors on this page
are the ones those write-ups had when the register was a single page, so an
older link to `re-settled-threads.md#<thread>` lands on the row that links to
the write-up's current home.

---

## World map / kingdom bundles

Threads: [`re-settled-threads/world-map.md`](re-settled-threads/world-map.md).

| Detailed write-up |
|---|
| <a id="world-map-walk-view-continent-ground-render"></a>[World-map walk-view continent ground render](re-settled-threads/world-map.md#world-map-walk-view-continent-ground-render) |
| <a id="field-decoration-path---does-it-dispatch-the-ncc-light-handlers"></a>[Field decoration path - does it dispatch the NCC light handlers?](re-settled-threads/world-map.md#field-decoration-path---does-it-dispatch-the-ncc-light-handlers) |
| <a id="kingdom-slot-4---per-record-semantic"></a>[Kingdom slot 4 - per-record semantic](re-settled-threads/world-map.md#kingdom-slot-4---per-record-semantic) |
| <a id="dat_8007c018-liveness-rule"></a>[DAT_8007C018 liveness rule](re-settled-threads/world-map.md#dat_8007c018-liveness-rule) |

## Battle / arts / level-up

Threads: [`re-settled-threads/battle.md`](re-settled-threads/battle.md).

| Detailed write-up |
|---|
| <a id="the-battle-hit-tint---what-a-landing-hit-writes-and-how-it-reaches-the-pixel"></a>[The battle hit tint - what a landing hit writes and how it reaches the pixel](re-settled-threads/battle.md#the-battle-hit-tint---what-a-landing-hit-writes-and-how-it-reaches-the-pixel) |
| <a id="battle-end---the-results-sequencers-timeline-and-who-strikes-the-pose"></a>[Battle end - the results sequencer's timeline, and who strikes the pose](re-settled-threads/battle.md#battle-end---the-results-sequencers-timeline-and-who-strikes-the-pose) |
| <a id="what-a-normal-party-attack-sounds-like"></a>[What a normal party attack sounds like](re-settled-threads/battle.md#what-a-normal-party-attack-sounds-like) |
| <a id="fun_8003eae4-is-a-seek-plus-bookkeeping---and-the-driver-is-not-untraced"></a>[`FUN_8003EAE4` is a seek plus bookkeeping - and the driver is not untraced](re-settled-threads/battle.md#fun_8003eae4-is-a-seek-plus-bookkeeping---and-the-driver-is-not-untraced) |
| <a id="formation-species-limit---what-the-battle-setup-does-with-3-distinct-monster-ids"></a>[Formation species limit - what the battle setup does with 3 distinct monster ids](re-settled-threads/battle.md#formation-species-limit---what-the-battle-setup-does-with-3-distinct-monster-ids) |
| <a id="attack-band-damage-pacing---the-hit-event-model"></a>[Attack-band damage pacing - the hit-event model](re-settled-threads/battle.md#attack-band-damage-pacing---the-hit-event-model) |
| <a id="the-battle-huds-per-phase-surfaces-are-the-sub-draw-script-table"></a>[The battle HUD's per-phase surfaces are the sub-draw script table](re-settled-threads/battle.md#the-battle-huds-per-phase-surfaces-are-the-sub-draw-script-table) |
| <a id="the-rings-element-chip-reads---or-the-ra-seru-and-the-gate-is-the-equipment-byte"></a>[The ring's element chip reads `-` or the Ra-Seru, and the gate is the equipment byte](re-settled-threads/battle.md#the-rings-element-chip-reads---or-the-ra-seru-and-the-gate-is-the-equipment-byte) |
| <a id="the-combo-counters-glide-is-placement-record-80s"></a>[The combo counter's glide is placement record 80's](re-settled-threads/battle.md#the-combo-counters-glide-is-placement-record-80s) |
| <a id="the-chrome-kind-byte-is-an-index-into-the-widget-class-table"></a>[The chrome `kind` byte is an index into the widget-class table](re-settled-threads/battle.md#the-chrome-kind-byte-is-an-index-into-the-widget-class-table) |
| <a id="the-element-badge-palette-selector"></a>[The element-badge palette selector](re-settled-threads/battle.md#the-element-badge-palette-selector) |
| <a id="the-status-element-badge-sheet-0x180x20"></a>[The status-element badge sheet (`0x18..=0x20`)](re-settled-threads/battle.md#the-status-element-badge-sheet-0x180x20) |
| <a id="battle-intro-tile-shatter---the-side-face-shade-page"></a>[Battle-intro tile shatter - the side-face shade page](re-settled-threads/battle.md#battle-intro-tile-shatter---the-side-face-shade-page) |
| <a id="battle-intro-transition-length---dat_801d2458"></a>[Battle-intro transition length - `DAT_801D2458`](re-settled-threads/battle.md#battle-intro-transition-length---dat_801d2458) |
| <a id="battle-ground-grid-depth-cue---the-far-colour"></a>[Battle ground grid depth cue - the far colour](re-settled-threads/battle.md#battle-ground-grid-depth-cue---the-far-colour) |
| <a id="who-calls-the-battle-on-screen-test-fun_8005126c"></a>[Who calls the battle on-screen test `FUN_8005126C`?](re-settled-threads/battle.md#who-calls-the-battle-on-screen-test-fun_8005126c) |
| <a id="action-sm-state-0xff-treated-as-battle-end-by-the-port"></a>[Action-SM state `0xFF` treated as battle end by the port](re-settled-threads/battle.md#action-sm-state-0xff-treated-as-battle-end-by-the-port) |
| <a id="endless-camera-orbit---the-0x19-attack-approach-park"></a>[Endless camera orbit - the `0x19` attack-approach park](re-settled-threads/battle.md#endless-camera-orbit---the-0x19-attack-approach-park) |
| <a id="the-in-fight-action-framing-and-the-yaw-counter-ladder"></a>[The in-fight action framing and the yaw-counter ladder](re-settled-threads/battle.md#the-in-fight-action-framing-and-the-yaw-counter-ladder) |
| <a id="the-done-band-framing-and-the-two-orbit-writers"></a>[The Done band framing and the two orbit writers](re-settled-threads/battle.md#the-done-band-framing-and-the-two-orbit-writers) |
| <a id="the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2"></a>[The summon-then-melee park trigger - the stale field is `+0x1DC` bit 2](re-settled-threads/battle.md#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2) |
| <a id="super--miracle-arts-trigger-chain"></a>[Super / Miracle Arts trigger chain](re-settled-threads/battle.md#super--miracle-arts-trigger-chain) |
| <a id="super-art-connectors-and-physical-inputs---the-tokenizer-keeps-the-leading-arrows"></a>[Super Art connectors and physical inputs - the tokenizer keeps the leading arrows](re-settled-threads/battle.md#super-art-connectors-and-physical-inputs---the-tokenizer-keeps-the-leading-arrows) |
| <a id="the-runtime-art-record-chase---the-4-and-the---0x10-grid-origin"></a>[The runtime art-record chase - the `+4` and the `- 0x10` grid origin](re-settled-threads/battle.md#the-runtime-art-record-chase---the-4-and-the---0x10-grid-origin) |
| <a id="character-record-hpmpap-pair-order"></a>[Character-record HP/MP/AP pair order](re-settled-threads/battle.md#character-record-hpmpap-pair-order) |
| <a id="effect-vm-pass-1-state-token-algebra-fun_801e0088"></a>[Effect-VM pass-1 "state token algebra" (`FUN_801E0088`)](re-settled-threads/battle.md#effect-vm-pass-1-state-token-algebra-fun_801e0088) |
| <a id="battle-face-stamp-issuing-site"></a>[Battle face-stamp issuing site](re-settled-threads/battle.md#battle-face-stamp-issuing-site) |
| <a id="spine-flag-0x482-drake-mist-wall-writer"></a>[Spine flag `0x482` (Drake mist-wall) writer](re-settled-threads/battle.md#spine-flag-0x482-drake-mist-wall-writer) |
| <a id="flag-0x63a---the-vellvozz-p27-gate-with-no-script-writer"></a>[Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer](re-settled-threads/battle.md#flag-0x63a---the-vellvozz-p27-gate-with-no-script-writer) |
| <a id="spine-flag-0x142-caruban-beat--dolk-dolk2-switch-writer"></a>[Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer](re-settled-threads/battle.md#spine-flag-0x142-caruban-beat--dolk-dolk2-switch-writer) |
| <a id="drake-castle-deep-interiors-jouincjouind-depth-decode"></a>[Drake Castle deep interiors (`jouinc`/`jouind`) depth decode](re-settled-threads/battle.md#drake-castle-deep-interiors-jouincjouind-depth-decode) |
| <a id="cave01-p216-spawner---the-slot-counted-interact-chain"></a>[cave01 P2[16] spawner - the slot-counted interact chain](re-settled-threads/battle.md#cave01-p216-spawner---the-slot-counted-interact-chain) |
| <a id="npc-dynamic-facing---two-laws-and-an-execution-order"></a>[NPC dynamic facing - two laws and an execution order](re-settled-threads/battle.md#npc-dynamic-facing---two-laws-and-an-execution-order) |
| <a id="0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis"></a>[0x4C 0x51 byte +3 reconcile - facing wins; no motion-bytecode synthesis](re-settled-threads/battle.md#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis) |
| <a id="kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker"></a>[kor-family op-0x49 flag window [0x138..0x13F] - Uru Mais warp-pad picker](re-settled-threads/battle.md#kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker) |
| <a id="encounter-man-sub-section-layout"></a>[Encounter MAN sub-section layout](re-settled-threads/battle.md#encounter-man-sub-section-layout) |
| <a id="seru-magic-summon-visual-eg-tail-fire"></a>[Seru-magic summon visual (e.g. Tail Fire)](re-settled-threads/battle.md#seru-magic-summon-visual-eg-tail-fire) |
| <a id="the-party-cast-trigger-is-a-params-stager"></a>[The party cast trigger is a params stager](re-settled-threads/battle.md#the-party-cast-trigger-is-a-params-stager) |
| <a id="player-summon-presentation"></a>[Player-summon presentation](re-settled-threads/battle.md#player-summon-presentation) |
| <a id="the-fade-actors-hold-word"></a>[The fade actor's hold word](re-settled-threads/battle.md#the-fade-actors-hold-word) |
| <a id="summondat--readefdat-side-band-streaming"></a>[`summon.dat` / `readef.DAT` side-band streaming](re-settled-threads/battle.md#summondat--readefdat-side-band-streaming) |
| <a id="monster-steal-item-evil-god-icon"></a>[Monster steal item (Evil God Icon)](re-settled-threads/battle.md#monster-steal-item-evil-god-icon) |
| <a id="per-spell-magic-power--multiplier"></a>[Per-spell magic power / multiplier](re-settled-threads/battle.md#per-spell-magic-power--multiplier) |
| <a id="stat-growth-rate-source"></a>[Stat growth-rate source](re-settled-threads/battle.md#stat-growth-rate-source) |
| <a id="monster-stat-record-archive-source"></a>[Monster stat-record archive source](re-settled-threads/battle.md#monster-stat-record-archive-source) |
| <a id="monster-mesh--texture-pool"></a>[Monster mesh + texture pool](re-settled-threads/battle.md#monster-mesh--texture-pool) |
| <a id="terra-slot-3--story-flag-overlap"></a>[Terra slot-3 / story-flag overlap](re-settled-threads/battle.md#terra-slot-3--story-flag-overlap) |
| <a id="battle-party-meshes--assembled-from-the-player-battle-files-prot-1204--baka-fighter--default-equipment-sibling"></a>[Battle party meshes = **assembled from the player battle files** (PROT 1204 = Baka Fighter / default-equipment sibling)](re-settled-threads/battle.md#battle-party-meshes--assembled-from-the-player-battle-files-prot-1204--baka-fighter--default-equipment-sibling) |
| <a id="mp-cost-ability-bit-priority-half-vs-quarter"></a>[MP-cost ability-bit priority (half vs quarter)](re-settled-threads/battle.md#mp-cost-ability-bit-priority-half-vs-quarter) |
| <a id="scripted-tetsu-encounter--battle-v01-oracle-battle-leg"></a>[Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg)](re-settled-threads/battle.md#scripted-tetsu-encounter--battle-v01-oracle-battle-leg) |
| <a id="the-battle-intro-enemy-name-banner"></a>[The battle-intro enemy-name banner](re-settled-threads/battle.md#the-battle-intro-enemy-name-banner) |
| <a id="what-a-port-owes-the-slot-b-module-band"></a>[What a port owes the slot-B module band](re-settled-threads/battle.md#what-a-port-owes-the-slot-b-module-band) |
| <a id="readef-groups-1921-and-monster-record-0x1c"></a>[readef groups 19..21 and monster record `+0x1C`](re-settled-threads/battle.md#readef-groups-1921-and-monster-record-0x1c) |
| <a id="the-dome-panel-still-arming"></a>[The dome panel-still arming](re-settled-threads/battle.md#the-dome-panel-still-arming) |
| <a id="the-miracle-marker-is-an-equipment-byte"></a>[The Miracle marker is an equipment byte](re-settled-threads/battle.md#the-miracle-marker-is-an-equipment-byte) |
| <a id="two-battle-context-bytes-read-wrong"></a>[Two battle-context bytes read wrong](re-settled-threads/battle.md#two-battle-context-bytes-read-wrong) |
| <a id="stone-and-curse-in-the-scus-effect-applier"></a>[Stone and Curse in the SCUS effect applier](re-settled-threads/battle.md#stone-and-curse-in-the-scus-effect-applier) |
| <a id="attack-x2-the-swing-class-and-the-apply-mode-arms"></a>[Attack x2, the swing class and the apply-mode arms](re-settled-threads/battle.md#attack-x2-the-swing-class-and-the-apply-mode-arms) |
| <a id="how-a-cast-reaches-its-slot-b-module"></a>[How a cast reaches its slot-B module](re-settled-threads/battle.md#how-a-cast-reaches-its-slot-b-module) |
| <a id="what-an-art-body-costs-in-ap"></a>[What an art body costs in AP](re-settled-threads/battle.md#what-an-art-body-costs-in-ap) |
| <a id="tutorial-prompt-machine-exclusivity"></a>[Tutorial prompt machine exclusivity](re-settled-threads/battle.md#tutorial-prompt-machine-exclusivity) |
| <a id="the-_dat_8007ba78-census-is-closed"></a>[The _DAT_8007BA78 census is closed](re-settled-threads/battle.md#the-_dat_8007ba78-census-is-closed) |
| <a id="the-0xf8-halt-acquire-handshake"></a>[The 0xF8 halt-acquire handshake](re-settled-threads/battle.md#the-0xf8-halt-acquire-handshake) |
| <a id="growing-a-scene-tmd-pack-member"></a>[Growing a scene TMD pack member](re-settled-threads/battle.md#growing-a-scene-tmd-pack-member) |
| <a id="what-of-a-signature-cast-is-data"></a>[What of a signature cast is data](re-settled-threads/battle.md#what-of-a-signature-cast-is-data) |
| <a id="op-0x23-move_to-keys-on-ctx-identity"></a>[Op `0x23` MOVE_TO keys on ctx identity](re-settled-threads/battle.md#op-0x23-move_to-keys-on-ctx-identity) |
| <a id="dome-ringside-panel-stills-prot-1221-and-1222"></a>[Dome ringside panel stills PROT 1221 and 1222](re-settled-threads/battle.md#dome-ringside-panel-stills-prot-1221-and-1222) |
| <a id="the-display-objects-sound-bank-category"></a>[The display object's sound-bank category](re-settled-threads/battle.md#the-display-objects-sound-bank-category) |
| <a id="the-evolved-cort-flow-park"></a>[The evolved-Cort flow park](re-settled-threads/battle.md#the-evolved-cort-flow-park) |

## Field / locomotion

Threads: [`re-settled-threads/field.md`](re-settled-threads/field.md).

| Detailed write-up |
|---|
| <a id="opdeene-runs-long-after-its-apply-4800-camera-move"></a>[`opdeene` runs long after its `apply 4800` camera move](re-settled-threads/field.md#opdeene-runs-long-after-its-apply-4800-camera-move) |
| <a id="the-field-follow-cameras-pose-chain"></a>[The field follow camera's pose chain](re-settled-threads/field.md#the-field-follow-cameras-pose-chain) |
| <a id="chapter-1-scene-frontier"></a>[Chapter-1 scene frontier](re-settled-threads/field.md#chapter-1-scene-frontier) |
| <a id="the-uru-mais-chain-and-jouine-exits"></a>[The Uru Mais chain and jouine exits](re-settled-threads/field.md#the-uru-mais-chain-and-jouine-exits) |
| <a id="the-upper-case-destination-fold"></a>[The upper-case destination fold](re-settled-threads/field.md#the-upper-case-destination-fold) |
| <a id="the-count-5-asset-tables"></a>[The count-5 asset tables](re-settled-threads/field.md#the-count-5-asset-tables) |
| <a id="the-ledge-hop-lock-leak"></a>[The ledge-hop lock leak](re-settled-threads/field.md#the-ledge-hop-lock-leak) |
| <a id="clip-end-latch-for-cross-context-clip-pokes"></a>[Clip-end latch for cross-context clip pokes](re-settled-threads/field.md#clip-end-latch-for-cross-context-clip-pokes) |
| <a id="ambient-render-mode-4---the-vram-rect-scroller"></a>[Ambient render-mode 4 - the VRAM-rect scroller](re-settled-threads/field.md#ambient-render-mode-4---the-vram-rect-scroller) |
| <a id="which-op-0x34-sub-3-installs-fire-at-scene-entry"></a>[Which op-`0x34` sub-3 installs fire at scene entry](re-settled-threads/field.md#which-op-0x34-sub-3-installs-fire-at-scene-entry) |
| <a id="master-ambient-record-0---the-per-scene-sfx-descriptor-bank"></a>[Master ambient record 0 - the per-scene SFX descriptor bank](re-settled-threads/field.md#master-ambient-record-0---the-per-scene-sfx-descriptor-bank) |
| <a id="rim-elms-south-gate"></a>[Rim Elm's south gate](re-settled-threads/field.md#rim-elms-south-gate) |
| <a id="townfield-free-movement-locomotion"></a>[Town/field free-movement locomotion](re-settled-threads/field.md#townfield-free-movement-locomotion) |
| <a id="field-collision-map-source"></a>[Field collision-map source](re-settled-threads/field.md#field-collision-map-source) |
| <a id="field-map-prot-resolution---define--2-universal"></a>[Field `.MAP` PROT resolution - `define − 2`, universal](re-settled-threads/field.md#field-map-prot-resolution---define--2-universal) |
| <a id="game_mode-0x03--fieldtown-gameplay"></a>[game_mode 0x03 = field/town gameplay](re-settled-threads/field.md#game_mode-0x03--fieldtown-gameplay) |
| <a id="engine-vram-byte-exactness-for-town01"></a>[Engine VRAM byte-exactness for town01](re-settled-threads/field.md#engine-vram-byte-exactness-for-town01) |
| <a id="world-map-clut-cycling-beyond-the-ocean-head---closed-operand-table--emitter--cadence-all-pinned"></a>[World-map CLUT cycling beyond the ocean head - CLOSED (operand table + emitter + cadence all pinned)](re-settled-threads/field.md#world-map-clut-cycling-beyond-the-ocean-head---closed-operand-table--emitter--cadence-all-pinned) |
| <a id="init_data-ui-tile-pages---journey-dependent-residency-resolved-map03-texture-column-resolved---not-uploaded-premise-falsified"></a>[`init_data` UI-tile pages - journey-dependent residency (resolved); map03 texture column (resolved - "not uploaded" premise falsified)](re-settled-threads/field.md#init_data-ui-tile-pages---journey-dependent-residency-resolved-map03-texture-column-resolved---not-uploaded-premise-falsified) |
| <a id="clut-row-510-population-boot-resident-system-ui-strip-band"></a>[CLUT row 510 population (boot-resident system-UI strip band)](re-settled-threads/field.md#clut-row-510-population-boot-resident-system-ui-strip-band) |
| <a id="scene-transition-0x3f-door-destination-indexing"></a>[Scene-transition (`0x3F` door) destination indexing](re-settled-threads/field.md#scene-transition-0x3f-door-destination-indexing) |
| <a id="intra-town-house--interior-door-mechanism"></a>[Intra-town (house / interior) door mechanism](re-settled-threads/field.md#intra-town-house--interior-door-mechanism) |
| <a id="fieldtown-environment-geometry-placement"></a>[Field/town environment-geometry placement](re-settled-threads/field.md#fieldtown-environment-geometry-placement) |
| <a id="region-story-flag-gate-families"></a>[Region story-flag gate families](re-settled-threads/field.md#region-story-flag-gate-families) |
| <a id="the-play-order-residual"></a>[The play-order residual](re-settled-threads/field.md#the-play-order-residual) |
| <a id="extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer"></a>[Extraction-0874 §2 (`player.lzs`) F-variant pixels - a one-shot opening face-frame stamp, not a menu writer](re-settled-threads/field.md#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer) |
| <a id="what-the-op-0x49-entry-context-kind-byte-is-and-which-screens-it-selects"></a>[What the op-`0x49` entry-context kind byte is, and which screens it selects](re-settled-threads/field.md#what-the-op-0x49-entry-context-kind-byte-is-and-which-screens-it-selects) |

## Text / fonts / dialog

Threads: [`re-settled-threads/text-dialog.md`](re-settled-threads/text-dialog.md).

| Detailed write-up |
|---|
| <a id="pause-itemsmagic-screens---remaining-sub-flows"></a>[Pause Items/Magic screens - remaining sub-flows](re-settled-threads/text-dialog.md#pause-itemsmagic-screens---remaining-sub-flows) |
| <a id="inline-dialog-box-format-0x1f-lead-segments"></a>[Inline dialog-box format (`0x1F`-lead segments)](re-settled-threads/text-dialog.md#inline-dialog-box-format-0x1f-lead-segments) |
| <a id="prot-0892-is-the-card-screen-kanji-font"></a>[PROT 0892 is the card-screen kanji font](re-settled-threads/text-dialog.md#prot-0892-is-the-card-screen-kanji-font) |

## Animation

Threads: [`re-settled-threads/animation.md`](re-settled-threads/animation.md).

| Detailed write-up |
|---|
| <a id="player-anm-per-record-layout"></a>[Player ANM per-record layout](re-settled-threads/animation.md#player-anm-per-record-layout) |

## Audio

Threads: [`re-settled-threads/audio.md`](re-settled-threads/audio.md).

| Detailed write-up |
|---|
| <a id="op-0x35-sub-op-0xa-is-the-track-swap-commit"></a>[Op-`0x35` sub-op `0xA` is the track-swap commit](re-settled-threads/audio.md#op-0x35-sub-op-0xa-is-the-track-swap-commit) |
| <a id="hyper-arts-fanfare-selector"></a>[Hyper Arts fanfare selector](re-settled-threads/audio.md#hyper-arts-fanfare-selector) |
| <a id="xa-clip-table-writer--clip_id-chan-cue-census"></a>[XA clip-table writer + `(clip_id, chan)` cue census](re-settled-threads/audio.md#xa-clip-table-writer--clip_id-chan-cue-census) |
| <a id="sfx-cue-bank-routing---the-category-byte-selects-the-vab-slot"></a>[SFX cue bank routing - the category byte selects the VAB slot](re-settled-threads/audio.md#sfx-cue-bank-routing---the-category-byte-selects-the-vab-slot) |
| <a id="which-prot-entries-fill-sfx-vab-slots-1--3--6--11"></a>[Which PROT entries fill SFX VAB slots 1 / 3 / 6 / 11](re-settled-threads/audio.md#which-prot-entries-fill-sfx-vab-slots-1--3--6--11) |
| <a id="the-fun_8006ef18-trio-is-a-bios-kernel-patch-sequence-not-an-spu-init"></a>[The `FUN_8006EF18` trio is a BIOS kernel-patch sequence, not an SPU init](re-settled-threads/audio.md#the-fun_8006ef18-trio-is-a-bios-kernel-patch-sequence-not-an-spu-init) |
| <a id="_dat_8007b910-is-the-live-audio-level-not-screen-brightness"></a>[`_DAT_8007B910` is the live audio level, not screen brightness](re-settled-threads/audio.md#_dat_8007b910-is-the-live-audio-level-not-screen-brightness) |
| <a id="key-on-pitch-unity-on-centre"></a>[Key-on pitch: unity on centre](re-settled-threads/audio.md#key-on-pitch-unity-on-centre) |
| <a id="fun_80018db0-is-a-rumble-cadence-not-an-audio-one"></a>[`FUN_80018DB0` is a rumble cadence, not an audio one](re-settled-threads/audio.md#fun_80018db0-is-a-rumble-cadence-not-an-audio-one) |
| <a id="xa-channel-map--str-demux-sm"></a>[XA channel map / STR demux SM](re-settled-threads/audio.md#xa-channel-map--str-demux-sm) |
| <a id="spu-reverb-live-routing-c7-reverb"></a>[SPU reverb live routing (C7-REVERB)](re-settled-threads/audio.md#spu-reverb-live-routing-c7-reverb) |
| <a id="bsedat-record-columns-and-the-gp0x678-consumers"></a>[bse.dat record columns and the gp+0x678 consumers](re-settled-threads/audio.md#bsedat-record-columns-and-the-gp0x678-consumers) |
| <a id="no-second-bse-dat-record-family"></a>[No second bse dat record family](re-settled-threads/audio.md#no-second-bse-dat-record-family) |

## Title / boot / overlays

Threads: [`re-settled-threads/title-boot-overlays.md`](re-settled-threads/title-boot-overlays.md).

| Detailed write-up |
|---|
| <a id="a-cold-boot-always-shows-title-sub-mode-0x10"></a>[A cold boot always shows title sub-mode `0x10`](re-settled-threads/title-boot-overlays.md#a-cold-boot-always-shows-title-sub-mode-0x10) |
| <a id="_dat_8007b98f-is-byte-3-of-the-debug-mode-word-_dat_8007b98c"></a>[`_DAT_8007B98F` is byte +3 of the debug-mode word `_DAT_8007B98C`](re-settled-threads/title-boot-overlays.md#_dat_8007b98f-is-byte-3-of-the-debug-mode-word-_dat_8007b98c) |
| <a id="new-game-opening-chain--narration-roller"></a>[New-Game opening chain + narration roller](re-settled-threads/title-boot-overlays.md#new-game-opening-chain--narration-roller) |
| <a id="overlay-loader-index-off-by-2---remaining-ripple"></a>[Overlay-loader index off-by-2 - remaining ripple](re-settled-threads/title-boot-overlays.md#overlay-loader-index-off-by-2---remaining-ripple) |
| <a id="muscle-dome-match-shape-an-ordinary-battle-ladder-not-a-card-battle"></a>[Muscle Dome match shape: an ordinary battle ladder, not a card battle](re-settled-threads/title-boot-overlays.md#muscle-dome-match-shape-an-ordinary-battle-ladder-not-a-card-battle) |
| <a id="the-dome-runs-two-state-machines-the-outer-one-is-the-contest"></a>[The dome runs two state machines; the outer one is the contest](re-settled-threads/title-boot-overlays.md#the-dome-runs-two-state-machines-the-outer-one-is-the-contest) |
| <a id="battle-arts-input-ui-decomposition-dome--standard-battle-input"></a>[Battle arts-input UI decomposition (dome = standard battle input)](re-settled-threads/title-boot-overlays.md#battle-arts-input-ui-decomposition-dome--standard-battle-input) |
| <a id="slot-b-overlay-cluster-09000969-per-entry-identity"></a>[Slot-B overlay cluster (`0900..0969`) per-entry identity](re-settled-threads/title-boot-overlays.md#slot-b-overlay-cluster-09000969-per-entry-identity) |
| <a id="0x80010390-is-the-slot-b-overlay-destination-pointer"></a>[`0x80010390` is the slot-B overlay destination pointer](re-settled-threads/title-boot-overlays.md#0x80010390-is-the-slot-b-overlay-destination-pointer) |
| <a id="prot-0968---the-cort-battle-stage-overlay"></a>[PROT 0968 - the Cort battle stage overlay](re-settled-threads/title-boot-overlays.md#prot-0968---the-cort-battle-stage-overlay) |
| <a id="prot-0977--0978-extraction--the-dump-re-key"></a>[PROT 0977 / 0978 extraction + the dump re-key](re-settled-threads/title-boot-overlays.md#prot-0977--0978-extraction--the-dump-re-key) |
| <a id="slot-b-capture-module-band-09350966-per-entry-identity"></a>[Slot-B capture-module band `0935..0966` per-entry identity](re-settled-threads/title-boot-overlays.md#slot-b-capture-module-band-09350966-per-entry-identity) |
| <a id="new-game-world-state-seed-store-widths"></a>[New-game world-state seed store widths](re-settled-threads/title-boot-overlays.md#new-game-world-state-seed-store-widths) |
| <a id="_dat_8007b8c2-polarity-and-its-writer"></a>[`_DAT_8007B8C2` polarity, and its writer](re-settled-threads/title-boot-overlays.md#_dat_8007b8c2-polarity-and-its-writer) |
| <a id="key-item-area-consumers"></a>[Key-item area consumers](re-settled-threads/title-boot-overlays.md#key-item-area-consumers) |
| <a id="titlepak-prot-entry"></a>[`title.pak` PROT entry](re-settled-threads/title-boot-overlays.md#titlepak-prot-entry) |
| <a id="title-screen-mode-table-prot"></a>[Title screen mode-table PROT](re-settled-threads/title-boot-overlays.md#title-screen-mode-table-prot) |
| <a id="xp-table-source--reader"></a>[XP-table source + reader](re-settled-threads/title-boot-overlays.md#xp-table-source--reader) |
| <a id="overlay-identity-from-the-disc-static-extraction"></a>[Overlay identity from the disc (static extraction)](re-settled-threads/title-boot-overlays.md#overlay-identity-from-the-disc-static-extraction) |
| <a id="prot-0896-bat_back_dat-identity"></a>[PROT 0896 (`bat_back_dat`) identity](re-settled-threads/title-boot-overlays.md#prot-0896-bat_back_dat-identity) |
| <a id="scus-recomp-gap---rendergte--bootinit-clusters"></a>[SCUS recomp gap - render/GTE + boot/init clusters](re-settled-threads/title-boot-overlays.md#scus-recomp-gap---rendergte--bootinit-clusters) |
| <a id="full-window-item-add-oob-reachability"></a>[Full-window item-add OOB reachability](re-settled-threads/title-boot-overlays.md#full-window-item-add-oob-reachability) |
| <a id="phantom-va-sweep-of-the-prot-0897-imports"></a>[Phantom-VA sweep of the PROT 0897 imports](re-settled-threads/title-boot-overlays.md#phantom-va-sweep-of-the-prot-0897-imports) |
| <a id="the-publisher-logo-quads"></a>[The publisher-logo quads](re-settled-threads/title-boot-overlays.md#the-publisher-logo-quads) |
| <a id="the-title-menus-law"></a>[The title menu's law](re-settled-threads/title-boot-overlays.md#the-title-menus-law) |
| <a id="fun_801e5a08-the-per-slot-equip-applier"></a>[`FUN_801E5A08`, the per-slot equip applier](re-settled-threads/title-boot-overlays.md#fun_801e5a08-the-per-slot-equip-applier) |
| <a id="the-dead-return-view-mode"></a>[The dead Return view mode](re-settled-threads/title-boot-overlays.md#the-dead-return-view-mode) |

## Rendering / camera

Threads: [`re-settled-threads/rendering-camera.md`](re-settled-threads/rendering-camera.md).

| Detailed write-up |
|---|
| <a id="does-retail-stack-coincident-curved-shells"></a>[Does retail stack coincident curved shells?](re-settled-threads/rendering-camera.md#does-retail-stack-coincident-curved-shells) |
| <a id="does-any-retail-shot-author-a-non-zero-camera-roll"></a>[Does any retail shot author a non-zero camera roll?](re-settled-threads/rendering-camera.md#does-any-retail-shot-author-a-non-zero-camera-roll) |

## Measurement + corpus

Threads: [`re-settled-threads/measurement-corpus.md`](re-settled-threads/measurement-corpus.md).

| Detailed write-up |
|---|
| <a id="which-live-ports-cover-only-part-of-their-routine"></a>[Which live ports cover only part of their routine](re-settled-threads/measurement-corpus.md#which-live-ports-cover-only-part-of-their-routine) |
| <a id="what-is-in-the-scus_94254-code-gap"></a>[What is in the `SCUS_942.54` code gap](re-settled-threads/measurement-corpus.md#what-is-in-the-scus_94254-code-gap) |

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts, and the page to move a row back to if new evidence reopens it.
- [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - the falsified hypotheses.
- [`docs/reference/functions.md`](functions.md) - canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the grading rubric behind the `decompiled-C` column.
- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) - per-function dumped x documented x ported x ignored axes; the function-level companion to this page's question-level index.
