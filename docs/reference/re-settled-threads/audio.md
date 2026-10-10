# Settled threads: Audio

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers how the game makes sound: the BGM sequencer and its track-swap
protocol, which sound bank (VAB) a sound-effect cue plays from, the SPU pitch and
reverb laws, the XA voice / movie audio streams, and the `bse.dat` battle cue bank.
A reader porting or debugging audio wants these because a wrong bank, pitch or
routing still plays *something* - the answers here say what retail plays.
The owning pages are [`audio.md`](../../subsystems/audio.md),
[`sfx-table.md`](../../formats/sfx-table.md) and [`seq.md`](../../formats/seq.md).

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [Op-`0x35` sub-op `0xA` is the track-swap commit](#op-0x35-sub-op-0xa-is-the-track-swap-commit)
- [Hyper Arts fanfare selector](#hyper-arts-fanfare-selector)
- [XA clip-table writer + `(clip_id, chan)` cue census](#xa-clip-table-writer--clip_id-chan-cue-census)
- [SFX cue bank routing - the category byte selects the VAB slot](#sfx-cue-bank-routing---the-category-byte-selects-the-vab-slot)
- [Which PROT entries fill SFX VAB slots 1 / 3 / 6 / 11](#which-prot-entries-fill-sfx-vab-slots-1--3--6--11)
- [The `FUN_8006EF18` trio is a BIOS kernel-patch sequence, not an SPU init](#the-fun_8006ef18-trio-is-a-bios-kernel-patch-sequence-not-an-spu-init)
- [`_DAT_8007B910` is the live audio level, not screen brightness](#_dat_8007b910-is-the-live-audio-level-not-screen-brightness)
- [Key-on pitch: unity on centre](#key-on-pitch-unity-on-centre)
- [`FUN_80018DB0` is a rumble cadence, not an audio one](#fun_80018db0-is-a-rumble-cadence-not-an-audio-one)
- [XA channel map / STR demux SM](#xa-channel-map--str-demux-sm)
- [SPU reverb live routing (C7-REVERB)](#spu-reverb-live-routing-c7-reverb)
- [bse.dat record columns and the gp+0x678 consumers](#bsedat-record-columns-and-the-gp0x678-consumers)
- [No second bse dat record family](#no-second-bse-dat-record-family)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Does a BGM track change close the side-band (3) or reward (11) VAB slot? | resolved (no) | `disassembly` | `FUN_800243F0` loads each track into slot 1 (`0x80024678` / `0x80024780`) and never calls the closer `FUN_8001FF58`. Slot 3 is closed by the battle mode init; slot 11 by the field init `FUN_801D6704` (`0x801D68A4..0x801D68B8` closes 7, 8 and 11 through delay-slot `a0`). Port `legaia_engine_audio::bgm_tail`, shared by both directors. |
| Does retail age SFX cues under the pause menu or a shop? | resolved (yes) | `disassembly` | Mode `0x17`'s per-frame handler `FUN_80025F74` calls the drainer `FUN_80016B6C` at `0x80025F9C`, gated only on `FUN_8001698C` and `FUN_80017978` returning 0. Both hosts now step the scheduler there. |
| Is a field track the script left running audible under a mid-game movie's XA, `town0d` and `jouine` included? | resolved (yes, where the script left it sounding) | `capture` + `disassembly` | The movie path makes no BGM-slot or `SsSeq*` call. Poked triggers keep a sounding voice in 81 / 83 of 84 samples (`town01`, `chitei2`); a stopped score stays silent. `town0d`'s `P2[31]` and `jouine`'s `P2[16]` are uncaptured, but both end on a sub-op `0xA` commit attaching a fresh track (`2034`, `2069`) before `fmv_id 6` / `8`, the `town01` shape; sub-op `5` fades and stops the slot's occupant, in a `9 · 5 · 0xA` window the outgoing track ([`audio.md`](../../subsystems/audio.md#movies-and-the-score)). |
| What does BGM sub-op 7 do, and sub-op 5? | resolved (7 stores the next scene's entry level ramp; 5 is a timed release) | `disassembly` | Sub-op 7 only stores `_DAT_8007B880` (-1 = none), which the field per-scene initializer reads at `0x801D6B48` for its entry level ramp. Sub-op 5 is `FUN_800267A8(0, n)`: it arms the auto-release cells `gp+0x808..+0x81C` (target 0, duration `n`) and ramps the slot's sequence through `FUN_80062004`. Whether the result is heard as a fade or a cut is not captured. |
| What does a movie do to the BGM? | resolved (nothing; only the title attract acts) | `disassembly` | None of the movie entry `FUN_80025FB4`, the dispatch `FUN_801CEA3C`, the play loop `FUN_801CF098` or the mode-change pass `FUN_80016230` makes a BGM-slot or `SsSeq*` call, and the sequencer is clocked by the root-counter callback (the one reference to `FUN_80062F98` is the data word at `0x8007A910`), so a movie inherits the score the script left. The title attract releases the slot (`0x801DDD7C` / `0x801DDD84` in PROT 0899), and CARD INIT `FUN_8002574C` restreams the title theme and re-attaches it from its first beat (`0x80025948`). Whether the inherited score is audible under the XA is open. |
| What do BGM sub-ops 2, 3 and 4 do to the sequence? | resolved (stop+rewind, key-off pause, replay from the top) | `disassembly` | Sub-op 2 is `FUN_800266E0` -> `FUN_80064370` -> `FUN_800641EC`: notes killed, cursor reset to `+0x4`, flag `0x4`. Sub-op 3 is `FUN_80026740` -> `FUN_8006282C` -> `FUN_8006275C`: flag `0x2` raised, cursor kept, notes keyed off by the next tick. Sub-op 4 is `FUN_80026478` -> `FUN_80062880(id,1,1)` -> `FUN_800628F0`, which resets the cursor to `+0x4` for every mode (`0x80062934..0x80062948`) before playing. So no script word resumes a track mid-phrase; the `FUN_800628F0` mode-0 pause is real but no BGM script word reaches it. |
| What does a scene-local BGM id (below 2000) load? | resolved (global slot 2, never a scene bank) | `disassembly` + `capture` | `FUN_800243F0` stores `*(0x80084540) + 6 + id` only as its change-test index (`0x80024458`); the load arm replaces the index it loads with `*(0x8007BC64) + 2` (`0x800245A4..0x800245BC`) and parks the id in `gp+0x728`, read only by the `WARNING BGM NO %d` print. `0x8007BC64` is `990` in 12 of 12 states, so it loads extraction 990 - the track id `2002` plays. Retail stages no scene bank. Engine `SCENE_LOCAL_BGM_FALLBACK_ID` ([audio.md](../../subsystems/audio.md#a-scene-local-id-loads-a-fallback-track-not-a-scene-bank)). |
| Where does the credits bank land in SPU RAM? | resolved (VAB 10 at slot 0's base, over the SFX banks) | `disassembly` | `FUN_800265E8` seeds `0x800917B0[10] = 0x1010`, the same base as slot 0 (`sw v1,0x28(v0)` at `0x80026668`), and `FUN_8002630C` with `a3 = 0` opens there; the bodies span `0x1010..0x641C0`, over slots 0 to 3 and short of slot 4 / 7 at `0x65010`. The latch `0x8007B9B8` is cleared only by the debug overlay, so retail never re-stages the SFX banks in a session. Pin `credits_bank_spu_layout_disc` ([audio.md](../../subsystems/audio.md#where-the-credits-bank-lands-in-spu-ram)). |
| How does a Muscle Dome round enter battle mode? | resolved (the arena stores mode word `0x14`) | `disassembly` | PROT 0977 stores `0x14` into `0x8007B83C` at `0x801D15B8` (`sh v0,-0x47c4(v1)`), so a round takes `FUN_8001DCF8`'s mode-`0x14` arm like any battle. A round's slot residency itself is not captured (open row in [`open-rev-eng-threads.md`](../open-rev-eng-threads.md)). |
| When does battle mode init close the field bank? | resolved (only when the mode word reads `0x14`) | `disassembly` + `capture` | `FUN_8001DCF8`'s close-slots-6-and-3 and latch-clear arm is gated `lh 0x8007B83C` / `bne 0x14` at `0x8001DF74..0x8001DF80`. A battle transition reaches it; a minigame overlay's call with the mode word `0x18` skips it, so after the Baka Fighter's warp slots 2 and 6 are both enabled over one region - slot 6's header PROT 0876's, the samples PROT 0869's ([`audio.md`](../../subsystems/audio.md#retail-capture-of-the-slot-2--slot-6-residency)). |
| Which field scripts push SFX cues, and how are the ids keyed? | resolved (op `0x36` sub `0` / `4` and motion op `0x09`, ids straight from the bytecode) | `disassembly` | `jal 0x80035B50` at `0x801E0348` and `jal 0x80035BAC` at `0x801E03D8` (PROT 0897), `a0 = (s16)word1`; motion op `0x09` at `0x80039178`. Ids below `0x200` key the static table, the rest the scene prescript's record 0; across every scene MAN only `balden2`'s `0x20B` falls one row past its bank ([`sfx-table.md`](../../formats/sfx-table.md#the-fields-producers-op-0x36-and-the-motion-vms-op-0x09)). |
| Which bank does an op-`0x36` sub-`1` request stream? | resolved (a `vab_01` bank, by four overwriting arms) | `disassembly` + `capture` | `FUN_800243F0` at `0x800248B4..0x8002494C`: `< 2000` -> `vab_01 + 2`, `2000..=2999` -> `+ id - 2000` in slot 3, `>= 3000` -> `+ id - 3000` in slot 6, `0x1000` parks. `*(0x8007BBE4)` reads `1072` in the catalogued states ([`sfx-table.md`](../../formats/sfx-table.md#the-side-band-bank-a-field-script-selects)). |
| Who writes the frame-step floor `DAT_8007B9D8`? | resolved (each mode's init, and the scene through move-VM ext `0x2F`) | `disassembly` + `capture` | Field init writes `2` (`0x801D6990`); jump-table slot `0x2F` of `0x801CE868` stores the halfword operand (`0x801D45E4`), and five scene prescripts carry it - `opdeene`'s record 16 opens with operand `3`, which the cold-boot capture holds. Name entry `FUN_801F03F0` holds `1` while open ([`actor-vm.md`](../../subsystems/actor-vm.md)). |
| How do the field bank and the class-2 bank share SPU memory? | resolved (one region, refilled per mode, behind a latch) | `disassembly` | Slots 2 and 6 share a base. The field init loads PROT 0876 into slot 6 only while `0x8007BAFC` is clear and closes 2 / 7 / 8 / 11; battle init closes 6 and 3 and clears it (`0x8001DFC0`), as do the minigame warp (`0x800259A4`, `gp+0x7E4`) and op `0x36` sub `3` ([`sfx-table.md`](../../formats/sfx-table.md#one-region-per-mode-slot-2-and-slot-6)). |
| What does a cue whose bank is closed play? | resolved (nothing) | `disassembly` | The drainer `FUN_80016B6C` skips a cue whose mixer record's `+0xB` byte is zero (`lb v0,0xb(v1)` / `beqz` at `0x80016CE4..0x80016CEC`), and the VAB close `FUN_8001FF58` zeroes that byte in the delay slot of its `jal 0x80068C80`. So a category-6 cue in battle is silent, and `FUN_80035BD0(0)` is a cancel in the field. |
| What is `FUN_80035B50`? | resolved (a round-robin ring of four, not a delay queue) | `disassembly` | It writes the cue and a zero timer into slot `gp+0x158`, latches that slot into `gp+0x15A` and advances the cursor modulo four with no free-slot search; `FUN_80035BAC` sets the latched slot's countdown and `FUN_80035BD0` overwrites its cue. A fifth pending cue replaces one ([`sfx-table.md`](../../formats/sfx-table.md)). |
| Why does the port sustain more sounding voices than retail on the same track? | resolved (two instrument defects, no engine change owed) | `capture` | The pairing was never aligned: frame 0 of an engine trace is tick 0 of a track while a capture sits wherever play parked it, and the `s3_rimelm_freeroam` window aligns at engine frame `3111` of `3601`. Aligned, the sides carry the same ten packed ADSR words, the same per-tone key-on count on nine of ten tones, and `71` engine key-ons against `67` retail. The rest is the envelope channel, which a per-vsync save-state capture cannot measure at all - PCSX-Redux steps ADSR on an audio-paced thread. The surviving comparand is the key-on **rate**. See [`audio.md`](../../subsystems/audio.md#align-the-windows-before-comparing-them). |
| Which voice clip does a Seru cast play? | resolved (the **module** names it, not the caster) | `disassembly` | Each slot-B cast image hardcodes a literal cue id near its head - 62 of the 64, with PROT 0936 and 0937 forming theirs at run time - so the mapping is per module. Resolved through the cue dispatcher the ids land on seventeen streamed-audio files: `XA7`, `XA9..15`, `XA18..20`, `XA22`, `XA23`, `XA25` and `XA34`. Three of the literal ids sit **below** the `0x100` streamed threshold and are ordinary ring cues instead. |
| What bounds the cue dispatcher's streamed-voice leg? | resolved (two decline gates, and **nothing** bounds the table index) | `disassembly` | `FUN_8004FCC8` tests `sltiu v0,s0,0x100` at `0x8004FCD4` and nothing else: above the threshold it declines on the context byte `*(gp+0xA0C)+0x276` being non-zero and on the drive-idle poll `FUN_8003DE7C(1)` returning non-zero, then indexes `DAT_800788B8` at `id - 0x100` with no upper bound (`lhu` at `0x8004FD44`). The clip slot is `(id - 0x100) >> 3` with slots 1 / 3 / 5 remapped to `0x1A` / `0x1B` / `0x1C`. Measured off the executable the table is `0x110` entries with live rows as high as `0x10F` and several interior zero runs (`0x37..0x40`, `0x78..0x88`, `0xE8..0x10A`); a reader that stops at `0x40` drops every cast cue. |
| Which tracks do the in-world minigames start? | resolved (through the **piecewise** extraction map) | `disassembly` | The loaders name PROT entries `1043` / `1048` / `1054`, which the `music_01` bank's piecewise map sends to global BGM ids `2055` / `2060` / `2066`. Subtracting a single `990 + slot` base gives `2053` / `2058` / `2064` - three tracks off, and the gap at `1056`/`1057` is why one base cannot work. See [`music-tracks.md`](../music-tracks.md). |
| What does the sequencer's pause do to sounding notes? | resolved (it is a **key-off**, not a freeze) | `disassembly` | `FUN_800628F0` mode `0` falls through both compares (`bne a2,v0(=1)` at `0x80062988`, `bne a2,zero` at `0x800629D8`) to `ori v0,v0,0x2` / `sw v0,0x98(v1)`, raising slot flag `0x2`. The per-tick `FUN_80062F98` tests `andi v0,v0,0x2` and calls `FUN_800638D8`, which calls `FUN_800684CC` to release every sounding voice whose owner halfword matches the `(sequence, channel)` key, stores `0` at the channel's `+0x14`, and clears the flag with `li v1,-0x3` / `and`. The play cursor is untouched, so resume continues where it stopped - the notes do not. |
| What drives the CD / XA transport? | resolved | `disassembly` | `FUN_8003D764` is an 11-state machine over the transport word at `0x800111C4`. The port models it as `xa_transport`, tagged `REF:` rather than `PORT:` - the states are the shape the engine's own streaming follows, not a routine a host calls. |
| SPU reverb live routing (C7-REVERB) | resolved (wired; Studio C, global) | `capture` | [details ↓](#spu-reverb-live-routing-c7-reverb) |
| XA channel map / STR demux SM | resolved (static decompile of PROT 0970 + SCUS) | `disassembly` | [details ↓](#xa-channel-map--str-demux-sm) |
| `FUN_80018DB0` is a rumble cadence, not an audio one | resolved (libpad, not SsAPI; no cue to pin) | `disassembly` | [details ↓](#fun_80018db0-is-a-rumble-cadence-not-an-audio-one) |
| Key-on pitch: what does retail put in the voice pitch register? | resolved (unity on centre; the port was an octave low) | `disassembly` | [details ↓](#key-on-pitch-unity-on-centre) |
| SFX cue bank routing - the category byte selects the VAB slot | resolved (mechanism + the two pinned banks; ported) | `capture` | [details ↓](#sfx-cue-bank-routing---the-category-byte-selects-the-vab-slot) |
| Which PROT entries fill SFX VAB slots 1 / 3 / 6 / 11 | resolved (slot 6 = 0876, slot 11 = 0889; 1 / 3 are variable banks) | `disassembly` | [details ↓](#which-prot-entries-fill-sfx-vab-slots-1--3--6--11) |
| The `FUN_8006EF18` trio is a BIOS kernel-patch sequence, not an SPU init | resolved-negative | `disassembly` | [details ↓](#the-fun_8006ef18-trio-is-a-bios-kernel-patch-sequence-not-an-spu-init) |
| `_DAT_8007B910` is the live audio level, not screen brightness | resolved (both hosts' labels corrected) | `disassembly` | [details ↓](#_dat_8007b910-is-the-live-audio-level-not-screen-brightness) |
| XA clip-table writer + `(clip_id, chan)` cue census | resolved (writer pinned statically; census in `audio.md`) | `disassembly` | [details ↓](#xa-clip-table-writer--clip_id-chan-cue-census) |
| Hyper Arts fanfare selector - what audio fires when a Hyper executes | resolved (per-(char, art) coin flip over a fixed channel pair of the even-slot fanfare bank) | `disassembly` | [details ↓](#hyper-arts-fanfare-selector) |
| Op-`0x35` sub-op `0xA` - what the "unhalt-pause toggle" waits on | resolved (it is the track-swap commit; both globals pinned; ported) | `disassembly` | [details ↓](#op-0x35-sub-op-0xa-is-the-track-swap-commit) |
| What do `bse.dat`'s record columns mean? | resolved (the static SFX-table columns; `+4` is a `u8` category) | `disassembly` | [details ↓](#bsedat-record-columns-and-the-gp0x678-consumers) |
| Who consumes the `bse.dat` record-table pointer `gp+0x678` (`0x8007B990`)? | resolved (cue router `FUN_8004FE5C` + the overlay-0971 sound test, via `lui`+`lw` the word scan cannot see) | `disassembly` | [details ↓](#bsedat-record-columns-and-the-gp0x678-consumers) |
| When does `bse.dat` load? | resolved (battle-scene setup, not boot) | `disassembly` | `FUN_8001FA88`'s only caller on the disc is `0x80051A3C` in battle init `FUN_800513F0`, reached from the battle tick `FUN_80046A20` at `0x80046F74` under `ctx[+0x11] == 0`. |
| `bse.dat` 888 vs its 1195 sibling | resolved (one format, two occupants of the `>= 0x200` bank role) | `disassembly` | 888 is the battle occupant; a scene prescript's record 0 fills the same slot in the field (`FUN_8001F7C0` at `0x8001F864` repoints it). 1195 is such a prescript and the detector should keep matching it. |
| Is there a second `bse.dat` record family past the table (0888 `0x94C`, 1062 `0x51AD`)? | resolved (no - a neighbouring file's PsyQ `VagAtr` tone rows left in the sector) | `capture` | [details ↓](#no-second-bse-dat-record-family) |
| What is `_DAT_8007BA9C`? | resolved (the BGM-swap **force-reload** latch) | `disassembly` | Read at `0x8002457C` and XOR-paired with `_DAT_8007BAB8`; when the two disagree the next track request re-reads its bank instead of reusing the resident one. It is the barrier op-`0x35` sub-op `9` waits on before it makes sub-op 1's own `_DAT_8007BAC8` store. |
| What is `FUN_80065034`? | resolved (`SsUtKeyOnV`) | `disassembly` | Its arguments are `(voice, vabid, prog, tone, note, fine, voll, volr)` - the **second** is the VAB id, which is what makes the cue's bank explicit rather than implied by the resident bank. The `CUE_LEVEL` name an overlay port gave the argument ("channel mixer level") describes no parameter of it. |
| How is a retail VAB carried in its DATA_FIELD stream? | resolved (**two** chunks, and the bodies are the second) | `disassembly` + `capture` | The 4-byte word in front of every `pBAV` is a chunk header whose payload is the bank's **header part** only - `0x20 + 0x800 + 0x200 * ps + 0x200` - which holds in all 424 retail banks. The VAG bodies are a separate chunk of the same stream, and that chunk's own header is the "+4 skew" the decoder had recorded as a property of the format. Six entries put the SEQ chunk **before** the body chunk, so a fixed `{0, 4}` probe cannot recover their origin; `legaia_vab::vag_body_origin` walks the stream for it. |
| What is the resident `monster.snd` index? | resolved | `disassembly` | `[u32 reserved][u32 count][u32 start_sector[count + 1]]` at `0x801C8980`, staged by PROT 0895's `memcpy` of `0x400` bytes at `0x801CEF74`. The reader `FUN_8003E104` bounds its argument against the count word, streams sectors `[table[i], table[i + 1])` relative to raw TOC entry `0x37D`'s start LBA, and forks on the build-mode flag for the dev filename path. The trailing entry is the archive's own sector length rather than a bank, which is what makes the span arithmetic total. |
| A VAB's VAG-body origin - is the `+4` a format property? | resolved (two wrongs that cancel on all but six carriers) | `disassembly` + `capture` | The parse reports a body origin four bytes early and the upload re-slices by the same four, so the pair cancels on 212 of 218 carriers and the skew read as a property of the format. Six entries break the cancellation - `0886`, `1058`, `1059`, `1063`, `1064`, `1065` - because their SEQ chunk precedes the body chunk, so the four bytes uploaded as sample data are SEQ bytes; the legal-filter share on those rises from 0.53-0.84 to 1.00 once the origin is resolved off the stream. `legaia_vab::vag_body_origin_at` + `parse_in_stream` answer it; [`vab.md`](../../formats/vab.md). |
| What is in PROT 1062? | resolved (a single-chunk DATA_FIELD stream carrying a SEQ) | `disassembly` | One chunk, type byte `0x02`, payload `pQES`. It is the shape that made the stream walker's coverage of that entry read as partial rather than the entry being unparsed. |
| Where does a scene's entry BGM come from? | resolved (the scene's own script, not the loader) | `disassembly` | The BGM request word `_DAT_8007BAC8` has four `sw` sites disc-wide and all four are in PROT 0897: `0x801E012C` (field-VM op `0x35` sub-1), `0x801E0254` (sub-9), `0x801EAD1C` (the dev menu's `BGM CALL` row) and `0x801D6844` - a conditional `sw $zero` inside the per-scene field initializer `FUN_801D6704`. So the loader **clears** the id and the script installs it, and a port expecting a scene load to select the track has nothing to read. The sweep has to be `gp`-relative: 14 hits across the disc, none of them visible to an absolute-address scan. |
| What is the dev menu's `BGM CALL` arm? | resolved (it plays a track; cycling is a different routine) | `disassembly` | The arm at `0x801EACBC..0x801EAD20` inside `FUN_801EA9B0` indexes the sound-test table at `0x801F2E94` by the cursor `_DAT_801F2E90` - a 10-byte stride of `[i16 global bgm id][8-byte ASCII label]`, 71 rows plus an `OFF` row whose id reads `-1` and which raises `_DAT_8007B438` instead. The row index is **not** the id: the rows carry `2000..=2043` then `2045..=2071`, so cursor and id part company above the missing `2044`. The input half - stepping the cursor - is `FUN_801E9F64`. [details](../../subsystems/world-map.md#fun_801ea9b0---dev-menu-row-action-dispatcher-1000-bytes) |
| What makes a captured SPU voice audible? | resolved (the envelope **level**, not the phase word) | `capture` | Mednafen's `ADSR.Phase` has no `Off` member: it runs `0` Attack through `3` Release, and a key-off parks a voice in Release forever, so a phase test reports every voice a state ever keyed as live. The audibility predicate is `ADSR.EnvLevel != 0` (the PCSX-Redux analogue is `ADSRInfoEx.EnvelopeVol`). Over the 98-state retail corpus, 1624 of 2352 voice slots sit at level zero, and reading the level gives a median of 7.5 audible voices per state - the same order as the engine's 4-8. Everything a phase-keyed comparison concluded about retail's voice counts was the predicate talking. |
| What is BGM id `0x1000`? | resolved (a **park sentinel**, shared by both streaming slots) | `disassembly` + `capture` | `4096` is outside the `2000..=2077` pool and the resolver never tries: at `0x8002454C` it compares `_DAT_8007BAC8` against `li v0, 0x1000` and falls into `0x80024560`, which copies the pending index `_DAT_8007BAB8` onto the loaded-index barrier `_DAT_8007BA9C`, so the equality test four instructions later skips the load. The block at `0x800244C0` applies the same test to the second slot (`_DAT_8007BABC` -> `_DAT_8007BAA0`). In the catalogued corpus only the ending states carry it. [`audio.md`](../../subsystems/audio.md#0x1000-is-a-park-sentinel-not-a-track) |
| Does a save state's *scene* decide which BGM it holds? | resolved (**no** - `0x8007BAC8` does) | `capture` + `disassembly` | Of six catalogued `town01` PCSX-Redux states only one holds `2000`; the rest hold `2016`, the id `town01`'s own prescript selects. The `2000` state walked into the town from the world map, and `0x8007BA9C` / `0x8007BC64` corroborate through the resolver's own `base + (id - 2000)`. Pairing a retail capture with an engine trace by scene rather than by this word compares two pieces of music. |
| What is retail's reverb depth and work area? | resolved (`vLOUT` = `vROUT` = `0x3264`, `mBASE` -> `0x79020`) | `capture` | The depth pair is what `SpuSetReverbDepth` writes and sits **outside** the 32-register preset block, so matching the Studio C coefficients says nothing about it; the work-area size `0x6FE0` is Studio C's own, a second confirmation of the preset. Per-voice `EON` reads `0x00FFFFFF` - all 24 voices - on every frame of a 90-frame per-vsync capture, where a `.mc` freeze shows only the 15-22 a given moment has keyed. The engine's placeholder `0x4000` is replaced by the measured value. |
| Is slots-per-note a difference between the port and retail? | resolved (**no** - it is a property of the arrangement) | `capture` | Sounding voices over distinct `(sample, pitch)` pairs reads retail `1.158` / port `1.002` on track `2000` and retail `1.002` / port `1.037`-`1.061` on track `2016` - the sign reverses with the track. A second retail capture taken in the same town on the same walk reports `1.002`, which also disposes of the "town SFX in the retail window" caveat. The sequencer's three note-drop paths fire **zero** times across the window. [`audio.md`](../../subsystems/audio.md#comparing-per-voice) |
| Does a Muscle Dome round load the class-2 bank? | resolved (yes - PROT 0869 into slot 2, as a field battle does) | `capture` | From the arena's hub into a round, `FUN_8001DCF8(0x0C)` (from `0x80055D64`) runs with the mode word at `0x14` and closes slots 6 and 3 (`0x8001DFB4` / `0x8001DFBC`); the battle loader (`ra 0x800523B4` / `0x8005241C`) stages raw `0x367` into slot 2, and the header at `0x8008D708` turns from PROT 0876's (`0x2C090`) into 0869's (`0x2FB00`). The run starts from the one library state whose resident SCUS carries only the starting-bag seed patch ([`audio.md`](../../subsystems/audio.md#retail-capture-of-the-slot-2--slot-6-residency)). |
| What do `FUN_8001E54C`'s chunk types install? | resolved (bank and score, no LZS) | `disassembly` + disc scan | Jump table `0x80010600`: `0` releases the SEQ slot and copies the header; `1` and `3` are the VAB open and body transfer (`FUN_8002630C`, `3` budget-split); `2` detaches and closes the SEQ player, copies the payload into the slot's staging buffer and opens it through `FUN_80026410` (`SsSeqOpen`); `0xC` opens the fixed record `0x800705AC`. All 90 disc entries with a `pQES` score past offset 0 carry it as a type-2 chunk, and no stream carries `0xC` ([`audio.md`](../../subsystems/audio.md#vab-slots---one-installer-twelve-records)). |
| Is every `music_01` slot a `[VAB][SEQ]` pair? | resolved (75 of 81) | `capture` (disc-gated walk) | The installer walk splits 75 of the 81 slots into a bank and a score; slot 72 is a score with no bank, 76..79 are one-sector fills and 80 is a bank with no score, and the old magic hunt agrees on all 81 (`engine-core/tests/seq_chunk_walk_disc.rs`, [`audio.md`](../../subsystems/audio.md#global-pool-bgm-the-music_01-bank)). |
| What does field-VM op `0x35` sub-5's expiry do? | resolved (a scheduled field-BGM pause) | `disassembly` | The expiry arm of `FUN_800267FC` (`0x80026828..0x8002686C`) is `FUN_800266E0`'s body inline - sub-op 2's pause on the field-BGM slot `0x8007052C`. The engine now emits a BGM sub-op 2 event at expiry ([`audio.md`](../../subsystems/audio.md)). |
| What does the field init's slot-10 load serve? | resolved (the credits theme `0x814`) | `disassembly` + `capture` | `FUN_801D6704`'s two-part arm (`0x801D71A0..0x801D72D0`) stages the score from raw `0x428` (extraction 1062, one SEQ chunk) and the instruments from raw `0x422` (extraction 1056, a 53-program VAB-only bank) into sound slot 10, starts the sequence with `FUN_80026478(0x800705BC)` and sets the latch `0x8007B9B8`; while it is set `FUN_800243F0` returns at once (`0x8002440C`). The ending states hold extraction 1062's SEQ chunk in slot 10's buffer byte for byte, and all sixteen programs the score keys are defined in bank 1056. Engine: `mode_entry_init::two_part_bgm_stream` ([`audio.md`](../../subsystems/audio.md)). |
| Is `FUN_80064090` (the sequencer's channel restart) reachable? | resolved (no) | `disassembly` | Its only reference is the `jal` at `0x80063C94`, behind `channel[+0x22] != 0xFF`, and the byte's only two writers in SCUS (`0x80061DAC`, `0x80064580`) both store `0xFF`. The port is `REPLACED-BY`. |
| What does field op `0x35` sub-op 8 (`FUN_80019898`) sound? | resolved (nothing, on every catalogued state) | `disassembly` + `capture` | It replays once the sequence bound to the second sound-source record `0x8007057C` through `FUN_80026478`; that record's id is whichever of 1 / 2 the field-BGM record `0x8007052C` does not hold, and across 98 mednafen states its channel is either unstreamed (97) or inactive (`+8 == 0`, one), so the call returns before replaying. The engine's default no-op is faithful. |

### Op-`0x35` sub-op `0xA` is the track-swap commit

*Status:* resolved; ported. Evidence: `disassembly` (a store-offset writer census over SCUS and every based overlay image, [`address-reference-scan.md`](../../tooling/address-reference-scan.md)).

Sub-op `0xA` (arm `0x801E0264`, field overlay 0897) is the **commit** half of the sub-op 9 / `0xA` swap handshake, not a toggle: wait until the incoming track is staged, release the slot's paused occupant, acknowledge with flag bit 4 (which unstalls the poller's install), clear pause bit 1.

- **Nothing writes `_DAT_8007B868`.** Its only store in the static corpus is a read-modify-write clearing bit 1 at `0x8001E008` in the boot mode-init `FUN_8001DCF8`; a raw byte sweep over all 1233 PROT entries adds one incidental data word (`0392_map03.BIN +0x2bc40`, surrounded by non-code). The word never goes non-zero in retail play. It is the dev / dual-mode gate the whole actor-sound family (`FUN_800266E0` / `80026520` / `26740` / `26478` / `26410`) checks, so the arm's early return mirrors its callees.
- **`_DAT_8007B750` bit 3 has one setter:** `ori v1,v0,0x8` at `0x800246D0` in `FUN_800243F0`, the BGM resolver / poller's load-settle stage. It is reached only while a track swap is in flight, after the settle countdown at `gp+0x768` (armed to `0x1E` frames at load start, `0x3C` when master mode is 2) hits zero. The poller then stalls its own install while bit 0 (sub-op 9's script-owned start) is up and bit 4 is not (`0x800246E0..E8`).
- **`FUN_80026520` closes what `FUN_800266E0` only detaches.** `800266E0` resets the pan state and rewinds / stops the bound sequence (`FUN_80064370`, the `SsSeqRewind` wrapper), leaving the source active. `80026520` VSyncs, clears the source's active flag (`+0x8`), rewinds and closes the handle (`FUN_80061E94`, the `SsSeqClose` shim). Together they are a full slot release; the poller's own teardown calls both.
- **Sub-ops 2 / 3 / 4:** sub-op 2's pause sets flag bit 1; sub-op 3 also sets it and calls the voice-stop `FUN_80026740`; sub-op 4 clears it and calls the re-attach `FUN_80026478`.

Port: `SceneHost::route_bgm_events` routes sub-op 10 to `BgmDirector::unhalt_pause` (release the source only while the pause latch is set, then clear the latch unconditionally), overridden by `AudioBgmDirector`, the one director both play hosts run; its starts also clear the pause gate, as retail's sub-op 1 arm does. Test: `crates/engine-core/tests/bgm_midscene_change_disc.rs` (town01's cutscene records carry the op).

Owning pages: [`audio.md`](../../subsystems/audio.md#the-track-swap-handshake-fun_800243f0--op-0x35-sub-op-0xa) (protocol + flag-word bit map), [`script-vm.md`](../../subsystems/script-vm.md#sub-op-0xa-is-the-swap-commit) (the arm). See `ghidra/scripts/funcs/800243f0.txt`, `800266e0.txt`, `80026520.txt`, `8001dcf8.txt`.

### Hyper Arts fanfare selector

*Status:* resolved - selector pinned in code and capture.

A Hyper art fires **no pool shout**: its action constant sits below the shout table's `lo` bound. Instead:

- The staged-animation materialiser `FUN_8004AD80`, on the Hyper class byte `0x1A` at `actor+0x1DA`, reads the queued art constant and fires the jingle queue (`FUN_8004FCC8` -> `FUN_8003D53C`) with `jingle_id = rand()%2*3 + base`.
- That is a per-(character, art) coin flip between the fixed channel pair `{base, base+3}` of the character's stereo fanfare bank (the even clip slots: Vahn `XA1.XA`, Noa `XA3.XA`, Gala `XA5.XA`). There is no avoid-repeat memory, unlike the shout pool.
- Super and Miracle expansions take a sibling branch to fixed ids `0x101` / `0x111` / `0x121`, the same bank's generic channel 1. A Miracle's finisher additionally fires its anim cue track (`FUN_800508DC`, ids `0xC8..0xFF` rebased `+0x38`).

All nine per-art Hyper rows are capture-witnessed off the `FUN_8003D53C` staging globals, and every witnessed duration reproduces the `0x800788B8` table arithmetic against the real SCUS. Unwitnessed: the second pair member for seven of nine arts is rule-derived, as are the Vahn / Noa Miracle finisher cue-track ids and `XA1` channel 0.

Engine table: `legaia_art::hyper_fanfare::CAPTURED_FANFARES`. Owning page: [`battle-action.md`](../../subsystems/battle-action.md).

### XA clip-table writer + `(clip_id, chan)` cue census

*Status:* resolved - writer pinned; cue census decoded.

- **Table.** `0x801C6ED8` holds 34 `[CdlLOC][len]` slots = `XA1..XA34`; a title capture matches the disc files byte for byte.
- **Writer.** `FUN_801CFA78` in PROT 0895 `init.pak` (base `0x801CE818`, recovered from four in-blob string refs) sprintf-generates `\XA\XA%d.XA;1` per slot and fills `[BCD-MSF][size]` through the ISO9660 lookup `FUN_8005DBB4`. It is called once from the init boot tick `0x801CF500`. The two `lui 0x801c` sites in SCUS (`FUN_8003D53C` / `FUN_8003EAE4`) are the **readers**; the writer is overlay-resident, so no absolute-form scan of SCUS sees it.
- **Cues** (caller census of `FUN_8003D53C` / `FUN_8003EAE4`): menu voice `FUN_8004FCC8`; the normal-move grunt (`XA30` chan 0 / 4 / 6, overlay `0x801EEB44`); the arts shout (`FUN_8004C140` -> `XA2` / `XA4` / `XA6` per character, per-art channel pool); SM state `0x6E` (`XA9` via `0x800787AF`); slot machine `XA1`.
- **Capture.** `scripts/recomp/xa_cue_capture.py` reads live-battle fires frame-tagged off the `FUN_8003D53C` staging globals and pins the live table variant and the packed second-half spans; witnessed picks are `legaia_art::arts_voice::CAPTURED_ART_CHANNELS`.
- **Census caveat.** A PROT-entry over-read aliases call sites into neighbouring overlays; dedupe by true entry extent (gameover 0902 and world-map 0901 have zero genuine XA calls).

Owning pages: [`audio.md`](../../subsystems/audio.md) (full one-shot + streamed cue census), [`battle-action.md`](../../subsystems/battle-action.md).

### SFX cue bank routing - the category byte selects the VAB slot

*Status:* resolved - a cue names its own bank, and both hosts stage two banks and route by category. Which PROT entry fills each slot is [the next entry](#which-prot-entries-fill-sfx-vab-slots-1--3--6--11).

- A descriptor's `+4` byte is a **category**. It selects the 12-byte mixer record at `0x80091508 + category*12`.
- That record's `+8` is a **VAB slot id**, not a level: `FUN_80065034` hands it to `FUN_80068b98`, which rejects it unless the per-bank open-state byte `_DAT_801CE368[id] == 1` and then repoints the current-bank globals at that slot before the program / tone lookup.
- Across the catalogued save states record `N` holds `+8 == N` and `+0` == slot `N`'s live `VabHdr`, in every record of every state: category `N` selects slot `N`.
- Slot 0 is **PROT 0868** (a live field state's 512-byte slot-0 `VagAtr` program-0 page occurs verbatim in that entry at VAB offset `+4`, `ps = 5` agreeing). Slot 2 is the class-2 bank **PROT 0869**, which the battle scene loader `FUN_800520F0` loads with `a1 = 2`.
- Histogram over the 100 descriptors: category `0`: 16, `2`: 53, `6`: 30, `11`: 1.

A port that fires every cue through one bank neither errors nor goes silent: both banks carry a one-VAG-per-semitone UI key map at program 0, so a category-`0` id resolves to a sibling sample (PROT 0869's is roughly twice as long and a fifth lower, because its `center` bytes are authored higher). Peak, duration and "did a voice key on" all pass; only the source entry of the samples separates the two, which is what the disc-gated oracles assert.

Port: `legaia_asset::sfx_table` carries the law (`slot_for_category`, `prot_index_for_slot`, `SLOT_BANKS`, `PINNED_SLOT_BANKS`). `engine-session`'s boot and the browser play page each stage the two resident banks out of one SPU allocator over their shared reserved region and resolve every cue through its own slot; categories `6` and `11` fall back to the class-2 bank.

Owning page: [`sfx-table.md`](../../formats/sfx-table.md#category-is-a-bank-selector-and-four-banks-are-open-at-once).

### Which PROT entries fill SFX VAB slots 1 / 3 / 6 / 11

*Status:* resolved - slot `6` is **PROT 0876** and slot `11` **PROT 0889**; slots `1` and `3` hold banks that are re-selected at runtime, so neither has a fixed entry. Evidence: `disassembly` for the bindings, with a `capture` byte-pin and a structural cross-check.

A bank reaches a slot through one pair of calls. `FUN_8001FC00(raw_toc_index, category, buf, append, len)` streams the entry in; it ignores its second argument. `FUN_8001E54C(category, buf, len)` installs it: it indexes the same 12-byte mixer record the descriptors do, takes the header buffer from `+0` and the VAB slot from `+8`, and opens the bank via `FUN_8002630C` -> `SsVabOpenHead` (sticky, at the SPU address the per-slot table at `0x800917B0` holds) -> `SsVabTransBody`. The binding is therefore `a0` at each call site of `FUN_8001E54C`:

| Slot | Filler | Call site |
|---|---|---|
| `0` | PROT 0868 | resident system bank |
| `1` | current BGM bank (`music_01`), variable | `FUN_800243F0`, `raw = *(0x8007BC64) + id - 2000` |
| `2` | PROT 0869 (raw `0x367`) / `0875` | `FUN_800520F0`, `FUN_801CF00C` |
| `3` | a `vab_01` side-band bank, variable | `FUN_800243F0`, `raw = *(0x8007BBE4) + id - 2000` from `_DAT_8007BABC` |
| `6` | PROT 0876 (raw `0x36E`) | field init `FUN_801D6704` |
| `7` / `8` | the two `monster.snd` banks | `FUN_8003E104` from `FUN_800520F0` |
| `11` | PROT 0889 (raw `0x37B`) | battle-end reward resolution `FUN_8004E568` |

Cross-checks on the two pins:

- PROT 0889 populates exactly one `ProgAtr` slot, number **10**, and the one category-`11` descriptor (`0x50`) names program 10 with 2 voices against that program's 2 tones. The function that loads it is the one that fires the cue.
- PROT 0876 holds **30** VAGs for the 30 category-`6` descriptors, and its populated programs `1..=7` cover 29 of the 30.
- A catalogued field state's live slot-6 and slot-1 header buffers match extraction 0876 and 0998 byte for byte - unique hits across all 218 VABs on the disc once the runtime-written `ProgAtr +8..0xF` words are excluded.

Two laws from the same read:

- `FUN_8001D424` writes `+8 = record index` for all 16 mixer records, so "category is the slot" is the initialiser's own statement.
- It assigns four pairs of records one shared header buffer, which `FUN_800265E8` matches with one shared SPU base. Slot 6 and slot 2 are **the same physical bank in two modes**, so retail needs no extra SPU room for the field cues, and a host that stages once at boot cannot simply add them. The open-state array `_DAT_801CE368` holds slots `0,1,3,6` in every field-family state and `0,1,2,7` in every battle state, never 2 and 6 together.

Owning page: [`sfx-table.md`](../../formats/sfx-table.md#which-prot-entry-reaches-which-slot) (map, budget arithmetic, port surface).

### The `FUN_8006EF18` trio is a BIOS kernel-patch sequence, not an SPU init

*Status:* resolved-negative - the trio touches no SPU register, voice block or libspu global. Evidence: `disassembly` (the veneer bodies and the patch payloads are both read out of the executable).

- `FUN_8006EF68` is a bare BIOS stub (`li t2,0xb0; jr t2; li t1,0x4c`) = B0 `0x4C` `StopCARD`. Its neighbours `8006EF48` / `8006EF58` are the same shape with `0x4A` `InitCARD` and `0x4B` `StartCARD`.
- `FUN_8006F088` calls `GetB0Table`, takes entry `0x5B` (`ChangeClearPAD`) as a version-stable anchor, and **swaps** five words between `+0x9C8` off it and the static block at `0x8006F058`. The shipped block is a `jalr` trampoline back to `0x8006F058`, so after the swap the kernel calls a buffer holding its own displaced instructions, falls through into a `0xC8`-iteration busy-wait at `0x8006F070`, and returns: a timing delay spliced into a kernel routine. A swap is its own inverse, so install and teardown both call it.
- `FUN_8006F118` calls `GetC0Table`, takes entry `6` (`ExceptionHandler`) and copies three words from `0x8006F180` over `+0x70..+0x78`. That blanks the immediate pair its install-side sibling `FUN_8006EFD0` reads to reconstruct a kernel address (and then patches at `+0x28` with a jump out into SCUS).
- Both are bracketed by `EnterCriticalSection` (`syscall(1)`) and `FlushCache` (A0 `0x44`).
- The install veneer is `FUN_8006EE8C(pad_enable)`: `ChangeClearPAD(0)`, `InitCARD`, then `_EFD0` + `_F088`. `FUN_8006EF18` is its teardown mirror, which the caller `FUN_8002035C` runs after closing eight kernel event handles.

Owning page: [`functions/runtime-libs.md`](../functions/runtime-libs.md#the-bios-kernel-patch-cluster-8006ee8c--8006ef18).

### `_DAT_8007B910` is the live audio level, not screen brightness

*Status:* resolved - the cell is a **volume** and `_DAT_8008457C` is its persistent reference. Evidence: `disassembly`.

- `FUN_80062004(a, b, c)` tail-calls `FUN_80061EDC(a, 0, b, c)` = `SsSeqSetVol(slot, channel 0, vol, ...)`, so the halved cell (`(v << 15) >> 16`) that `FUN_800267A8` passes lands in the volume argument.
- `FUN_80026478` hands `v >> 1` to `FUN_8002657C`, which writes it as both channels of `FUN_80064890(slot, vol_l, vol_r)`: a symmetric level, not a directional pan.
- The dumped corpus holds **26 read sites** of the cell: `SsSeqSetVol` (six), `SpuSetCommonAttr` (`FUN_8006BCB4`, four, each building an `SpuCommonAttr` on the stack with the cell in the CD-volume pair), the audio-context volume re-apply `FUN_8002614C`, `FUN_8002657C`, and arithmetic / tween plumbing. None reaches a draw primitive.
- The cold reset `FUN_8001FFA4` seeds `0xD7` into both `_DAT_8008457C` and `_DAT_8007B910`, then calls `FUN_8002614C(0)`. A `0..255` cell halved is libsnd's `0..0x7F`.

What the ramps are:

- Battle-action states `0x35` / `0x6F` / `0x70` duck the mix to 75% of the configured level (50% for spell ids `>= 0x99`); `0x51` restores it.
- The world-map sub-list halves it on open and doubles it on close.
- The field VM's `MENU_CTRL` sub-`0xD` sets it to `(input * _DAT_8008457C) >> 12`, a percentage of the player's setting.

Not brightness: a summon does dim the screen in step, but through a different scalar, `_DAT_8007B440`, ramped by `FUN_801ED308` and drawn by the wipe / curtain emitter `FUN_8003479C` (clamped `0xF2`).

Port names: `BattleActionHost::duck_audio_level`, `BattleEvent::DuckAudioLevel`, `SubListEffect::ScaleAudioLevel`, `PanelActorHost::audio_level` (seeded `0xD7`). Owning page: [`battle-action.md`](../../subsystems/battle-action.md#the-_dat_8007b910-ramps-are-an-audio-duck).

### Key-on pitch: unity on centre

*Status:* resolved - `note == center` keys **`0x1000`**, unity, 44.1 kHz.

Both the SFX / direct key-on path (`FUN_80065034`) and the sequencer note-on path (`FUN_80066308`) reach the same arithmetic (`FUN_80066e50` / `FUN_80066d8c`) and hand the result to `FUN_80067550`, which stores it verbatim into the shadow register file at `0x801CE084 + voice*16` (voice `+4` = pitch). Nothing rescales it afterwards:

```
n     = note + 60 - center + carry        (MIPS div: truncates toward zero)
pitch = PITCH[(n % 12) * 16 + fine] << (n / 12 - 5)
```

- `PITCH` is the 192-entry table at `DAT_8007A940` (SCUS file `0x6B140`). Every entry is exactly `floor(0x1000 * 2^(k/192))` (192 of 192 verified against the disc; first entry `0x1000`, last `0x1fe2`): a one-octave table at 1/16-semitone resolution starting at unity, with the octave applied by the shift. The closed form is exact, so no disc bytes are needed to reproduce it.
- The retail cue arm passes `fine = 0x40` at every traced `FUN_80065034` call site, so a cue keys half a semitone above the sequencer for the same tone. The two paths also differ in whether the fine index saturates or carries a whole semitone.
- A 22.05 kHz VAG body is authored with `center` twelve semitones high: the sample rate is already encoded in `center`. Applying a `22050/44100` factor on top keys every voice one octave low.
- Capture: 126 of 128 voices holding a non-zero staged pitch match this law exactly; the 2 misses are records whose bank was swapped after key-on.
- The recomp PCM oracle cannot check this law, because it mirrors retail's captured pitch into the engine SPU rather than deriving one.

Owning page: [`audio.md` § key-on pitch law](../../subsystems/audio.md).

### `FUN_80018DB0` is a rumble cadence, not an audio one

*Status:* resolved - the surrounding cluster is **libpad**, `DAT_800915DA` / `DB` are port 0's actuator bytes, and the kernel plays no sound. There is no retail footstep SFX cue id to pin. The identification is instruction-level: the `FUN_8006CE30` and `FUN_8001D230` windows are read straight out of `extracted/SCUS_942.54` at `0x800 + va - 0x80010000`.

- **`FUN_8006E2B4(buf0, buf1)` = `PadInitDirect`.** `FUN_8001D230` `bzero`s `0x44` = 2 x `0x22` bytes at `0x800840F8` and calls it with `(0x800840F8, 0x800840F8 + 0x22)` (`addiu a1,a0,0x22`), the canonical pair of 34-byte direct-mode report buffers. It clears `0x1E0` = 2 x `0xF0` at `0x801CE628` (one context per socket), stores the two buffers at each context `+0x30`, seeds each buffer `[0] = 0xFF` / `[1] = 0`, and fills six bytes at context `+0x5D` with `0xFF` (`PadSetActAlign`'s unassigned default). The pad pump `FUN_8001822C` decodes those buffers as `[status][type nibble][inverted u16 buttons]`, port 1 at `+0x22` / `+0x23`.
- **`FUN_8006CE30(socket, table, len)` = `PadSetAct`.** Three arguments in the instructions: `a0` passes untouched into the context resolver `jalr _DAT_801CE564`; `a1` / `a2` are stashed in `s0` / `s1` and forwarded. Ghidra's C drops `param_1` (artifact #1 in [`ghidra.md`](../../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims)). The tail `FUN_8006D7B4` stores `ctx+0x28 = table`, `ctx+0x34 = (u8)len`.
- **Siblings.** `FUN_8006CA7C` = `PadGetState` (report status byte through `ctx+0x30`, then normalises `ctx+0x49`). `FUN_8006CB3C` = `PadInfoMode`, whose `term = 4` branch returns the id-table length `ctx+0xE3` for `offs < 0` and otherwise the bounds-checked `((u16 *)ctx[0])[offs]`. `FUN_8006CDB0` = `PadSetActAlign`. `FUN_8006D1E0` / `FUN_8006D2AC` = `PadStartCom` / `PadStopCom` (`ChangeClearRCnt(3, 0)` vs `(3, 1)`). `FUN_8006EE8C` / `FUN_8006EEE0` call `ChangeClearPAD` (B0 `0x5B`) and wrap `InitCARD` / `StartCARD` (B0 `0x4A` / `0x4B`). `FUN_80056618` = `_bu_init`. The eight `OpenEvent` / `EnableEvent` pairs on `0xF4000001` / `0xF0000011` are the memory-card event set.
- **The bytes `FUN_80018DB0` writes are the actuator table.** It stores to `0x800915DA` / `0x800915DB`, and `FUN_80018F94` registers the same block per port with `PadSetAct(socket, block+2, 2)`, where `block = 0x800915D8 + (socket>>4)*0x40 + (socket&3)*0x10`. The `0x40` stride matches `FUN_8001D230`'s `s1+2` / `s1+0x42`, and the `0x80`-byte `bzero` matches 2 x `0x40`.
- **`DAT_8007B79C` is not a footstep-active flag.** `FUN_80018F94` sets it from `_DAT_800845A8 == 0 && PadInfoMode(socket, 2, 0) == 0` (the pad reports no extended-mode data). It selects between two actuator payload layouts: set -> `act[0] = 0x40` fixed with `act[1]` carrying the pulse; clear -> `act[0]` carries the pulse and `act[1]` is loaded with the low byte of `gp+0x618` every frame.
- **No audio call in the kernel.** Its other branch counts down ~1200 frames and calls `FUN_8005C034(9, 0)`, the retry wrapper over `CdControl` (`FUN_8005CF80`) issuing `CdlPause`: a CD-drive pause, not a voice stop or rewind.

Consequences:

- The two "per-voice trigger bytes" are one actuator payload: a per-step on / off pulse and an intensity level, transmitted by libpad every poll.
- `gp+0x614` / `gp+0x618` are vibration-intensity requests, not a locomotion speed (`+0x618` is written verbatim into an actuator level byte). Their writers are unpinned.
- `_DAT_8007B8A4` pinned at `2` across four field and overworld runs means nothing requests vibration while walking, as the game's Vibration options (battles / events / encounters) predict.

Not SsAPI: the `0x8006C000..0x8006F000` band does hold libspu / libsnd code and the stride-`0xF0` record array with an `0xFF` idle fill resembles a sequence-worker table, but the resolved context's `+0x30` is the button report `FUN_8001822C` decodes, `PadInfoMode`'s id-table branch has no sequencer reading, and the record count is 2 - the number of controller sockets.

Port: `engine-audio::footstep` mirrors the arithmetic and keeps its `// PORT:` tag. Owning page: [`audio.md`](../../subsystems/audio.md#not-ssapi-the-0x801ce628-cluster-is-libpad).

See `ghidra/scripts/funcs/8006e2b4.txt`, `8006ce30.txt`, `8006d7b4.txt`, `8006ca7c.txt`, `8006cb3c.txt`, `8006cdb0.txt`, `8006d1e0.txt`, `8006d2ac.txt`, `8001d230.txt`, `8001822c.txt`, `80018db0.txt`, `80018f94.txt`, `8005c034.txt`.

### XA channel map / STR demux SM

*Status:* resolved - both halves are statically decompiled from PROT 0970 at its base plus the SCUS St library.

- **No XA channel selector exists in the STR overlay.** FMV playback reads with Setmode `0xE0` (`Speed|RT|Size1`, sector filter off): the drive hardware-plays every ADPCM sector, and each `MOV/MV*.STR` interleaves exactly one XA track at `(file 1, chan 0)` (raw-subheader-verified across all six movies). The per-cue channel selector is the SCUS XA-clip sequencer `FUN_8003D764` (`CdlSetfilter {file 1, chan}`, mode `0xC8`), used for the `XA1..XA34` voice / music files, not for movies. Not the `\DATA\MOV.STR` container: that is a dev path in slots 11..=22 of the dispatch table, absent from the disc. See [cutscene.md § XA channel selection](../../subsystems/cutscene.md#xa-channel-selection).
- **The FMV dispatch table stride is 32 bytes** (`sll v0,v0,0x5` at `0x801CEC9C`), not 64. All nine retail slots `0..=8` resolve, every movie on the disc plays, and `MV3.STR` carries four abutting segments. The master dispatch `FUN_801CEA3C` hands each mid-game FMV off to a **return scene** (the seven-label table at `0x801CE8AC` plus a spawn word). Parser `legaia_asset::fmv_dispatch` (disc-gated `fmv_dispatch_real`); the engine resolver `legaia_engine_core::cutscene::fmv_index_to_str_filename` mirrors the nine-slot map and the return scenes. See [str-fmv-table.md](../../formats/str-fmv-table.md#authoritative-runtime-mapping).
- **The 24-byte records at `0x801CAE08` are libcd's directory cache**: `CdlFILE` structs (`[loc][size][name[16]]`). A name-first parse pairs each name with the next record's location, which is what produces an apparent "MV1 points at disc MV2 / MV6 points at XA15" shift. See [str-fmv-table.md](../../formats/str-fmv-table.md#directory-record-cache-0x801cae08-24-b-cdlfile-records).

### SPU reverb live routing (C7-REVERB)

*Status:* resolved - retail runs **`Studio C`, master-enabled, globally**. There is no selective per-cue reverb-enable source. Evidence: the mednafen save-state corpus, read by `legaia_mednafen::PsxSpu` (CLI `mednafen-state spu <state>`).

`PsxSpu` reads the SPU register shadow (`Regs` block): `reverb_master_enabled` (`SPUCNT` bit 7), `reverb_registers` (the 32 reverb coefficient / address registers at `0x1F801DC0..0x1F801DFF`), and `voice_reverb_mask` (the per-voice `EON` enable at `0x1F801D98` / `0x9A`, which mednafen also mirrors under its `Reverb_Mode` sub-entry - a byte-for-byte cross-check in every state). Across all 45 mednafen states (field / town / battle / summon / title / minigames):

- **Master reverb is always enabled** (`SPUCNT` bit 7 set everywhere). No scene toggles it.
- **The preset is `Studio C` everywhere.** The 32-register block is byte-identical in every state and matches the `StudioC` libspu preset exactly (`dAPF1=0x00E3`, `dAPF2=0x00A9`, work area `0x6FE0`). [`engine_audio::ReverbMode::identify`](../../../crates/engine-audio/src/spu/reverb.rs) resolves the captured block to `StudioC`.
- **Per-voice reverb send (`EON`) is broad**: 15-22 of 24 voices in any state, BGM and SFX alike. Reverb is the default routing, not a per-cue effect ("Spirit Arts / echo cues opt in, everything else dry" is not what the states show).

Port: the engine calls `Spu::set_retail_reverb` once at SPU init (`StreamResampler::new`): `ReverbMode::StudioC` with every voice routed. The PCM oracle's retail side reads the EON mask as a mask. Not modelled: the output-depth setting (`SpuSetReverbDepth`, `vLIN` / `vROUT`); the engine uses a fixed half-scale approximation.

Owning page: [`audio.md`](../../subsystems/audio.md#retail-reverb-routing---studio-c-always-on-capture-confirmed).

### bse.dat record columns and the gp+0x678 consumers

*Status:* resolved - grade `disassembly`.

`gp = 0x8007B318` (`lui gp,0x8008; addiu gp,gp,-0x4ce8` at `0x80026CA8`), so
the record-table pointer `FUN_8001FA88` stores at `0x8001FBC0`
(`sw a0,0x678(gp)`) lives at `0x8007B990`. Seven readers exist and every one
forms the address as `lui rX,0x8008` + `lw rY,-0x4670(rX)` - the pair the
five-form address scan does not accept, which is why the consumer read as
untraced: `0x8004FFAC` / `0x8004FFE0` / `0x80050078` in the battle SFX-cue
router `FUN_8004FE5C`, and `0x801CEE48` / `0x801CEFC0` / `0x801CF038` /
`0x801CF0A0` in the overlay-0971 debug sound test. Sweep:
`scripts/ghidra-analysis/find-gp-relative-refs.py`.

The columns are the static SFX-table columns, pinned by **shared code** rather
than analogy: `FUN_80016B6C` picks its arm at `0x80016C24` (`slti v0,s0,0x200`),
resolves either `0x8006F198 + id*8` or `record[id - 0x200]` off `gp[0x5B8]`,
and then falls into one block of field reads from `0x80016CB0` - `+0` program,
`+1` tone (`+i` per voice), `+2` note level, `+3` low-5 voice count / `0x20`
sustained, `+4` category into the 12-byte mixer record `0x80091508 + cat*12`.
`+4` is a `u8`: `+5..+7` are zero in every retail row. The router's two tinted
legs (`sb v1,-0x31c(v0)` at `0x8004FFEC`, `sb v1,0x40c(v0)` at `0x80050084`)
both reduce to `record[cue_id - 0x200] + 4`, and the sound test stores `7` at
`0xDC(gp[0x678])` before enqueuing cue `0x21B` = `0x200 + 27` - row index is
cue id minus `0x200`, and the category byte is rewritten per cue. Layout and
carriers: [`bse-dat.md`](../../formats/bse-dat.md).

### No second bse dat record family

*Status:* resolved (negative) - grade `capture`

Entry 888's 1,716-byte tail is byte-identical to entries 886 and 1063 at the
same file offsets; entry 1062's 1,616-byte tail matches entry 1056 (two bytes
differ where its SEQ padding clipped a row). The builder fill `C0 00 C1 00 C2 00
C3 00` occurs in 221 entries, always at offset `≡ 0x1C (mod 0x20)` (`VagAtr +
0x18`), and only these two carry it without a `pBAV` of their own.
`bse_bank::detect` stops on a residue row's structurally-zero `vib / por` fields
- reliable, but a foreign row's field. 1062 as a whole is a SEQ-only `music_01`
entry (sound-test track 72) borrowing another entry's bank. See
[`bse-dat.md`](../../formats/bse-dat.md).
