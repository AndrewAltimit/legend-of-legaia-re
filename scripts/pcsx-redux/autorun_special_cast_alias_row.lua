-- autorun_special_cast_alias_row.lua
--
-- Player-cast experiment rig for a capture-class cast: force the chosen
-- party slot's action to the cast, optionally alias the caster's action
-- table row 0x0B onto row 0x0A's record (ptr[0xB] <- ptr[0xA], the stage
-- boundary the retail Block record chokes on), optionally inject RAM
-- (a blob at a VA, or word pokes) before the cast fires, and trace the
-- phase walk + finale.
--
-- Env: LEGAIA_SSTATE (required), LEGAIA_FRAMES, LEGAIA_CAST_SLOT
--      (party slot, default 0), LEGAIA_NO_ALIAS=1 to skip the row alias,
--      LEGAIA_INJECT_BLOB=path + LEGAIA_INJECT_VA=addr for a blob,
--      LEGAIA_POKE_FILE / LEGAIA_POKE_WORDS for word pokes.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1700)
local SPELL = probe.getenv_num("LEGAIA_SPELL", 0x7A)
local CAST_SLOT = probe.getenv_num("LEGAIA_CAST_SLOT", 0)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local WILD = 0x08044880

local ACTOR_TABLE = 0x801C9370
local BANK_TABLE = 0x801C9360
local POOL = 0x801C90F0
local CTX_PTR = 0x8007BD24

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local function ctx()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

local function dump_world(tag)
    for slot = 0, 7 do
        local p = u32(ACTOR_TABLE + slot * 4)
        if p >= 0x80000000 and p < 0x80200000 then
            PCSX.log(string.format("[%s] actor%d @%08X hdr=%08X att24=%08X chain=%08X c74=%08X mode=%02X anim=%02X/%02X",
                tag, slot, p, u32(p + 4), u32(p + 0x24), u32(p + 0x44), u32(p + 0x74), u8(p + 0x56), u8(p + 0x1D9), u8(p + 0x1DA)))
        end
    end
    local seated = 0
    for i = 0, 0x5F do
        local p = u32(POOL + i * 4)
        if p ~= 0 then
            seated = seated + 1
            if seated <= 12 then
                PCSX.log(string.format("[%s] part%02d @%08X hdr=%08X chain=%08X mode=%02X flags=%08X",
                    tag, i, p, u32(p + 4), u32(p + 0x44), u8(p + 0x56), u32(p + 0x10)))
            end
        end
    end
    PCSX.log(string.format("[%s] parts seated=%d", tag, seated))
end

local function dump_obj(tag)
    local obj = 0x8007F84C
    for row = 0, 15 do
        local b = obj + row * 0x10
        PCSX.log(string.format("[%s] obj+%02X: %08X %08X %08X %08X",
            tag, row * 0x10, u32(b), u32(b + 4), u32(b + 8), u32(b + 0xC)))
    end
end


local function finale_trace(cx, elapsed, ph)
    if ph < 14 or ph > 250 or elapsed % 4 ~= 0 then return end
    local ent = u32(cx + 0x102C)
    local e10, e44 = 0, 0
    if ent >= 0x80000000 and ent < 0x80200000 then
        e10, e44 = u32(ent + 0x10), u32(ent + 0x44)
    end
    if elapsed % 8 == 0 and (_G.__fin_shots or 0) < 90 then
        _G.__fin_shots = (_G.__fin_shots or 0) + 1
        local ok, ss = pcall(PCSX.GPU.takeScreenShot)
        if ok and ss ~= nil then
            local fh = io.open(probe.out_path(string.format("fin_%04d_ph%d.raw", elapsed, ph)), "wb")
            if fh then
                fh:write(tostring(ss.data))
                fh:close()
            end
        end
    end
    PCSX.log(string.format("[FIN t%d] ph=%d ent=%08X e10=%08X e44=%08X cam=%d,%d,%d",
        elapsed, ph, ent, e10, e44,
        (u32(0x1F800314 + 0x14) + 0x80000000) % 0x100000000 - 0x80000000,
        (u32(0x1F800314 + 0x18) + 0x80000000) % 0x100000000 - 0x80000000,
        (u32(0x1F800314 + 0x1C) + 0x80000000) % 0x100000000 - 0x80000000))
