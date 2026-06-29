# Muscle Dome minigame

The **Muscle Dome** is a card-battle arena contest. The player fields one party character against an opponent; each round both sides choose a set of action cards from a hand, the chosen cards are queued onto the fighters, and the round is resolved by playing the queued actions out. It is **distinct from the fishing / slot / dance / Baka Fighter minigame-hub family** - it does not share their controller library.

Instead, the Muscle Dome runs **inside the battle-action overlay** (PROT entry **0898**, base `0x801CE818` - the same overlay [`move-power.md`](../formats/move-power.md) reads): its match SM `FUN_801d0748` and all its data tables (deck `0x801f4b8c`, sub-draw script `0x801f4d34`, victory messages `0x801f4dfc`) are resident there, so they are statically extractable from the disc (parser [`legaia_asset::muscle_dome`], disc-gated `muscle_dome_real`).

This matches the design - the arena reuses the battle engine wholesale (its fighters are battle actors, card plays resolve through the battle-action path). The "`overlay_muscle_dome.bin`" Duckstation capture was that battle-overlay slot resident during the arena, **not** a separate overlay; the `0977` "Ronginus" entry is only the mode-24 sub-id-5 door/init slot (arena roster + `other6` paths), not the match SM.

The whole contest runs on a shared context block at `_DAT_8007bd24` (referred to below as **ctx**). The fighters are ordinary battle actors reached through the global actor pointer table `&DAT_801c9370` (the same table the main battle system uses), so a "card play" ultimately resolves through the battle action machinery against actor records.

## Match state machine

The per-frame controller is `FUN_801d0748` (`overlay_muscle_dome_801d0748.txt`). It is the largest function in the overlay and drives the entire contest:

1. **Read input.** It folds the current pad-edge masks (`_DAT_8007b874` and `_DAT_8007b938`) into a single press mask `s2`. The four card-selection directions are the standard PSX face/d-pad bits `0x8000`, `0x2000`, `0x1000`, `0x4000`; the controller maps the pressed direction to one of the four queued card slots `ctx+0x1114 / +0x1118 / +0x111c / +0x1120` and records the chosen direction in `ctx+0x880`.
2. **Dispatch on the phase byte `ctx+6`.** This byte is the match phase. Confirmed phase values include `0x00`, `0x0a`, `0x0b`, `0x14`, `0x1e`, `0x28`, `0x32`, `0x3c`, `0x46`, `0x50`, `0x5a`, `0x5b`, `0x5c`, `0x5d`, `0x5e`, `0x64`, `0x65`, `0x66`, `0x67`, `0x6e`, `0x78`, `0xfe`. Phases advance by writing the next value back into `ctx+6` (`s3`). The terminal/idle phases `0x1e / 0x32 / 0x6e / 0xfe` also tick a spin/azimuth global at `_DAT_8007b938+2` each frame (the rotating dome camera). **(Confirmed: the dispatch is a `ctx+6` switch.) (Inferred: the exact ordering of phases is the deal → select → confirm → resolve → score loop; individual phase semantics below are partially confirmed.)**
3. **Run the presentation + camera.** Most phase arms call the presentation driver `FUN_801d388c` (card/sprite layout, see below) and the camera director `FUN_801d5854`, then play a UI/SFX cue through `func_0x8004fcc8`.

A small number of phase arms are confirmed by content:
- Phase `0x14` arm: copies the four pressed-direction card handles, sets `ctx+0x880` to the chosen direction bit, and marks the selected card slot's actor field `+0x1d = 2` (selection lock). This is the **player card-pick** phase.
- Phase `0x3c` / `0x46` / `0x50` arms: write the chosen action id into the fighter actor's `+0x1dd` (action) and `+0x1de` (action-state) fields and kick the battle action - this is **commit the queued cards and play them out**.
- Phase `0x6e` arm (`FUN_801d0748` near `0x801d0f24`): when a sub-result tag equals `0xb6` it computes a percentage `actor[+0x14c]*0x6c/actor[+0x14e]` (current/max ratio ×108) and renders it as a number - the **score / HP-percentage readout**.

