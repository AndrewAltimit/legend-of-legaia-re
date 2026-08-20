-- autorun_special_cast_stall.lua
--
-- Follow-up to autorun_player_special_cast.lua: same forced Megaton Press
-- cast from Vahn's slot, plus
--   * a Read breakpoint on the wild address 0x08044880 seen at the phase
--     9->10 boundary (logs pc/ra to attribute the reader);
--   * per-frame dump, once the module phase passes 8, of the victim's
--     reaction fields (+0x1F1/+0x1F2), staged/current anim (+0x1DA/+0x1D9),
--     hide markers (+0x21C, +0x4), and the module scratch counters.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1600)
local SPELL = probe.getenv_num("LEGAIA_SPELL", 0x7A)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local WILD = 0x08044880

local ACTOR_TABLE = 0x801C9370
local CTX_PTR = 0x8007BD24

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end

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

local injected, cast_seen = false, false
local wild_hits = 0
local last = ""

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log("== special cast stall probe ==")
        probe.arm_breakpoint(WILD, "Read", 4, "wild", function()
            local r = PCSX.getRegisters()
            local pc = tou32(r.pc)
            local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
            wild_hits = wild_hits + 1
            if wild_hits <= 5 then
                PCSX.log(string.format("[WILD read] pc=0x%08X ra=0x%08X", pc, ra))
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

        if cast_seen then
            local ph = u8(cx + 0x279)
            local v = actor(TARGET_SEAT)
            if ph >= 8 and v then
                local line = string.format(
                    "ph=%d st7=0x%02X cstr(1d9/1da)=%02X/%02X vic(1d9/1da)=%02X/%02X vic(1f1/1f2)=%02X/%02X vic(21c,4)=%02X,%08X vhp=%d cnt=%08X scr(69,7f)=%02X,%02X wild=%d",
                    ph, st7, u8(a0+0x1D9), u8(a0+0x1DA), u8(v+0x1D9), u8(v+0x1DA),
                    u8(v+0x1F1), u8(v+0x1F2), u8(v+0x21C), u32(v+4), u16(v+0x14C),
                    u32(0x801F9624), u8(0x1F800314+0x69), u8(0x1F800314+0x7F), wild_hits)
                if line ~= last then
                    PCSX.log(string.format("[t%d] %s", elapsed, line))
                    last = line
                end
            end
            if st7 == 0x50 or st7 == 0x51 then
                PCSX.log(string.format("[DONE t%d] st7=0x%02X", elapsed, st7))
            end
        end
    end,
})
