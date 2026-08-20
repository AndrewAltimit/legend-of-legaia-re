-- autorun_special_cast_alias_row.lua (derived)
--
-- Pinpoint the renderer hang at the Megaton Press phase-0xA boundary on a
-- player cast: breakpoint the wild read (0x08044880) and, at the hit, dump
-- every battle actor's render fields (+0x4 header, +0x44 mesh chain, +0x56
-- render mode) and every seated part-actor in the 0x60-slot effect pool at
-- DAT_801C90F0, to attribute the garbage chain. Also logs the party slot-0
-- anim pointer table entries 0x0A/0x0B while the module runs.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1700)
local SPELL = probe.getenv_num("LEGAIA_SPELL", 0x7A)
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
        local a0 = actor(0)
        if cx == nil or a0 == nil then return end
        local st7 = u8(cx + 7)
        local cat = u8(a0 + 0x1DE)
        if elapsed == 10 then
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
