-- autorun_player_special_cast.lua
--
-- Probe: can a PARTY actor run a capture-class boss cast (Megaton Press,
-- spell 0x7A -> module PROT 959) end to end?
--
-- On the party_battle_gobu_gobu state (command menu open, Vahn's turn,
-- one Gobu Gobu at monster seat 3):
--   1. pad-drive CROSS to select Attack + confirm the target;
--   2. while the actor's category byte +0x1DE reads 3 (Attack), rewrite it
--      to 2 (Magic) with +0x1DF = 0x7A and target seat +0x1DD = 3;
--   3. log ctx[7] (party action state), ctx[+0x279] (capture-module phase),
--      anims, HP of party seat 0 and monster seat 3, and the first word of
--      the slot-B window 0x801F69D8 (module identity);
--   4. screenshot every 30 frames once the cast band (0x28/0x6E..0x71) is
--      entered.
--
-- Success = ctx[7] walks 0x28 -> 0x6E..0x71 -> 0x50 band and the battle
-- keeps ticking afterwards. Damage landing on party seat 0 is EXPECTED on
-- an unpatched module (the module's damage sites hardcode actor table[0]).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local SPELL = probe.getenv_num("LEGAIA_SPELL", 0x7A)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)

local ACTOR_TABLE = 0x801C9370
local CTX_PTR = 0x8007BD24
local SLOTB = 0x801F69D8

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

local injected = false
local cast_seen = false
local done_seen = false
local last_line = ""
local shot_n = 0

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== player special cast probe == spell=0x%02X target_seat=%d", SPELL, TARGET_SEAT))
        return {}
    end,
    on_capture = function(c, elapsed)
        local cx = ctx()
        local a0 = actor(0)
        if cx == nil or a0 == nil then return end
        local st7 = u8(cx + 7)
        local phase = u8(cx + 0x279)
        local cat = u8(a0 + 0x1DE)
        local id = u8(a0 + 0x1DF)
        local tgt = u8(a0 + 0x1DD)
        local a3 = actor(TARGET_SEAT)
        local hp0 = u16(a0 + 0x14C)
        local hp3 = a3 and u16(a3 + 0x14C) or 0xFFFF
        local slotb = u32(SLOTB)

        -- 1) On party_basic_attack_vs_gobu_gobu the Attack is already queued
        -- and the battle sits at the Begin/Reselect confirm: press X (Begin)
        -- a few spaced times until the cast band is entered.
        probe.pad_release(probe.BTN.CROSS)
        if not cast_seen and elapsed < 300 then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end

        -- 2) The moment the Attack category lands, convert it to the cast.
        if cat == 3 and not cast_seen then
            probe.write_u8(a0 + 0x1DE, 2)
            probe.write_u8(a0 + 0x1DF, SPELL)
            probe.write_u8(a0 + 0x1DD, TARGET_SEAT)
            if not injected then
                PCSX.log(string.format("[inject t%d] +0x1DE 3->2, +0x1DF=0x%02X, +0x1DD=%d (st7=0x%02X)", elapsed, SPELL, TARGET_SEAT, st7))
            end
            injected = true
        end

        -- 3) Track the cast band.
        if injected and (st7 == 0x28 or (st7 >= 0x6E and st7 <= 0x71)) and not cast_seen then
            cast_seen = true
            PCSX.log(string.format("[cast t%d] entered magic band st7=0x%02X", elapsed, st7))
        end
        if cast_seen and not done_seen and (st7 == 0x50 or st7 == 0x51 or st7 == 0x5A) then
            done_seen = true
            PCSX.log(string.format("[done t%d] reached done band st7=0x%02X hp0=%d hp3=%d", elapsed, st7, hp0, hp3))
        end

        local line = string.format("st7=0x%02X ph=%d cat=%d id=0x%02X tgt=%d hp0=%d hp3=%d slotb=0x%08X anim=%02X/%02X",
            st7, phase, cat, id, tgt, hp0, hp3, slotb, u8(a0 + 0x1D9), u8(a0 + 0x1DA))
        if line ~= last_line then
            PCSX.log(string.format("[t%d] %s", elapsed, line))
            last_line = line
        end
        if cast_seen and not done_seen and elapsed % 30 == 0 and shot_n < 40 then
            shot_n = shot_n + 1
            screenshot(string.format("cast_%04d", elapsed))
        end
        if done_seen and elapsed % 120 == 0 then
            screenshot(string.format("after_%04d", elapsed))
        end
    end,
})
