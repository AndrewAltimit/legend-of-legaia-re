-- autorun_cast_sequence.lua
--
-- Retail side of the per-cast frame-sequence oracle
-- (docs/tooling/cast-sequence-oracle.md, driver scripts/ci/cast-sequence.py).
--
-- From a catalogued PRE-cast battle state, rewrite the acting seat's queued
-- action into the cast under test, then write one metadata row per vsync from
-- the inject, and the DISPLAYED frame every LEGAIA_SHOT_EVERY vsyncs from the
-- caster's cast begin (action state 0x28), to LEGAIA_TAIL vsyncs past the
-- cast's Done band. The engine side
-- (`play-window` with LEGAIA_DIAG_CAST_SEQ) writes the same metadata, and
-- `legaia-engine cast-sequence-score` pairs the two on the action-SM clock.
--
-- The injection is the one autorun_w3a_cast_oracle.lua proved out: at action
-- state ctx[+0x07] == 0x0A of the acting seat (past the command-flow
-- validator, before state 0x0C reads the category byte) write
--     actor[+0x1DE] = 2 (Magic), actor[+0x1DF] = spell, actor[+0x1DD] = target
-- and retail pages the module in and runs it on retail bytes.
--
-- Outputs (--out-dir):
--   meta.csv     one row per vsync from the inject: the action-SM state, the
--                module phase ctx[+0x279], the acting seat, the adaptive
--                frame step *(0x1F800393), the countdown / camera words the
--                capture gates use, every seat's HP, and the main loop's
--                frame counter `frame` (gp+0x480), the side-band applier
--                stage `c276` (ctx[+0x276]), and the read-span poll's words
--                `xaspan` / `cdbusy` / `cdmode` / `cdread`, and the BIOS
--                `rand()` seed word 0x80009010 (`rand`, the state A(2Fh)
--                steps). `shot` names the frame taken on that vsync, if any.
--   r_<vsync>.raw  PCSX.GPU.takeScreenShot() pixels (BGR555, or BGR888 when
--                `bpp` says 24); width / height / bpp are on the meta row.
--   sfx.csv      one row per sound cue the drainer FUN_80016B6C plays: the
--                cue ring DAT_8007B6D8[4] (i16 ids) with its countdowns
--                DAT_8007C338[4] (u32, vsyncs), sampled every vsync. A slot
--                plays when its countdown is zero and its id >= 0
--                (0x80016BF8..0x80016C20) and the next frame's ager clears
--                it, so the edge into that condition is one cue. `drain`
--                is the drainer's own gate (0x8007BABC == 0x8007BAA0,
--                0x80016BC0..0x80016BD4).
--   sfxcall.csv  with LEGAIA_SFX_CALLERS=1 (breakpoint core): every call
--                into the cue producers FUN_80035B50 / FUN_80035BD0, the
--                battle router FUN_8004FE5C and the dispatcher FUN_8004FCC8,
--                with a0, a1 and ra - who fired each sfx.csv cue.
--                A `store` row is a write into the ring's ids
--                (0x8007B6D8..DF): `a0` is the storing pc.
--   seats.csv    per vsync from the cast, seats 0..7: clip +0x1D9, staged
--                +0x1DA, cue-track cursor +0x1F6 (the anim cue walk
--                FUN_800508DC's), and rates.csv beside it: each seat's animation
--                rate +0x21D and node cursor +0x68, off which a reaction's
--                pace reads.
--   watch.csv    with LEGAIA_WATCH_W=addr,addr (breakpoint core): each store into
--                these bytes with its pc, ra and registers.
--   manifest.txt the run's knobs.
-- These are retail pixels and retail RAM: never commit them.
--
-- Env:
--   LEGAIA_SPELL         action id to force (required; e.g. 0x86)
--   LEGAIA_PHYSICAL      1 = no inject: record the action a catalogued
--                        mid-strike state already holds, from the first vsync
--                        in the strike loop (action state 0x1E) to past the
--                        Done band, for whichever seat is acting. The cue
--                        channel of a monster's physical attack.
--   LEGAIA_CASTER_SEAT   seat whose queued action is rewritten (0); 255
--                        writes every party seat. A monster seat (3..7) is
--                        written on the first 0x0A of any seat: state 0x0C
--                        re-picks the actor, so 0x0A's seat is not the one
--                        that acts
--   LEGAIA_TARGET_SEAT   actor-table seat to target (3)
--   LEGAIA_INJECT_STATE  action state to inject on (0x0A)
--   LEGAIA_CAST_QUEUE    hex bytes (`09,0e,ff`) written behind the spell at
--                        +0x1E0.. by LEGAIA_MONSTER_PICK: the clip, shot and
--                        terminator a magic pick stores. A pick that was not
--                        a cast leaves no terminator, and 0x2A parks on it.
--   LEGAIA_MONSTER_PICK  1 = rewrite a MONSTER caster's action where the AI
--                        pick returns instead of at LEGAIA_INJECT_STATE: a
--                        breakpoint on 0x801DAF50 (FUN_801DABA4's monster
--                        arm, straight after `jal 0x801E9FD4`, before
--                        `jal 0x801DB124` reads the queue) writes the cast
--                        over what FUN_801E9FD4 just queued for the seat
--                        ctx[+0x274] names. The 0x0A write cannot do this
--                        for a monster: the pick runs in the same pass as
--                        state 0x0C's category read and overwrites it.
--                        Needs the BP core (not --fast); any seat 3..7 that
--                        owns a clip for the spell can be recorded from any
--                        battle state that reaches its turn
--   LEGAIA_FORCE_ROW     comma list of bytes written over the formation row
--                        FUN_801DA600 returns (needs the BP core, not --fast)
--   LEGAIA_PRESS_UNTIL   tap CROSS until this vsync while not injected (600)
--   LEGAIA_PRESS_PERIOD  CROSS tap period in vsyncs (60)
--   LEGAIA_PARTY_SPD     write this SPD into every party seat while in battle
--                        so the party acts first (0 = leave)
--   LEGAIA_MP_TOPUP      caster MP at the inject (999)
--   LEGAIA_POKE32        comma list of `addr=value` words written at the
--                        inject (hex or decimal), e.g. the per-seat
--                        choreography latch PROT 0946's arm 0 forks on
--                        (`0x801C8FE4=1` sends a seat-0 cast of 0x56 down
--                        Big Wave's branch instead of Call Wave's)
--   LEGAIA_LEARN_LEVEL   put the spell on the caster's list at this level at
--                        the inject (first list slot of roster record = seat).
--                        Off the list, FUN_801DD864 reads slot 0x20 and level 0
--                        wraps into a capped hit: a damage measurement wants
--                        this set (cast-sequence-oracle.md)
--   LEGAIA_MONSTER_HP    write this HP into every living monster seat at the
--                        cast begin, so a cast's whole damage shows in the HP
--                        column (a monster's own swing scales with its HP, so
--                        not before its turn)
--   LEGAIA_SHOT_EVERY    vsyncs between frames (2)
--   LEGAIA_MAX_SHOTS     frame cap (1500)
--   LEGAIA_TAIL          vsyncs to keep going past the Done band (60)
--   LEGAIA_RAM_AT        comma list of vsyncs past the cast begin at which the
--                        full 2 MiB main RAM is written as ram_<vsync>.bin
--                        (the absolute vsync, as in meta.csv), and the whole
--                        emulator state beside it as state_<vsync>.rawsstate
--                        (VRAM included: sstate_vram.py); empty = none
--   LEGAIA_CAST_PARTS    1 = also write parts.csv from the cast begin, one row per
--                        vsync per live move-VM part (actor-list nodes on the
--                        part tick FUN_80021DF4): the retail half of the engine
--                        recorder's LEGAIA_DIAG_CAST_PARTS, colour word, level,
--                        draw kind, draw mode, packed colours, radius and height
--                        channels, position, scale, UV / tpage / CLUT and the
--                        mode-3 H / S / V adds
--                        and fxpool.csv beside it: every live effect-pool child
--                        (_DAT_8007BD30) with its frame count, cursor and position
--   LEGAIA_POSE_OBJECTS  1 = pose.csv (breakpoint core): at each burning-body emit
--                        on the summon creature, its cursor, pair, facing and
--                        decoded object table
--   LEGAIA_PRIM_CALLS    1 = primcall.csv (breakpoint core): each prim dispatcher
--                        FUN_80043390 entry with its colour word, level, caller
--                        and first group words, and each cued leaf it reaches
--   LEGAIA_SAVE_INJECT   1 = write the whole emulator state as
--                        state_inject.rawsstate on the first vsync after the
--                        inject: a pre-cast state with the cast already
--                        queued, which replays on its own (back it up to the
--                        save library with manage-states.py)
--   LEGAIA_PRE_TRACE     1 = also write pre.csv, one row per vsync BEFORE the
--                        inject: the action state, the acting seat, the frame
--                        step, the yaw counter ctx[+0x6DA], the strike cursor
--                        ctx[+0x15], and per seat 0..3 the committed / staged
--                        clip (+0x1D9 / +0x1DA), the rate +0x21D, the node's
--                        anim cursor (+0x22C -> +0x68), the ground pair and
--                        the per-action damage total +0x00: the retail
--                        timeline of whatever the party played on the way to
--                        the cast (a combo's per-clip cadence); and the
--                        battle camera: the live pitch / yaw, TR trio and
--                        focus trio (stored negated) with the depth word
--                        ctx[+0x6D0] and all nine {step, endpoint} records
--                        of the tween table ctx[+0x118C]
--   LEGAIA_PRE_SHOTS     1 = with LEGAIA_PRE_TRACE, also take a frame each
--                        vsync before the inject while the action state is
--                        a strike / return / Done one (0x1E..0x20, 0x50..0x5A)
--                        (pre_NNNNN.raw, named in pre.csv's shot column):
--                        which pass's pose the display shows
--   LEGAIA_PARK          quit when the action state and the module phase
--                        have not moved for this
--                        many vsyncs after the inject (900): a cast the
--                        caster cannot run parks (a monster with no clip
--                        for the spell sits on 0x2A)
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH  = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES       = probe.getenv_num("LEGAIA_FRAMES", 4000)
local SPELL        = probe.getenv_num("LEGAIA_SPELL", 0x86)
local CASTER_SEAT  = probe.getenv_num("LEGAIA_CASTER_SEAT", 0)
local TARGET_SEAT  = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local INJECT_STATE = probe.getenv_num("LEGAIA_INJECT_STATE", 0x0A)
local FORCE_ROW    = probe.getenv("LEGAIA_FORCE_ROW", "")
local MONSTER_PICK = probe.getenv_num("LEGAIA_MONSTER_PICK", 0) ~= 0
local CAST_QUEUE = {}
for v in string.gmatch(probe.getenv("LEGAIA_CAST_QUEUE", ""), "[^,]+") do
    CAST_QUEUE[#CAST_QUEUE + 1] = tonumber(v, 16)
end
local PHYSICAL     = probe.getenv_num("LEGAIA_PHYSICAL", 0) ~= 0
local SAVE_INJECT  = probe.getenv_num("LEGAIA_SAVE_INJECT", 0) ~= 0
local inject_saved = false
local PRESS_UNTIL  = probe.getenv_num("LEGAIA_PRESS_UNTIL", 600)
local PRESS_PERIOD = probe.getenv_num("LEGAIA_PRESS_PERIOD", 60)
local MP_TOPUP     = probe.getenv_num("LEGAIA_MP_TOPUP", 999)
-- Hung off `probe`: on_capture sits at Lua's 60-upvalue cap, and one over it
-- fails the whole file at load.
probe.cast_cfg = {
    party_spd = probe.getenv_num("LEGAIA_PARTY_SPD", 0),
    learn_level = probe.getenv_num("LEGAIA_LEARN_LEVEL", 0),
    monster_hp = probe.getenv_num("LEGAIA_MONSTER_HP", 0),
}
local POKE32 = {}
for kv in string.gmatch(probe.getenv("LEGAIA_POKE32", ""), "[^,]+") do
    local a, v = string.match(kv, "^%s*([^=]+)=(.+)$")
    if a ~= nil then POKE32[#POKE32 + 1] = { tonumber(a), tonumber(v) } end
end
local SHOT_EVERY   = probe.getenv_num("LEGAIA_SHOT_EVERY", 2)
local MAX_SHOTS    = probe.getenv_num("LEGAIA_MAX_SHOTS", 1500)
local TAIL         = probe.getenv_num("LEGAIA_TAIL", 60)
local PARK         = probe.getenv_num("LEGAIA_PARK", 900)
local PRE_TRACE    = probe.getenv_num("LEGAIA_PRE_TRACE", 0) ~= 0
local PRE_SHOTS    = probe.getenv_num("LEGAIA_PRE_SHOTS", 0) ~= 0
local RAM_AT = {}
for v in string.gmatch(probe.getenv("LEGAIA_RAM_AT", ""), "[^,]+") do
    RAM_AT[tonumber(v)] = true
end

local ACTOR_TABLE  = 0x801C9370
local CTX_PTR      = 0x8007BD24
local LOADER_ID    = 0x8007BC4C
local GIMARD_CD    = 0x801F7960   -- PROT 0903's module countdown word

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
-- The main loop's frame counter gp+0x480 (gp = 0x8007B318): FUN_80016B6C
-- bumps it once per pass (`sw v0,0x480(gp)` at 0x80016BA8), so the vsyncs
-- between two of its values are what one game frame really took.
local FRAME_COUNTER = 0x8007B798
-- The words FUN_8003DE7C(1) (the read-span poll state 0x32 waits on) reads:
-- the XA read-span countdown gp+0x91C (`xaspan`), the gate halfwords at
-- 0x8007BA70 (`cdbusy`) and 0x8007B8C2 (`cdread`), and the mode halfword at
-- 0x8007B83C (`cdmode`, bit 0 the debug print).
local XA_SPAN = 0x8007BC34
local CD_BUSY = 0x8007BA70
local CD_MODE = 0x8007B83C
local CD_READ = 0x8007B8C2
-- The BIOS rand() seed (A(2Fh): seed = seed * 0x41C64E6D + 0x3039, returns
-- (seed >> 16) & 0x7FFF): the one stream SCUS and every overlay draw from.
local RAND_SEED = 0x80009010
-- The frame driver's cost ring `0x80084098` (sixteen halfwords) and its write
-- cursor `gp+0x440` (`0x8007B758`, the slot the next pass stores to): the
-- newest entry is the hblanks the last finished frame cost (`FUN_800173BC`).
-- (On the probe table: the capture callback is at luajit's upvalue cap.)
function probe.frame_cost_newest()
    local cur = bit.band((probe.read_u32(0x8007B758) or 1) - 1, 15)
    return probe.read_u16(0x80084098 + cur * 2) or 0
end
local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function s32(a)
    local v = u32(a)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
-- parts.csv: every live move-VM part, in list order. On the probe table: the
-- frame body already sits at Lua's limit of 60 upvalues.
local CAST_PARTS = probe.getenv_num("LEGAIA_CAST_PARTS", 0) ~= 0
function probe.cast_parts_row(cast_seen, elapsed, st7, cx)
    if not (cast_seen and CAST_PARTS) then return end
    if probe.cast_parts == nil then
        probe.cast_parts = probe.csv_open(probe.out_path("parts.csv"),
            "vsync,st7,phase,node,flags,colour,level,kind,mode5a,mode9e,a0,a4,b4,b6,b8,ba,x,y,z,scale,f52,c9c,a8,ac,b0,b2,h90,s92,v94,vz,f7a,b6d,f42,r24,r26,r28")
    end
    -- fxpool.csv: every live child of the effect pool `_DAT_8007BD30`
    -- (128 x 32-byte slots at `+0x10`): frame count, cursor, position >> 8.
    if probe.cast_fxpool == nil then
        probe.cast_fxpool = probe.csv_open(probe.out_path("fxpool.csv"),
            "vsync,st7,phase,slot,frames,cursor,x,y,z")
    end
    local pool = u32(0x8007BD30)
    if pool >= 0x80000000 and pool < 0x80200000 then
        for k = 0, 127 do
            local c = pool + 0x10 + 32 * k
            local frames = u8(c)
            if frames ~= 0 then
                probe.cast_fxpool:row("%d,%d,%d,%d,%d,%d,%d,%d,%d", elapsed, st7, u8(cx + 0x279), k,
                    frames, u8(c + 2), math.floor(s32(c + 0xC) / 256), math.floor(s32(c + 0x10) / 256), math.floor(s32(c + 0x14) / 256))
            end
        end
    end
    for k = 0, 8 do
        local n = u32(0x8007C34C + 4 * k)
        local guard = 0
        while n >= 0x80000000 and n < 0x80200000 and guard < 512 do
            guard = guard + 1
            if u32(n + 0xC) == 0x80021DF4 then
                probe.cast_parts:row("%d,%d,%d,0x%08X,0x%08X,0x%08X,%d,%d,%d,0x%04X,0x%08X,0x%08X,%d,%d,%d,%d,%d,%d,%d,%d,0x%04X,%d,0x%04X,0x%04X,0x%04X,0x%04X,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
                    elapsed, st7, u8(cx + 0x279), n, u32(n + 0x10), u32(n + 0x74), s32(n + 0x78),
                    s16(n + 0x56), s16(n + 0x5A), u16(n + 0x9E), u32(n + 0xA0), u32(n + 0xA4),
                    s16(n + 0xB4), s16(n + 0xB6), s16(n + 0xB8), s16(n + 0xBA),
                    s16(n + 0x14), s16(n + 0x16), s16(n + 0x18), u16(n + 0x72), u16(n + 0x52),
                    s16(n + 0x9C), u16(n + 0xA8), u16(n + 0xAC), u16(n + 0xB0), u16(n + 0xB2),
                    s16(n + 0x90), s16(n + 0x92), s16(n + 0x94), s32(n + 0x34), s16(n + 0x7A), u8(n + 0x6D), s16(n + 0x42),
                    s16(n + 0x24), s16(n + 0x26), s16(n + 0x28))
            end
            n = u32(n)
        end
    end
end
local function ptr_ok(p) return p >= 0x80000000 and p < 0x80200000 end
local function ctxp()
    local c = u32(CTX_PTR)
    if not ptr_ok(c) then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if not ptr_ok(p) then return nil end
    return p
end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

-- `fade_back`: which ordering table holds the screen fade's quad on this
-- vsync (`retail_compare_battle::display_lag::fade_passes_back`). The fade
-- actor links its POLY_F4 (0x2A / 0x2B) into OT bucket 1 of the table its
-- pass builds, so the grab shows the fade `2` passes back when the current
-- record (`gp+0x434`) already links one, `1` when only the other does, and
-- `0` stands for neither.
local DOUBLE_BUFFER, DOUBLE_BUFFER_STRIDE, DOUBLE_BUFFER_INDEX = 0x8007BF30, 0x74, 0x8007B74C
local function kseg0(w) return bit.bor(bit.band(w, 0xFFFFFF), 0x80000000) % 0x100000000 end
local function record_links_fade(i)
    local ot = u32(DOUBLE_BUFFER + i * DOUBLE_BUFFER_STRIDE + 0x70)
    local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end
    if not in_ram(ot) then return false end
    local p = kseg0(u32(ot + 4))
    for _ = 1, 64 do
        if (p >= ot and p < ot + 0x4000) or not in_ram(p) then return false end
        local tag = u32(p)
        local code = bit.band(bit.rshift(u32(p + 4), 24), 0xFE)
        if bit.rshift(tag, 24) ~= 0 and code == 0x2A then return true end
        p = kseg0(tag)
    end
    return false
end
-- On the probe table, like `cast_parts_row`: the frame body has no upvalue
-- to spare.
function probe.cast_fade_back()
    local cur = bit.band(u16(DOUBLE_BUFFER_INDEX), 1)
    if record_links_fade(cur) then return 2 end
    if record_links_fade(1 - cur) then return 1 end
    return 0
end

local meta, body, cam, pre, creature
-- The cue ring's sampler, hung off `probe` (on_capture already sits at
-- Lua's 60-upvalue cap, so it takes no new upvalue).
probe.cast_sfx = {
    ring = 0x8007B6D8,     -- i16 ids[4]
    timers = 0x8007C338,   -- u32 countdowns[4]
    ready = { false, false, false, false },
    prev = { -1, -1, -1, -1 },
    csv = nil,
}
local injected, cast_seen, done_seen = false, false, false
local inject_vsync, cast_vsync, done_vsync = -1, -1, -1
local forced_row = false
local last_elapsed = 0
local shots = 0
local last_flow = -1
local park_st7, park_since = -1, 0
local parked = false
local ground_at_cast = ""
local guard_at_cast = ""
local caster_at_cast = -1
local target_at_cast = -1

-- Each pool slot's live ground pair +0x34 / +0x38 and facing +0x46 (`-` for
-- an empty slot; the facing turns the body pair the range law reads):
-- the engine's inflight seed seats every combatant there at its dispatch, so
-- an action retail ran before the cast (a monster that struck first and was
-- not walked home) leaves the same stage.
probe.cast_seats = {
    csv = nil,
    rates = nil,
    row = function(self, elapsed, actor_of)
        if self.rates == nil then
            local h = {}
            for k = 0, 7 do h[#h + 1] = string.format("s%drate,s%dcur", k, k) end
            self.rates = probe.csv_open(probe.out_path("rates.csv"), "vsync," .. table.concat(h, ","))
        end
        if self.csv == nil then
            self.csv = probe.csv_open(probe.out_path("seats.csv"),
                "vsync,s0clip,s0stg,s0cue,s1clip,s1stg,s1cue,s2clip,s2stg,s2cue,s3clip,s3stg,s3cue," ..
                "s4clip,s4stg,s4cue,s5clip,s5stg,s5cue,s6clip,s6stg,s6cue,s7clip,s7stg,s7cue")
        end
        local cols, rate_cols = {}, {}
        for k = 0, 7 do
            local a = actor_of(k)
            if a ~= nil then
                cols[#cols + 1] = string.format("%d,%d,%d", probe.read_u8(a + 0x1D9) or 0,
                    probe.read_u8(a + 0x1DA) or 0, probe.read_u8(a + 0x1F6) or 0)
                local node = probe.read_u32(a + 0x22C) or 0
                rate_cols[#rate_cols + 1] = string.format("%d,%d", probe.read_u8(a + 0x21D) or 0,
                    node >= 0x80000000 and (probe.read_u16(node + 0x68) or 0) or -1)
            else
                cols[#cols + 1] = "-1,-1,-1"
                rate_cols[#rate_cols + 1] = "-1,-1"
            end
        end
        self.csv:row("%d,%s", elapsed, table.concat(cols, ","))
        self.rates:row("%d,%s", elapsed, table.concat(rate_cols, ","))
    end,
}

local function ground_pairs()
    local t = {}
    for s = 0, 7 do
        local a = actor(s)
        if a ~= nil and u16(a + 0x14E) > 0 then
            local x, z = u16(a + 0x34), u16(a + 0x38)
            if x >= 0x8000 then x = x - 0x10000 end
            if z >= 0x8000 then z = z - 0x10000 end
            t[#t + 1] = string.format("%d:%d:%d", x, z, u16(a + 0x46) % 0x1000)
        else
            t[#t + 1] = "-"
        end
    end
    return table.concat(t, ",")
end

-- The party seats standing in the Spirit stance (`+0x1DE == 4`), as a comma
-- list: both per-move wrappers double the defender roll on it and the
-- finisher halves the hit, so the engine side must stand the same seats.
local function guard_seats()
    local t = {}
    for s = 0, 2 do
        local a = actor(s)
        if a ~= nil and u16(a + 0x14E) > 0 and u8(a + 0x1DE) == 4 then
            t[#t + 1] = tostring(s)
        end
    end
    return table.concat(t, ",")
end

-- pre.csv's camera columns: the live pose, the depth word ctx[+0x6D0] and
-- the tween table's nine {step, endpoint} records as `step:end` pairs joined
-- by `/`; then, under LEGAIA_PRE_SHOTS, a frame in the strike / return / Done
-- states (its file name, or empty).
local function pre_cam_cols(cx, st7, elapsed)
    local tw = {}
    for k = 0, 8 do
        tw[#tw + 1] = string.format("%d:%d", s16(cx + 0x118C + k * 4), s16(cx + 0x118C + k * 4 + 2))
    end
    local shot = ""
    if PRE_SHOTS and ((st7 >= 0x1E and st7 <= 0x20) or (st7 >= 0x50 and st7 <= 0x5A)) then
        local ok, ss = pcall(PCSX.GPU.takeScreenShot)
        if ok and ss ~= nil then
            shot = string.format("pre_%05d.raw", elapsed)
            local fh = io.open(probe.out_path(shot), "wb")
            if fh ~= nil then fh:write(tostring(ss.data)); fh:close() else shot = "" end
        end
    end
    return string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%s",
        u16(0x8007B790) % 4096, u16(0x8007B792) % 4096,
        s32(0x800840B8), s32(0x800840BC), s32(0x800840C0),
        -s32(0x80089118), -s32(0x8008911C), -s32(0x80089120),
        s16(cx + 0x6D0), table.concat(tw, "/"), shot)
end

local function take_shot(elapsed)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return "", 0, 0, 0 end
    local name = string.format("r_%05d.raw", elapsed)
    local fh = io.open(probe.out_path(name), "wb")
    if fh == nil then return "", 0, 0, 0 end
    fh:write(tostring(ss.data)); fh:close()
    local bpp = 16
    if ss.bpp == "BPP_24" or (tonumber(ss.bpp) or 16) > 16 then bpp = 24 end
    shots = shots + 1
    return name, tonumber(ss.width) or 320, tonumber(ss.height) or 240, bpp
end

-- One pre.csv row (LEGAIA_PRE_TRACE): the battle state, each seat's clip and
-- stand, and the camera (pre_cam_cols).
local function pre_trace_row(cx, st7, acting, elapsed)
    if pre == nil then return end
    local cols = {}
    for s = 0, 3 do
        local a = actor(s)
        if a ~= nil then
            local node = u32(a + 0x22C)
            local x, z = u16(a + 0x34), u16(a + 0x38)
            if x >= 0x8000 then x = x - 0x10000 end
            if z >= 0x8000 then z = z - 0x10000 end
            local last = s == 3 and u16(a + 0x14C) or u16(a)
            cols[#cols + 1] = string.format("%d,%d,%d,%d,%d,%d,%d",
                u8(a + 0x1D9), u8(a + 0x1DA), u8(a + 0x21D),
                ptr_ok(node) and u16(node + 0x68) or -1, x, z, last)
        else
            cols[#cols + 1] = "-1,-1,-1,-1,0,0,0"
        end
    end
    pre:row("%d,%d,%d,%d,%d,%d,%d,%d,%s,%s", elapsed, u8(cx + 6), st7, acting,
        probe.read_scratch_u8(0x1F800393), u16(cx + 0x6DA), u8(cx + 0x15),
        u32(cx + 0x87C), table.concat(cols, ","), pre_cam_cols(cx, st7, elapsed))
end

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== cast sequence == spell=0x%02X caster=%d target=%d every=%d",
            SPELL, CASTER_SEAT, TARGET_SEAT, SHOT_EVERY))
        -- The debugger core pauses on a read of unmapped memory (psxmem.cc, after
        -- returning the same 0xFF the recompiler returns) and nothing would resume
        -- it: the run sat there until the driver's timeout.
        probe.cast_resume = PCSX.Events.createEventListener("ExecutionFlow::Pause", function()
            PCSX.log("[cast-seq] emulator paused itself: resumed")
            PCSX.resumeEmulator()
        end)
        probe.env.write_manifest("autorun_cast_sequence.lua", {
            sstate = SSTATE_PATH, spell = string.format("0x%02X", SPELL),
            caster_seat = CASTER_SEAT, target_seat = TARGET_SEAT,
            inject_state = INJECT_STATE, force_row = FORCE_ROW,
            party_spd = probe.cast_cfg.party_spd, monster_pick = MONSTER_PICK and 1 or 0, shot_every = SHOT_EVERY, tail = TAIL,
        })
        meta = probe.csv_open(probe.out_path("meta.csv"),
            "vsync,st7,phase,acting,step,flow,c6d8,c6da,c87c,cd7960,loader," ..
            "hp0,hp1,hp2,hp3,hp4,hp5,hp6,hp7,shot,w,h,bpp,frame,c276,xaspan,cdbusy,cdmode,cdread,rand,fade_back,cost")
        -- The acting seat's body against the camera that frames it: clip,
        -- node cursor, live pair +0x34/+0x38, body pair +0x3C/+0x40, and the
        -- camera focus (stored negated) with its tween endpoints.
        body = probe.csv_open(probe.out_path("body.csv"),
            "vsync,st7,acting,clip,cursor,lx,lz,bx,bz,fx,fz,ex,ez,c87c")
        -- The live camera pose (pitch / yaw `0x8007B790/92`, TR
        -- `0x800840B8..C0`) with the yaw and TR z records of the tween
        -- table `ctx[+0x118C]` (`{step, endpoint}`): a module shot's walk.
        creature = probe.csv_open(probe.out_path("creature.csv"), "vsync,clip,staged,cursor,x,y,z,facing")
        cam = probe.csv_open(probe.out_path("cam.csv"),
            "vsync,st7,phase,step,pitch,yaw,trx,try,trz,yaw_inc,yaw_end,trz_inc,trz_end")
        probe.cast_sfx.csv = probe.csv_open(probe.out_path("sfx.csv"), "vsync,frame,st7,phase,slot,id,drain")
        if PRE_TRACE then
            pre = probe.csv_open(probe.out_path("pre.csv"),
                "vsync,ctx6,st7,acting,step,c6da,c15,c87c," ..
                "s0clip,s0stg,s0rate,s0cur,s0x,s0z,s0tot,s1clip,s1stg,s1rate,s1cur,s1x,s1z,s1tot," ..
                "s2clip,s2stg,s2rate,s2cur,s2x,s2z,s2tot,s3clip,s3stg,s3rate,s3cur,s3x,s3z,s3hp," ..
                "pitch,yaw,trx,try,trz,fx,fy,fz,c6d0,tween,shot")
        end
        -- LEGAIA_WATCH_W="addr,addr": who stores these bytes (breakpoint core).
        local watch_w = probe.getenv("LEGAIA_WATCH_W", "")
        if watch_w ~= "" then
            local watch_csv = probe.csv_open(probe.out_path("watch.csv"), "vsync,st7,addr,pc,ra,v0,v1,a1")
            for word in string.gmatch(watch_w, "[^,]+") do
                local addr = tonumber(word)
                probe.arm_breakpoint(addr, "Write", 1, "watch", function()
                    local R = PCSX.getRegisters()
                    local n = R.GPR.n
                    local function w(v)
                        v = tonumber(v) or 0
                        return v < 0 and v + 0x100000000 or v
                    end
                    local c = u32(CTX_PTR)
                    watch_csv:row("%d,%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X", probe.cast_elapsed or 0,
                        c >= 0x80000000 and u8(c + 7) or -1, addr, w(R.pc), w(n.ra), w(n.v0), w(n.v1), w(n.a1))
                end)
            end
        end
        if probe.getenv_num("LEGAIA_PRIM_CALLS", 0) ~= 0 then
            -- Each entry of the prim dispatcher FUN_80043390: mesh a0, colour
            -- word a1, level a2, the caller, and the mesh's first group words;
            -- and each cued quad / triangle leaf it reaches (breakpoint core).
            local calls = probe.csv_open(probe.out_path("primcall.csv"), "vsync,fn,a0,a1,a2,ra,grp0,grp1")
            probe.arm_breakpoint(0x80043390, "Exec", 4, "primdisp", function()
                local r = PCSX.getRegisters().GPR.n
                local g = u32(tou32(r.a0) + 0x10)
                calls:row("%d,disp,0x%08X,0x%08X,%d,0x%08X,0x%08X,0x%08X", last_elapsed, tou32(r.a0), tou32(r.a1),
                    tou32(r.a2), tou32(r.ra), ptr_ok(g) and u32(g) or 0, ptr_ok(g) and u32(g + 4) or 0)
            end)
            for _, f in ipairs({ { 0x800453BC, "k18b1" }, { 0x80045584, "k19b1" }, { 0x80045BB4, "k19b3" },
                                  { 0x800457C4, "k18b2" }, { 0x80045988, "k19b2" }, { 0x80044DC8, "k17b1" },
                                  { 0x80044C14, "k16b1" }, { 0x80045194, "k15b1" }, { 0x80044FDC, "k14b1" } }) do
                probe.arm_breakpoint(f[1], "Exec", 4, f[2], function()
                    local r = PCSX.getRegisters().GPR.n
                    calls:row("%d,%s,0x%08X,0,0,0x%08X,0,0", last_elapsed, f[2], tou32(r.a1), tou32(r.ra))
                end)
            end
        end
        if probe.getenv_num("LEGAIA_POSE_OBJECTS", 0) ~= 0 then
            -- The burning-body emitter's loop head (FUN_8004998C,
            -- 0x8004A61C): s3 the decoded actor, s2 its object table (the
            -- pose the draw just decoded, 12 bytes an object), s4 the
            -- stream's part count. One row per emit on the creature (pool
            -- slot 7): its node cursor +0x68, its live pair and every
            -- object's translation.
            local pose = probe.csv_open(probe.out_path("pose.csv"), "vsync,cursor,x,y,z,facing,count,objects")
            probe.arm_breakpoint(0x8004A61C, "Exec", 4, "poseobj", function()
                local r = PCSX.getRegisters().GPR.n
                local a = tou32(r.s3)
                if a ~= actor(7) then return end
                local tbl, n = tou32(r.s2), u8(tou32(r.s4))
                local node = u32(a + 0x22C)
                local objs = {}
                for k = 0, n - 1 do
                    objs[#objs + 1] = string.format("%d:%d:%d", s16(tbl + 12 * k), s16(tbl + 12 * k + 2), s16(tbl + 12 * k + 4))
                end
                pose:row("%d,%d,%d,%d,%d,%d,%d,%s", last_elapsed, ptr_ok(node) and u16(node + 0x68) or -1,
                    s16(a + 0x34), s16(a + 0x36), s16(a + 0x38), s16(a + 0x46), n, table.concat(objs, " "))
            end)
        end
        if probe.getenv_num("LEGAIA_SFX_CALLERS", 0) ~= 0 then
            -- Who fills the cue ring: the producer FUN_80035B50 / its
            -- replace FUN_80035BD0, the battle router FUN_8004FE5C and the
            -- dispatcher FUN_8004FCC8, each with a0 / a1 and ra.
            local calls = probe.csv_open(probe.out_path("sfxcall.csv"), "vsync,fn,a0,a1,ra")
            for _, f in ipairs({ { 0x80035B50, "push" }, { 0x80035BD0, "replace" },
                                  { 0x8004FE5C, "router" }, { 0x8004FCC8, "dispatch" } }) do
                probe.arm_breakpoint(f[1], "Exec", 4, f[2], function()
                    local r = PCSX.getRegisters()
                    calls:row("%d,%s,%d,%d,0x%08X", last_elapsed, f[2], tou32(r.GPR.n.a0),
                        tou32(r.GPR.n.a1), tou32(r.GPR.n.ra))
                end)
            end
            -- Every store into the ring's id half (four i16 slots): an
            -- inline writer no entry above sees. `fn` is `store`, `a0` the
            -- pc of the store, `a1` the value the slot holds after it.
            probe.arm_breakpoint(0x8007B6D8, "Write", 8, "ringw", function()
                local r = PCSX.getRegisters()
                calls:row("%d,store,%d,%d,0x%08X", last_elapsed, tou32(r.pc),
                    -1, tou32(r.GPR.n.ra))
            end)
        end
        if FORCE_ROW ~= "" then
            -- FUN_801DA600 returns the formation row in v0; rewriting its
            -- bytes on the return seats the forced monsters.
            probe.arm_breakpoint(0x801DA600, "Exec", 4, "forcerow", function()
                local r = PCSX.getRegisters()
                local p = tou32(r.GPR.n.v0)
                if not ptr_ok(p) then return end
                local i = 0
                for b in string.gmatch(FORCE_ROW, "[^,]+") do
                    probe.write_u8(p + i, tonumber(b)); i = i + 1
                end
                forced_row = true
                PCSX.log(string.format("[forcerow] row@0x%08X forced %s", p, FORCE_ROW))
            end)
        end
        if MONSTER_PICK then
            -- 0x801DAF50 is the instruction after the AI pick's delay slot
            -- in FUN_801DABA4 (`jal 0x801E9FD4` at 0x801DAF48 with
            -- a0 = ctx[+0x274] - 3), and FUN_801DB124 is called next: the
            -- queue the pick wrote (+0x1DE category, +0x1DF.. stream,
            -- +0x1DD target) is final here and nothing has read it yet.
            probe.arm_breakpoint(0x801DAF50, "Exec", 4, "monsterpick", function()
                if injected then return end
                local cx = ctxp()
                if cx == nil then return end
                local seat = u8(cx + 0x274)
                if seat < 3 or (CASTER_SEAT >= 3 and CASTER_SEAT <= 7 and seat ~= CASTER_SEAT) then
                    return
                end
                local a = actor(seat)
                if a == nil then return end
                local was_cat, was_id = u8(a + 0x1DE), u8(a + 0x1DF)
                if MP_TOPUP > 0 then probe.write_u16(a + 0x150, MP_TOPUP) end
                probe.write_u8(a + 0x1DE, 2)
                probe.write_u8(a + 0x1DF, SPELL)
                probe.write_u8(a + 0x1DD, TARGET_SEAT)
                for i, v in ipairs(CAST_QUEUE) do probe.write_u8(a + 0x1DF + i, v) end
                for _, pv in ipairs(POKE32) do probe.write_u32(pv[1], pv[2]) end
                injected = true
                inject_vsync = last_elapsed
                PCSX.log(string.format(
                    "[inject t%d] monster pick seat=%d was cat=%d id=0x%02X -> spell=0x%02X target=%d",
                    last_elapsed, seat, was_cat, was_id, SPELL, TARGET_SEAT))
            end)
        end
        return {}
    end,

    on_capture = function(c, elapsed)
        probe.cast_elapsed = elapsed
        last_elapsed = elapsed
        local cx = ctxp()
        local cs = actor(CASTER_SEAT == 255 and 0 or CASTER_SEAT)
        probe.pad_release(probe.BTN.CROSS)
        if cx == nil or cs == nil then return end
        if not injected and not PHYSICAL and elapsed < PRESS_UNTIL then
            local sub = elapsed % PRESS_PERIOD
            if sub >= PRESS_PERIOD / 2 and sub < PRESS_PERIOD / 2 + 4 then
                probe.pad_force(probe.BTN.CROSS)
            end
        end
        local in_battle = u8(cx) >= 1 and u8(cx) <= 3 and u8(cx + 1) >= 1 and u8(cx + 1) <= 4
            and (FORCE_ROW == "" or forced_row)
        if in_battle and probe.cast_cfg.party_spd > 0 and not injected then
            for sx = 0, 2 do
                local a = actor(sx)
                if a ~= nil and u16(a + 0x14E) > 0 then probe.write_u16(a + 0x164, probe.cast_cfg.party_spd) end
            end
        end
        local st7 = u8(cx + 7)
        local acting = u8(cx + 0x13)
        local flow = u8(cx + 6) * 256 + st7
        if flow ~= last_flow and not injected then
            PCSX.log(string.format("[flow t%d] ctx6=0x%02X ctx7=0x%02X acting=%d",
                elapsed, u8(cx + 6), st7, acting))
            last_flow = flow
        end
        if PHYSICAL and not injected and in_battle then
            injected = true
            inject_vsync = elapsed
        end
        if not injected and not MONSTER_PICK and in_battle and st7 == INJECT_STATE
            and (CASTER_SEAT == 255 or CASTER_SEAT >= 3 or acting == CASTER_SEAT) then
            injected = true
            inject_vsync = elapsed
            local seats = {}
            if CASTER_SEAT == 255 then seats = { 0, 1, 2 } else seats = { CASTER_SEAT } end
            for _, s in ipairs(seats) do
                local a = actor(s)
                if a ~= nil then
                    if MP_TOPUP > 0 then probe.write_u16(a + 0x150, MP_TOPUP) end
                    probe.write_u8(a + 0x1DE, 2)
                    probe.write_u8(a + 0x1DF, SPELL)
                    probe.write_u8(a + 0x1DD, TARGET_SEAT)
                end
            end
            for _, pv in ipairs(POKE32) do probe.write_u32(pv[1], pv[2]) end
            if probe.cast_cfg.learn_level > 0 then
                for _, s in ipairs(seats) do
                    local rec = 0x80084708 + s * 0x414
                    probe.write_u8(rec + 0x13D, SPELL)
                    probe.write_u8(rec + 0x161, probe.cast_cfg.learn_level)
                end
            end
            PCSX.log(string.format("[inject t%d] spell=0x%02X target=%d acting=%d",
                elapsed, SPELL, TARGET_SEAT, acting))
        end
        if not injected and in_battle then pre_trace_row(cx, st7, acting, elapsed) end
        if not injected then return end
        if SAVE_INJECT and not inject_saved then
            inject_saved = true
            pcall(function()
                local w = PCSX.createSaveState()
                local sh = Support.File.open(probe.out_path("state_inject.rawsstate"), "TRUNCATE")
                sh:writeMoveSlice(w); sh:close()
            end)
            PCSX.log(string.format("[inject t%d] state written: state_inject.rawsstate", elapsed))
        end
        -- Cast begin (0x28) is the state every Magic action passes through
        -- first - a party summon, a monster spell, a capture-class cast - and
        -- the engine recorder starts on the same one. A physical recording
        -- starts on the state's own instant, a heal's Spirit / Item flow
        -- 0x3C..0x40 among them (the harvester saves heals at 0x3E); without
        -- those the recording waited for the next actor's strike.
        if not cast_seen and ((PHYSICAL and (st7 == 0x1E or st7 == 0x1F or st7 == 0x20 or st7 == 0x2B or st7 == 0x70 or st7 == 0x6E or (st7 >= 0x3C and st7 <= 0x40))) or (not PHYSICAL and (st7 == 0x28 or st7 == 0x3C)
            and ((CASTER_SEAT == 255 and acting < 3) or acting == CASTER_SEAT))) then
            cast_seen = true
            cast_vsync = elapsed
            ground_at_cast = ground_pairs()
            guard_at_cast = guard_seats()
            caster_at_cast = acting
            target_at_cast = u8(actor(acting) + 0x1DD)
            PCSX.log(string.format("[cast t%d] caster=%d ground=%s", elapsed, acting, ground_at_cast))
            if probe.cast_cfg.monster_hp > 0 then
                for s = 3, 7 do
                    local a = actor(s)
                    if a ~= nil and u16(a + 0x14C) > 0 then
                        probe.write_u16(a + 0x14C, probe.cast_cfg.monster_hp)
                        probe.write_u16(a + 0x172, probe.cast_cfg.monster_hp)
                    end
                end
            end
            PCSX.log(string.format("[cast t%d] cast begin loader=%d", elapsed, u8(LOADER_ID)))
        end
        -- 0x65: the Spikefish flute's flee leaves the summon band for the run
        -- band's wait instead (PROT 0925, arm 8) and the fight ends there.
        if cast_seen and not done_seen and (st7 == 0x50 or st7 == 0x51 or st7 == 0x5A or st7 == 0x65) then
            done_seen = true
            done_vsync = elapsed
            PCSX.log(string.format("[done t%d] st7=0x%02X", elapsed, st7))
        end

        local shot, w, h, bpp = "", 0, 0, 0
        if cast_seen and (elapsed - cast_vsync) % SHOT_EVERY == 0 and shots < MAX_SHOTS then
            shot, w, h, bpp = take_shot(elapsed)
        end
        if cast_seen and RAM_AT[elapsed - cast_vsync] then
            local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
            if buf ~= nil then
                local fh = io.open(probe.out_path(string.format("ram_%05d.bin", elapsed)), "wb")
                if fh ~= nil then fh:write(tostring(buf)); fh:close() end
            end
            pcall(function()
                local w = PCSX.createSaveState()
                local sh = Support.File.open(probe.out_path(string.format("state_%05d.rawsstate", elapsed)), "TRUNCATE")
                sh:writeMoveSlice(w); sh:close()
            end)
        end
        local hp = {}
        for s = 0, 7 do
            local a = actor(s)
            hp[#hp + 1] = a and u16(a + 0x14C) or -1
        end
        meta:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            elapsed, st7, u8(cx + 0x279), acting, probe.read_scratch_u8(0x1F800393),
            u8(cx + 6), u16(cx + 0x6D8), u16(cx + 0x6DA), u32(cx + 0x87C), s32(GIMARD_CD),
            u8(LOADER_ID), hp[1], hp[2], hp[3], hp[4], hp[5], hp[6], hp[7], hp[8],
            shot, w, h, bpp, u32(FRAME_COUNTER), u8(cx + 0x276),
            s32(XA_SPAN), s16(CD_BUSY), u16(CD_MODE), s16(CD_READ), u32(RAND_SEED), probe.cast_fade_back(),
            probe.frame_cost_newest())
        if cast_seen then probe.cast_seats:row(elapsed, actor) end
        probe.cast_parts_row(cast_seen, elapsed, st7, cx)
        -- creature.csv: pool slot 7's clip, staged byte, cursor and stand, the
        -- engine recorder's file of the same name.
        if cast_seen then
            local a = actor(7)
            if a ~= nil then
                local node = u32(a + 0x22C)
                local function sh7(off)
                    local v = u16(a + off)
                    if v >= 0x8000 then v = v - 0x10000 end
                    return v
                end
                creature:row("%d,%d,%d,%d,%d,%d,%d,%d", elapsed, u8(a + 0x1D9), u8(a + 0x1DA),
                    ptr_ok(node) and u16(node + 0x68) or -1, sh7(0x34), sh7(0x36), sh7(0x38), sh7(0x46))
            end
        end
        -- The cue ring's ready edges (see sfx.csv in the header).
        for i = 0, 3 do
            local id = s16(probe.cast_sfx.ring + i * 2)
            local ready = id >= 0 and u32(probe.cast_sfx.timers + i * 4) == 0
            if ready and (not probe.cast_sfx.ready[i + 1] or probe.cast_sfx.prev[i + 1] ~= id) then
                probe.cast_sfx.csv:row("%d,%d,%d,%d,%d,%d,%d", elapsed, u32(FRAME_COUNTER), st7, u8(cx + 0x279),
                    i, id, u32(0x8007BABC) == u32(0x8007BAA0) and 1 or 0)
            end
            probe.cast_sfx.ready[i + 1], probe.cast_sfx.prev[i + 1] = ready, id
        end
        local function sh(a)
            local v = u16(a)
            if v >= 0x8000 then v = v - 0x10000 end
            return v
        end
        cam:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            elapsed, st7, u8(cx + 0x279), probe.read_scratch_u8(0x1F800393),
            u16(0x8007B790) % 4096, u16(0x8007B792) % 4096,
            s32(0x800840B8), s32(0x800840BC), s32(0x800840C0),
            sh(cx + 0x118C + 1 * 4), sh(cx + 0x118C + 1 * 4 + 2),
            sh(cx + 0x118C + 5 * 4), sh(cx + 0x118C + 5 * 4 + 2))
        local ba = actor(math.min(acting, 7))
        if ba ~= nil then
            local node = u32(ba + 0x22C)
            local function s16(a)
                local v = u16(a)
                if v >= 0x8000 then v = v - 0x10000 end
                return v
            end
            body:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
                elapsed, st7, acting, u8(ba + 0x1D9), ptr_ok(node) and u16(node + 0x68) or -1,
                s16(ba + 0x34), s16(ba + 0x38), s16(ba + 0x3C), s16(ba + 0x40),
                -s32(0x80089118), -s32(0x80089120),
                -s16(cx + 0x118C + 6 * 4 + 2), -s16(cx + 0x118C + 8 * 4 + 2), u32(cx + 0x87C))
        end
        -- The module phase counts as movement: a slot-B module runs every
        -- arm inside one band state.
        local park_key = st7 * 256 + u8(cx + 0x279)
        if park_key ~= park_st7 then park_st7, park_since = park_key, elapsed end
        if not done_seen and elapsed - park_since >= PARK then
            parked = true
            PCSX.log(string.format("[park t%d] action state 0x%02X phase %d unchanged for %d vsyncs",
                elapsed, st7, u8(cx + 0x279), PARK))
            c.request_quit = true
        end
        if done_seen and elapsed >= done_vsync + TAIL then c.request_quit = true end
    end,

    on_done = function()
        PCSX.log(string.format("[cast-seq] inject=%d cast=%d done=%d shots=%d",
            inject_vsync, cast_vsync, done_vsync, shots))
        if meta then meta:close() end
        if body then body:close() end
        if cam then cam:close() end
        if probe.cast_sfx.csv then probe.cast_sfx.csv:close() end
        if probe.cast_parts then probe.cast_parts:close() end
        if probe.cast_seats.csv then probe.cast_seats.csv:close() end
        local fh = io.open(probe.out_path("done.txt"), "w")
        if fh then
            fh:write(string.format("inject=%d\ncast=%d\ndone=%d\nshots=%d\ncaster=%d\nspell=%d\ntarget=%d\ncast_target=%d\nground=%s\nguard=%s\nparked=%d\n",
                inject_vsync, cast_vsync, done_vsync, shots, caster_at_cast, SPELL, TARGET_SEAT, target_at_cast, ground_at_cast,
                guard_at_cast,
                parked and math.floor(park_st7 / 256) or -1))
            fh:close()
        end
        PCSX.quit(0)
    end,
})
