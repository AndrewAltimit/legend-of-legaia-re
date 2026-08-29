-- autorun_special_cast_row_poke.lua
--
-- Discriminator for the entry-0x0B hang: same forced Megaton Press cast
-- from Vahn's slot as autorun_special_cast_stall.lua, but BEFORE the cast,
-- Vahn's resident action-bank record 11 (the Miracle row - never staged on
-- a party actor in retail; the module's stepper stages it as Megaton's
-- second stage) is overwritten with a byte copy of record 10 (proven to
-- stage cleanly as stage one). If the boundary hang is a half-configured
-- record, this run sails past it; if the hang keys on the entry INDEX,
-- it reproduces regardless.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3000)
local SPELL = probe.getenv_num("LEGAIA_SPELL", 0x7A)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local SRC_ROW = probe.getenv_num("LEGAIA_SRC_ROW", 10)
local DST_ROW = probe.getenv_num("LEGAIA_DST_ROW", 11)

local ACTOR_TABLE = 0x801C9370
local BANK_TABLE = 0x801C9360
local CTX_PTR = 0x8007BD24
local REC_STRIDE = 0xD0

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

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local poked, injected, cast_seen, done_seen = false, false, false, false
local last = ""
local shot_n = 0

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== row-poke cast probe == copy bank row %d -> %d, spell 0x%02X", SRC_ROW, DST_ROW, SPELL))
        return {}
    end,
    on_capture = function(c, elapsed)
        local cx = ctx()
        local a0 = actor(0)
        if cx == nil or a0 == nil then return end

        if not poked and elapsed >= 2 then
            local bank = u32(BANK_TABLE)
            if bank >= 0x80000000 and bank < 0x80200000 then
                for i = 0, REC_STRIDE - 1 do
                    probe.write_u8(bank + DST_ROW * REC_STRIDE + i,
                        u8(bank + SRC_ROW * REC_STRIDE + i))
                end
                poked = true
                PCSX.log(string.format("[poke t%d] bank=0x%08X row %d <- row %d (src stream=%d rate=%d)",
                    elapsed, bank, DST_ROW, SRC_ROW,
                    u8(bank + SRC_ROW * REC_STRIDE + 0x0A), u8(bank + SRC_ROW * REC_STRIDE + 0x0B)))
            end
        end

        local st7 = u8(cx + 7)
        local cat = u8(a0 + 0x1DE)

        probe.pad_release(probe.BTN.CROSS)
        if not cast_seen and elapsed < 300 then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end
        if poked and cat == 3 and not cast_seen then
            probe.write_u8(a0 + 0x1DE, 2)
            probe.write_u8(a0 + 0x1DF, SPELL)
            probe.write_u8(a0 + 0x1DD, TARGET_SEAT)
            injected = true
        end
        if injected and st7 == 0x70 then cast_seen = true end

        if cast_seen then
            local ph = u8(cx + 0x279)
            local a3 = actor(TARGET_SEAT)
            local line = string.format(
                "ph=%d st7=0x%02X cstr=%02X/%02X hp0=%d hp3=%d",
                ph, st7, u8(a0 + 0x1D9), u8(a0 + 0x1DA), u16(a0 + 0x14C),
                a3 and u16(a3 + 0x14C) or 0xFFFF)
            if line ~= last then
                PCSX.log(string.format("[t%d] %s", elapsed, line))
                last = line
            end
            if elapsed % 60 == 0 and shot_n < 30 then
                shot_n = shot_n + 1
                screenshot(string.format("poke_%04d", elapsed))
            end
            if not done_seen and (st7 == 0x50 or st7 == 0x51 or st7 == 0x5A) then
                done_seen = true
                PCSX.log(string.format("[DONE t%d] st7=0x%02X", elapsed, st7))
                screenshot(string.format("done_%04d", elapsed))
            end
        end
    end,
})
