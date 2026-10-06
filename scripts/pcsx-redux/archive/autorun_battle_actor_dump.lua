-- autorun_battle_actor_dump.lua
--
-- Diagnostic: locate the live battle actors for the party-attack damage trace.
-- Dumps both candidate actor tables so the real enemy (Gobu Gobu) actor + its HP
-- field can be pinned without driving any input:
--   - the battle-action actor-pointer array (&DAT_801c9370)[slot]
--   - the ctx (_DAT_8007bd24) +0x1000..0x1800 pointer band
-- For each candidate pointer logs +0x14c (HP), +0x150, +0x1df (move id), +0x72
-- (render scale). No input; short window; quits.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")

local ACTOR_ARRAY = 0x801C9370
local CTX_PTR     = 0x8007BD24
local GMODE       = 0x8007B83C

local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end
local function u32(a) return probe.read_u32(a) or 0 end
local function ok_ptr(p) return p >= 0x80000000 and p < 0x80200000 end

local function describe(p)
    return string.format(
        "0x%08X hp(+14c)=%d max(+150)=%d mid(+1df)=0x%02X scale(+72)=%d",
        p, s16(probe.read_u16(p + 0x14C) or 0), s16(probe.read_u16(p + 0x150) or 0),
        probe.read_u8(p + 0x1DF) or 0, s16(probe.read_u16(p + 0x72) or 0))
end

local function dump(tag)
    local gm = probe.read_u8(GMODE) or 0
    local cx = u32(CTX_PTR)
    PCSX.log(string.format("== actor dump %s == gm=0x%02X ctx=0x%08X", tag, gm, cx))
    -- 1) the battle-action actor-pointer array.
    for slot = 0, 11 do
        local p = u32(ACTOR_ARRAY + slot * 4)
        if ok_ptr(p) then
            PCSX.log(string.format("  arr[%2d] -> %s", slot, describe(p)))
        end
    end
    -- 2) the ctx pointer band (dedup).
    if ok_ptr(cx) then
        local seen = {}
        for off = 0x1000, 0x1800, 4 do
            local p = u32(cx + off)
            if ok_ptr(p) and not seen[p] then
                seen[p] = true
                local hp = s16(probe.read_u16(p + 0x14C) or 0)
                -- Only the band entries that look like a combat actor (HP 1..9999).
                if hp >= 1 and hp <= 9999 then
                    PCSX.log(string.format("  ctx+0x%X -> %s", off, describe(p)))
                end
            end
        end
    end
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = 200,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed == 60 or elapsed == 130 then dump("t" .. elapsed) end
        if elapsed >= 150 then c.request_quit = true end
    end,
})