end
local colour_bp_armed = false
local colour_logged = 0
local function arm_colour_bp()
    local first_hit = false
    probe.arm_breakpoint(0x80043390, "Exec", 4, "tmd_entry", function()
        if not first_hit then
            first_hit = true
            PCSX.log("[tmd_entry BP live]")
        end
        local r = PCSX.getRegisters()
        local n = r.GPR and r.GPR.n or {}
        local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
        local a0v = tou32(n.a0)
        if a0v == 0x8007F84C and colour_logged < 8 then
            colour_logged = colour_logged + 1
            local s0 = tou32(n.s0)
            local v0 = tou32(n.v0)
            PCSX.log(string.format("[BADCALL] ra=%08X s0=%08X a0=%08X a1=%08X v0=%08X s5=%08X s3=%08X",
                tou32(n.ra), s0, a0v, tou32(n.a1), v0, tou32(n.s5), tou32(n.s3)))
            for row = 0, 8 do
                local b = s0 + row * 0x10
                PCSX.log(string.format("[BADCALL] s0+%02X: %08X %08X %08X %08X",
                    row * 0x10, u32(b), u32(b + 4), u32(b + 8), u32(b + 0xC)))
            end
            local t44 = u32(s0 + 0x44)
            for k = 0, 7 do
                PCSX.log(string.format("[BADCALL] tbl44[%d] @%08X = %08X %08X",
                    k, t44 + k * 8, u32(t44 + k * 8), u32(t44 + k * 8 + 4)))
            end
        end
    end)
end