Auxiliary per-frame helpers the controller calls every frame:
- `FUN_801d3444` - animates the round **time meter**: ramps a 0..0xc counter `DAT_801f4e0a` up while the phase tag `ctx+6 == 'P'` (0x50) and an enable flag is set, down otherwise, and maps it to a bar Y position. (`overlay_muscle_dome_801d3444.txt`.)
- `FUN_801d9bbc` - advances every **active animated sprite handle** (`ctx+0x1074[]`, up to 0x28 entries) one step toward its target screen position over a per-handle frame count; returns the count still in flight. (`overlay_muscle_dome_801d9bbc.txt`.)

## Card / move representation + selection

A "card" is an action drawn from the active fighter's move set. Cards are built and laid out by `FUN_801d388c` case `9` / `0x2c` (the **deal-hand** step):

- The hand has **four card slots**, built in a `do { … } while (uVar17 < 4)` loop.
- Each slot's card id comes from a small **deck-order table** at `&DAT_801f4b8c` / `&DAT_801f4b94` (a per-slot move-index list); the per-slot screen layout (X/Y/size) is read from a parallel layout table walked at stride 6. **(Confirmed: 4-slot loop reading `&DAT_801f4b8c`/`&DAT_801f4b94`; Inferred: these tables encode the standard four card categories.)**
- Each card carries a **cost** read from the fighter record: the loop loads a per-move cost byte (stored into `ctx[uVar17 + 0x14]`), normalises it against a `0x1e` baseline, and uses it both to size the card sprite and to debit the round's point budget.
- For party character index `2` the slot order is swapped (slots `0` and `3` exchange), i.e. the layout is mirrored for one of the fighters.

