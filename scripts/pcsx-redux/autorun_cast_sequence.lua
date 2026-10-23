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
--                `xaspan` / `cdbusy` / `cdmode` / `cdread`. `shot` names the frame
--                taken on that vsync, if any.
--   r_<vsync>.raw  PCSX.GPU.takeScreenShot() pixels (BGR555, or BGR888 when
--                `bpp` says 24); width / height / bpp are on the meta row.
--   manifest.txt the run's knobs.
-- These are retail pixels and retail RAM: never commit them.
--
-- Env:
--   LEGAIA_SPELL         action id to force (required; e.g. 0x86)
--   LEGAIA_CASTER_SEAT   seat whose queued action is rewritten (0); 255
--                        writes every party seat. A monster seat (3..7) is
--                        written on the first 0x0A of any seat: state 0x0C
--                        re-picks the actor, so 0x0A's seat is not the one
--                        that acts
--   LEGAIA_TARGET_SEAT   actor-table seat to target (3)
--   LEGAIA_INJECT_STATE  action state to inject on (0x0A)
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
--   LEGAIA_SHOT_EVERY    vsyncs between frames (2)
--   LEGAIA_MAX_SHOTS     frame cap (1500)
--   LEGAIA_TAIL          vsyncs to keep going past the Done band (60)
--   LEGAIA_RAM_AT        comma list of vsyncs past the cast begin at which the
--                        full 2 MiB main RAM is written as ram_<vsync>.bin
--                        (the absolute vsync, as in meta.csv), and the whole
--                        emulator state beside it as state_<vsync>.rawsstate
--                        (VRAM included: sstate_vram.py); empty = none
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
local PRESS_UNTIL  = probe.getenv_num("LEGAIA_PRESS_UNTIL", 600)
local PRESS_PERIOD = probe.getenv_num("LEGAIA_PRESS_PERIOD", 60)
local PARTY_SPD    = probe.getenv_num("LEGAIA_PARTY_SPD", 0)
local MP_TOPUP     = probe.getenv_num("LEGAIA_MP_TOPUP", 999)
local POKE32 = {}
for kv in string.gmatch(probe.getenv("LEGAIA_POKE32", ""), "[^,]+") do
    local a, v = string.match(kv, "^%s*([^=]+)=(.+)$")
    if a ~= nil then POKE32[#POKE32 + 1] = { tonumber(a), tonumber(v) } end
end
local SHOT_EVERY   = probe.getenv_num("LEGAIA_SHOT_EVERY", 2)
local MAX_SHOTS    = probe.getenv_num("LEGAIA_MAX_SHOTS", 1500)
local TAIL         = probe.getenv_num("LEGAIA_TAIL", 60)
local PARK         = probe.getenv_num("LEGAIA_PARK", 900)
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

local meta, body, cam
local injected, cast_seen, done_seen = false, false, false
local inject_vsync, cast_vsync, done_vsync = -1, -1, -1
local forced_row = false
local shots = 0
local last_flow = -1
local park_st7, park_since = -1, 0
local parked = false
local ground_at_cast = ""
local caster_at_cast = -1

-- Each pool slot's live ground pair +0x34 / +0x38 (`-` for an empty slot):
-- the engine's inflight seed seats every combatant there at its dispatch, so
-- an action retail ran before the cast (a monster that struck first and was
-- not walked home) leaves the same stage.
local function ground_pairs()
    local t = {}
    for s = 0, 7 do
        local a = actor(s)
        if a ~= nil and u16(a + 0x14E) > 0 then
            local x, z = u16(a + 0x34), u16(a + 0x38)
            if x >= 0x8000 then x = x - 0x10000 end
            if z >= 0x8000 then z = z - 0x10000 end
            t[#t + 1] = string.format("%d:%d", x, z)
        else
            t[#t + 1] = "-"
        end
    end
    return table.concat(t, ",")
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

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== cast sequence == spell=0x%02X caster=%d target=%d every=%d",
            SPELL, CASTER_SEAT, TARGET_SEAT, SHOT_EVERY))
        probe.env.write_manifest("autorun_cast_sequence.lua", {
            sstate = SSTATE_PATH, spell = string.format("0x%02X", SPELL),
            caster_seat = CASTER_SEAT, target_seat = TARGET_SEAT,
            inject_state = INJECT_STATE, force_row = FORCE_ROW,
            party_spd = PARTY_SPD, shot_every = SHOT_EVERY, tail = TAIL,
        })
        meta = probe.csv_open(probe.out_path("meta.csv"),
            "vsync,st7,phase,acting,step,flow,c6d8,c6da,c87c,cd7960,loader," ..
            "hp0,hp1,hp2,hp3,hp4,hp5,hp6,hp7,shot,w,h,bpp,frame,c276,xaspan,cdbusy,cdmode,cdread")
        -- The acting seat's body against the camera that frames it: clip,
        -- node cursor, live pair +0x34/+0x38, body pair +0x3C/+0x40, and the
        -- camera focus (stored negated) with its tween endpoints.
        body = probe.csv_open(probe.out_path("body.csv"),
            "vsync,st7,acting,clip,cursor,lx,lz,bx,bz,fx,fz,ex,ez,c87c")
        -- The live camera pose (pitch / yaw `0x8007B790/92`, TR
        -- `0x800840B8..C0`) with the yaw and TR z records of the tween
        -- table `ctx[+0x118C]` (`{step, endpoint}`): a module shot's walk.
        cam = probe.csv_open(probe.out_path("cam.csv"),
            "vsync,st7,phase,step,pitch,yaw,trx,try,trz,yaw_inc,yaw_end,trz_inc,trz_end")
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
        return {}
    end,

    on_capture = function(c, elapsed)
        local cx = ctxp()
        local cs = actor(CASTER_SEAT == 255 and 0 or CASTER_SEAT)
        probe.pad_release(probe.BTN.CROSS)
        if cx == nil or cs == nil then return end
        if not injected and elapsed < PRESS_UNTIL then
            local sub = elapsed % PRESS_PERIOD
            if sub >= PRESS_PERIOD / 2 and sub < PRESS_PERIOD / 2 + 4 then
                probe.pad_force(probe.BTN.CROSS)
            end
        end
        local in_battle = u8(cx) >= 1 and u8(cx) <= 3 and u8(cx + 1) >= 1 and u8(cx + 1) <= 4
            and (FORCE_ROW == "" or forced_row)
        if in_battle and PARTY_SPD > 0 and not injected then
            for sx = 0, 2 do
                local a = actor(sx)
                if a ~= nil and u16(a + 0x14E) > 0 then probe.write_u16(a + 0x164, PARTY_SPD) end
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
        if not injected and in_battle and st7 == INJECT_STATE
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
            PCSX.log(string.format("[inject t%d] spell=0x%02X target=%d acting=%d",
                elapsed, SPELL, TARGET_SEAT, acting))
        end
        if not injected then return end
        -- Cast begin (0x28) is the state every Magic action passes through
        -- first - a party summon, a monster spell, a capture-class cast - and
        -- the engine recorder starts on the same one.
        if not cast_seen and st7 == 0x28
            and ((CASTER_SEAT == 255 and acting < 3) or acting == CASTER_SEAT) then
            cast_seen = true
            cast_vsync = elapsed
            ground_at_cast = ground_pairs()
            caster_at_cast = acting
            PCSX.log(string.format("[cast t%d] caster=%d ground=%s", elapsed, acting, ground_at_cast))
            PCSX.log(string.format("[cast t%d] cast begin loader=%d", elapsed, u8(LOADER_ID)))
        end
        if cast_seen and not done_seen and (st7 == 0x50 or st7 == 0x51 or st7 == 0x5A) then
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
        meta:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            elapsed, st7, u8(cx + 0x279), acting, probe.read_scratch_u8(0x1F800393),
            u8(cx + 6), u16(cx + 0x6D8), u16(cx + 0x6DA), u32(cx + 0x87C), s32(GIMARD_CD),
            u8(LOADER_ID), hp[1], hp[2], hp[3], hp[4], hp[5], hp[6], hp[7], hp[8],
            shot, w, h, bpp, u32(FRAME_COUNTER), u8(cx + 0x276),
            s32(XA_SPAN), s16(CD_BUSY), u16(CD_MODE), s16(CD_READ))
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
        local fh = io.open(probe.out_path("done.txt"), "w")
        if fh then
            fh:write(string.format("inject=%d\ncast=%d\ndone=%d\nshots=%d\ncaster=%d\nspell=%d\ntarget=%d\nground=%s\nparked=%d\n",
                inject_vsync, cast_vsync, done_vsync, shots, caster_at_cast, SPELL, TARGET_SEAT, ground_at_cast,
                parked and math.floor(park_st7 / 256) or -1))
            fh:close()
        end
        PCSX.quit(0)
    end,
})
