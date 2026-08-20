-- autorun_special_cast_chain_dump.lua
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
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 0)
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
            PCSX.log(string.format("[%s] actor%d @%08X hdr=%08X chain=%08X mode=%02X anim=%02X/%02X",
                tag, slot, p, u32(p + 4), u32(p + 0x44), u8(p + 0x56), u8(p + 0x1D9), u8(p + 0x1DA)))
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
local injected, cast_seen, dumped = false, false, false
local last = ""

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log("== ENEMY-CASTER control probe ==")
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
        local a0 = actor(3)
        if cx == nil or a0 == nil then return end
        local st7 = u8(cx + 7)
        local st6 = u8(cx + 6)
        local cat = u8(a0 + 0x1DE)
        -- Settled-frame conversion: the monster's rolled action sits on the
        -- actor while the Begin/Reselect confirm is up; convert it before
        -- Begin is pressed (t=20; presses start t=30).
        if elapsed == 20 and cat == 3 and probe.getenv("LEGAIA_NO_POKE", "") == "" then
            probe.write_u8(a0 + 0x1DE, 2)
            probe.write_u8(a0 + 0x1DF, SPELL)
            probe.write_u8(a0 + 0x1DD, TARGET_SEAT)
            PCSX.log("[poke t20] monster action converted to cast")
        end

        probe.pad_release(probe.BTN.CROSS)
        if not cast_seen and elapsed < 300 and probe.getenv("LEGAIA_NO_POKE", "") == "" then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end
        if st6 == 0x70 or st7 == 0x70 then cast_seen = true end

        do
            local ph0 = u8(cx + 0x279)
            local line6 = string.format("st6=%02X st7=%02X ph=%d slot=%d mcat=%02X mid=%02X manim=%02X/%02X", st6, st7, ph0, u8(cx+0x13), u8(a0+0x1DE), u8(a0+0x1DF), u8(a0+0x1D9), u8(a0+0x1DA))
            if line6 ~= last then
                PCSX.log(string.format("[t%d] %s", elapsed, line6))
                last = line6
            end
        end
        if cast_seen then
            local ph = u8(cx + 0x279)
            finale_trace(cx, elapsed, ph)
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