local safe_dumped = false
local injected, cast_seen, dumped = false, false, false
local last = ""

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log("== alias-row cast probe: ptr[0xB] <- ptr[0xA] ==")
        probe.arm_breakpoint(0x02000008, "Read", 4, "wild2", function()
            if not dumped then
                dumped = true
                local r = PCSX.getRegisters()
                local n = r.GPR and r.GPR.n or {}
                local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
                PCSX.log(string.format("[WILD2] pc=%08X ra=%08X a0=%08X a1=%08X a2=%08X s0=%08X s1=%08X s2=%08X s3=%08X s4=%08X v0=%08X v1=%08X t0=%08X",
                    tou32(r.pc), tou32(n.ra), tou32(n.a0), tou32(n.a1), tou32(n.a2), tou32(n.s0), tou32(n.s1),
                    tou32(n.s2), tou32(n.s3), tou32(n.s4), tou32(n.v0), tou32(n.v1), tou32(n.t0)))
                dump_world("WILD2")
                dump_obj("WILD2")
            end
        end)
        probe.arm_breakpoint(WILD, "Read", 4, "wild", function()
            if not dumped then
                dumped = true
                local r = PCSX.getRegisters()
                local n = r.GPR and r.GPR.n or {}
                local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
                PCSX.log(string.format("[WILD] pc=%08X ra=%08X a0=%08X a1=%08X s0=%08X s1=%08X v0=%08X v1=%08X",
                    tou32(r.pc), tou32(n.ra), tou32(n.a0), tou32(n.a1), tou32(n.s0), tou32(n.s1),
                    tou32(n.v0), tou32(n.v1)))
                dump_world("WILD")
            end
        end)
        return {}
    end,
    on_capture = function(c, elapsed)
        local cx = ctx()
        local a0 = actor(CAST_SLOT)
        if cx == nil or a0 == nil then return end
        local st7 = u8(cx + 7)
        local cat = u8(a0 + 0x1DE)
        if elapsed == 4 and not _G.__injected_blob then
            _G.__injected_blob = true
            local bf = probe.getenv("LEGAIA_INJECT_BLOB", "")
            local bva = tonumber(probe.getenv("LEGAIA_INJECT_VA", "0")) or 0
            if bf ~= "" and bva ~= 0 then
                local fh = io.open(bf, "rb")
                if fh then
                    local blob = fh:read("*a")
                    fh:close()
                    for i = 1, #blob do
                        probe.write_u8(bva + i - 1, string.byte(blob, i))
                    end
                    PCSX.log(string.format("[inject t%d] blob %d bytes @%08X", elapsed, #blob, bva))
                else
                    PCSX.log("[inject] FAILED to open " .. bf)
                end
            end
            local pf = probe.getenv("LEGAIA_POKE_FILE", "")
            if pf ~= "" then
                local ok, list = pcall(dofile, pf)
                if ok and type(list) == "table" then
                    for _, e in ipairs(list) do
                        local van, wn = e[1], e[2]
                        probe.write_u8(van + 0, wn % 0x100)
                        probe.write_u8(van + 1, math.floor(wn / 0x100) % 0x100)
                        probe.write_u8(van + 2, math.floor(wn / 0x10000) % 0x100)
                        probe.write_u8(van + 3, math.floor(wn / 0x1000000) % 0x100)
                    end
                    PCSX.log(string.format("[inject] poke file: %d words", #list))
                else
                    PCSX.log("[inject] poke file FAILED: " .. tostring(list))
                end
            end
            local pokes = probe.getenv("LEGAIA_POKE_WORDS", "")
            for va, w in string.gmatch(pokes, "(%w+):(%w+)") do
                local van, wn = tonumber(va, 16), tonumber(w, 16)
                if van and wn then
                    probe.write_u8(van + 0, wn % 0x100)
                    probe.write_u8(van + 1, math.floor(wn / 0x100) % 0x100)
                    probe.write_u8(van + 2, math.floor(wn / 0x10000) % 0x100)
                    probe.write_u8(van + 3, math.floor(wn / 0x1000000) % 0x100)
                    PCSX.log(string.format("[inject] word %08X @%08X", wn, van))
                end
            end
        end
        if elapsed == 10 and probe.getenv("LEGAIA_NO_ALIAS", "") == "" then
            local bank = u32(BANK_TABLE)
            if bank >= 0x80000000 and bank < 0x80200000 then
                local good = u32(bank + 0x0A * 4)
                local old = u32(bank + 0x0B * 4)
                probe.write_u8(bank + 0x0B * 4 + 0, good % 0x100)
                probe.write_u8(bank + 0x0B * 4 + 1, math.floor(good / 0x100) % 0x100)
                probe.write_u8(bank + 0x0B * 4 + 2, math.floor(good / 0x10000) % 0x100)
                probe.write_u8(bank + 0x0B * 4 + 3, math.floor(good / 0x1000000) % 0x100)
                PCSX.log(string.format("[alias t10] ptr[0xB] %08X -> %08X", old, good))
            end
        end

        probe.pad_release(probe.BTN.CROSS)
        if not cast_seen and elapsed < 300 then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end
        if cat == 3 and not cast_seen then
            probe.write_u8(a0 + 0x1DE, 2)
            probe.write_u8(a0 + 0x1DF, SPELL)
            probe.write_u8(a0 + 0x1DD, TARGET_SEAT)
            injected = true
        end
        if injected and st7 == 0x70 then cast_seen = true end

        if injected then
            local a3 = actor(TARGET_SEAT)
            local always = string.format("st7=%02X ph=%d gm=%02X hp0=%d hp3=%d canim=%02X/%02X",
                st7, u8(cx + 0x279), u8(0x8007B83C), u16(a0 + 0x14C),
                a3 and u16(a3 + 0x14C) or 0xFFFF, u8(a0 + 0x1D9), u8(a0 + 0x1DA))
            if always ~= last then
                PCSX.log(string.format("[t%d] %s", elapsed, always))
                last = always
            end
        end

        if cast_seen then
            local ph = u8(cx + 0x279)
            finale_trace(cx, elapsed, ph)
            if ph >= 16 and ph < 250 and not colour_bp_armed then
                colour_bp_armed = true
                arm_colour_bp()
                PCSX.log(string.format("[t%d] colour BP armed at ph=%d", elapsed, ph))
            end
            if ph >= 15 and ph < 250 and not safe_dumped then
                safe_dumped = true
                dump_obj(string.format("safe-ph%d t%d", ph, elapsed))
            end
            if ph >= 9 then
                local bank = u32(BANK_TABLE)
                local line = string.format("ph=%d ptrA=%08X ptrB=%08X cnt=%08X",
                    ph, u32(bank + 0x0A * 4), u32(bank + 0x0B * 4), u32(0x801F9624))
                if line ~= last then
                    PCSX.log(string.format("[t%d] %s", elapsed, line))
                    last = line
                end
                if ph == 10 and u32(0x801F9624) < 0x60 and not dumped then
                    dump_world(string.format("pre-boundary t%d", elapsed))
                end
            end
        end
    end,
})