The **round point budget** lives at `ctx+0x6dc`, seeded from the fighter record field `+0x154` (the character's available "spirit"/AP pool); the running spent total is `ctx+0x6d8`. The number of cards already committed this round is the **selection index `ctx+0x19`**, and the slot currently being committed is `ctx+0x1a`.

`FUN_801d388c` case `0xb` is **commit one selected card**:
- It rejects the commit if the remaining budget `ctx+0x6dc` is smaller than the card's cost (`ctx[ctx+0x1a + 0x14]`) - you cannot overspend.
- Otherwise it spawns the committed-card sprite, **records the chosen move id into the fighter actor's queue** at `actor+0x1df + ctx+0x19` (an in-actor list of queued action ids), debits the cost from `ctx+0x6dc`, adds it to `ctx+0x6d8`, and increments `ctx+0x19`.

So selection = repeatedly pick a hand slot (a direction), which appends that slot's move id into the actor's `+0x1df` action queue while there is budget left.

## Round resolution

`FUN_801d388c` (`overlay_muscle_dome_801d388c.txt`, the 7820-byte card driver) is a large `switch(param_1)` over **presentation/animation step ids** (0..0x31). It does *not* itself compute damage; it lays out card and label sprites, runs the deal/commit loops above, and at its tail walks a **per-step script-record table** `PTR_DAT_801f4d34[param_1]`:

```
record = PTR_DAT_801f4d34[step]
record[0] = sub-draw count
record[1] = side/animation selector (1/2/3 → FUN_801d99bc / FUN_801d9ae8 panel slides)
record[2] = active-panel id (compared against ctx+0x275; record[2]+ctx+0x275 == 6 triggers a panel-swap reset of ctx+0x880..0x883)
record[3+2k], record[4+2k] = (element id, mode) pairs fed to FUN_801d8de8 for each sub-draw
```

Each sub-draw calls the HUD/element renderer `FUN_801d8de8(id, mode)` (see below). When the global `_DAT_800846c8` is set, the returned sprite handles are also stashed into `ctx+0x1114[]` and some are flagged `+0x1d = 2` (the four directional selection cards), tying the drawn cards back to the input slots.

The **resolution of queued cards** happens when the match controller advances into the commit phases (`0x3c`/`0x46`/`0x50` in `FUN_801d0748`): it walks the actor's `+0x1df` action queue, sets the actor's `+0x1dd`/`+0x1de` (action / action-state), and lets the shared battle-action path play each queued action and apply its effect to the opponent actor record (HP at actor `+0x14c`, max-HP at `+0x14e`). The `+0x1df` queue is re-zeroed at the start of each round (`FUN_801d388c` case `3` clears `+0x1e7`/`+0x1de`; case `0xb` re-seeds the budget and re-walks the queue). **(Confirmed: queue lives at actor+0x1df, budget gating; Inferred: precise per-card damage uses the standard battle formulas via the action ids, not a dome-local damage table.)**

The `func_0x80035f04` calls throughout are the shared screen-projection helper (project a world position to screen), used to anchor card and label sprites over the 3D fighters.

## Opponent + scoring

- The fighters are battle actors in `&DAT_801c9370`; the active fighter index is `ctx+0x13`, the player party member id is `ctx+0x20`, and the opponent id is `ctx+0x21` (clamped to ≤ 2 in `FUN_801d8de8`). The character→record mapping uses `&DAT_8007bd10` (per-actor character id) to index the 0x414-byte party records.
- The opponent's hand is built by the **same** deal/commit code paths (`FUN_801d388c` cases `9`/`0x2c`/`0xb`) keyed on the opponent's `ctx+0x13`; the AI simply commits cards from its own move set against the same budget rule. There is **no separate scripted AI table** in this overlay - the opponent uses the shared selection logic with its own record. **(Inferred from the symmetric use of `ctx+0x13` across both fighters; no dome-specific AI scorer was found.)**
- Scoring is HP-ratio based: the phase-`0x6e` arm renders `current_hp(+0x14c) * 108 / max_hp(+0x14e)` as the readout, and the win/lose phases (`0x64`/`0x65`/`0x66`/`0x67`) branch on the fighter HP fields. The HUD draws each fighter's HP/stat bars from record fields `+0x14e`/`+0x152`/`+0x172`/`+0x174` (`FUN_801d8de8` via `func_0x8003563c`, the bar/gauge primitive).
- **Reward:** `FUN_801d8de8` case `0x59` composes a victory message from a victory-message string-pointer table at `0x801f4dfc` plus a spell/seru name looked up in the static spell-name table `DAT_800754d0` (12-byte stride, indexed by `ctx+0x269 + 0x80`). This matches Muscle Dome awarding a Seru / magic on a win. **(Confirmed: the message pulls a name from the shared spell-name table at the player Seru-magic block `0x80+`.)**

## RAM state

All offsets are relative to the context base `_DAT_8007bd24` unless noted otherwise. Globals outside the context are listed with their absolute address.

| Address / offset | Type | Role | Confidence |
|---|---|---|---|
| `_DAT_8007bd24` | ptr | Muscle Dome context base (**ctx**) | Confirmed |
| `ctx+0x00` | u8 | fighter count (loop bound for per-fighter HUD draws) | Inferred |
| `ctx+0x06` | u8 | **match phase id** (the `FUN_801d0748` dispatch byte) | Confirmed |
| `ctx+0x0d` | u8 | camera/view sub-mode (selects `FUN_801d5854` view offsets) | Inferred |
| `ctx+0x13` | u8 | active fighter index into `&DAT_801c9370` | Confirmed |
| `ctx+0x14 … +0x17` | u8[4] | per-hand-slot card cost cache | Confirmed |
| `ctx+0x19` | u8 | **cards committed this round** (selection index) | Confirmed |
| `ctx+0x1a` | u8 | hand slot currently being committed | Confirmed |
| `ctx+0x1b`, `ctx+0x1c` | u8 | sprite step / advance used during card layout | Inferred |
| `ctx+0x1e` | u8 | pending HUD element id to redraw | Inferred |
| `ctx+0x1f` | u8 | panel-layout variant (1/2/3 → different on-screen panel arrangement) | Confirmed |
| `ctx+0x20` | u8 | player party member id | Confirmed |
| `ctx+0x21` | u8 | opponent id (clamped ≤ 2) | Confirmed |
| `ctx+0x269` | u8 | awarded spell/seru id (offset into spell-name table at `+0x80`) | Confirmed |
| `ctx+0x275` | u8 | active panel id (vs `PTR_DAT_801f4d34` record `[2]`) | Confirmed |
| `ctx+0x6b2` | u16 | per-frame tick counter (bumped each `FUN_801d388c` call) | Confirmed |
| `ctx+0x6d6` | - | scratch sub-block used for HUD layout (`pbVar10` base) | Inferred |
| `ctx+0x6d8` | u16 | **points spent this round** | Confirmed |
| `ctx+0x6dc` | u16 | **remaining point budget** (seeded from record `+0x154`) | Confirmed |
| `ctx+0x880` | u32 | chosen card-direction bitmask (`0x8000`/`0x2000`/`0x1000`/`0x4000`) | Confirmed |
| `ctx+0x884` | u32 | latched input mask for the round | Inferred |
| `ctx+0x1074[0..0x27]` | ptr[40] | active animated **sprite-handle** array | Confirmed |
| `ctx+0x1114 … +0x1120` | ptr[4] | the four directional **card-slot** sprite handles | Confirmed |
| `ctx+0x11b4[0..0x27]` | u8[40] | per-handle "active" flags (walked by `FUN_801d9bbc`) | Confirmed |
| actor `+0x14c` | u16 | fighter current HP | Confirmed |
| actor `+0x14e` | u16 | fighter max HP | Confirmed |
| actor `+0x154` | u16 | fighter point/AP pool (seeds the round budget) | Confirmed |
| actor `+0x1dd` | u8 | current action id | Confirmed |
| actor `+0x1de` | u8 | action state | Confirmed |
| actor `+0x1df + n` | u8[] | **queued card/action ids** for the round | Confirmed |
| `&DAT_801c9370` | ptr[] | global actor pointer table (fighters) | Confirmed |
| `&DAT_8007bd10` | u8[] | per-actor character id → party-record selector | Confirmed |
| `&DAT_801f4b8c` / `&DAT_801f4b94` | u8[] | hand deck-order / move-index tables | Confirmed |
| `&PTR_DAT_801f4d34` | ptr[] | per-step **sub-draw script-record** table | Confirmed |
| `&DAT_800754d0` | ptr[] | shared spell-name pointer table (reward name source) | Confirmed |
| `_DAT_8007b874`, `_DAT_8007b938` | u32 | pad-edge masks folded into the press mask | Confirmed |
| `_DAT_800846c0` | u32 | global contest sub-mode flag (gates camera/HUD arms) | Inferred |
| `_DAT_800846c8` | u32 | "store handles back into card slots" enable | Confirmed |
| `DAT_801f4e0a` | u8 | round time-meter counter (0..0xc) | Confirmed |

## Key functions

| Address | Role | Provenance |
|---|---|---|
| `FUN_801d0748` | Per-frame match controller: reads pad, dispatches on `ctx+6` phase, drives card pick / commit / resolve / score loop | `overlay_muscle_dome_801d0748.txt` |
| `FUN_801d388c` | Card/presentation driver: deal-hand (4 slots), commit-card, per-step sprite layout, runs the `PTR_DAT_801f4d34` sub-draw script | `overlay_muscle_dome_801d388c.txt` |
| `FUN_801d5854` | Camera / view director: 10-way (`param_2` 0..9) switch computing the dome view transform per phase | `overlay_muscle_dome_801d5854.txt` |
| `FUN_801d8de8` | HUD / element renderer: draws labels, HP/stat bars, card numbers, and the reward message; returns a sprite handle | `overlay_muscle_dome_801d8de8.txt` |
| `FUN_801d3444` | Round time-meter bar animation | `overlay_muscle_dome_801d3444.txt` |
| `FUN_801d9bbc` | Advances active sprite handles toward target screen positions | `overlay_muscle_dome_801d9bbc.txt` |
| `FUN_801f19ec` | Fighter model installer: relocates a TMD model bundle, uploads it, and binds it to a dome actor | `overlay_muscle_dome_801f19ec.txt` |

## Open

- The exact phase ordering and meaning of every `ctx+6` value (deal/select/confirm/resolve/win/lose) - partially confirmed; a live phase-byte capture would pin the full graph.
- The byte layout of the deck tables `&DAT_801f4b8c`/`&DAT_801f4b94` and the per-step script table `&PTR_DAT_801f4d34` (now known to be **battle-overlay 0898 rodata** at file offsets `0x26374` / `0x2651c`, statically extractable; only their access patterns are reversed here, not their full contents). `&DAT_801f4b8c[slot]` is a 4-entry per-slot move-set index list; `&DAT_801f4b84[move_id]` is a per-move display/cost lookup the commit path uses.
- Whether card resolution applies any dome-specific damage scaling or uses the shared `battle_formulas` unmodified.

## See also

**Reference** -
[Tile-board grid](tile-board.md) ·
[Battle action SM](battle-action.md) ·
[Spell table](../formats/spell-table.md) ·
[Overlay capture](../tooling/overlay-capture.md)
