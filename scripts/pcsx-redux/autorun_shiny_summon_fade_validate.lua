-- autorun_shiny_summon_fade_validate.lua
--
-- Settle Piece 1 of the shiny-Seru cosmetics (summon-mesh transparency):
-- does forcing the SUMMON actor's fade byte (`+0x226`) actually render the
-- summoned creature semi-transparent during a Seru cast? The per-primitive
-- fade modulator `FUN_8004A908` (read at 0x8004AD0C) scales colour by
-- (0x80-fade)/0x80 and sets the GPU STP bit when `+0x226 != 0`. Prior work
-- proved the summon is drawn through that path but never visually confirmed a
-- strong fade (0x40) on the *summon* actor (earlier tries faded the target
-- enemy, or the hook was gated off mid-cast).
--
-- This probe is self-contained on the VANILLA cast state (SCUS94254.sstate7):
-- the fade mechanism is identical patched/unpatched; only the shiny-gating
-- differs, which this does NOT test. It:
--   1. dumps battle actor table slots 0..7 with the fields that distinguish a
--      summon (model `+0x22c`, marker `+0x06`, `+0x21c`, `+0x21d`, fade `+0x226`),
--   2. auto-detects the summon slot (model != 0 AND `+0x21d == 0x02`,
--      i.e. the write-#1 setup path, NOT the combatant `0x08` path),
--   3. screenshots the framebuffer BEFORE,
--   4. forces the summon actor `+0x226 = 0x40`,
--   5. runs a few frames and screenshots AFTER.
-- Compare the two PNGs: if the creature mesh is translucent in AFTER, the
-- fade approach is validated and the only remaining work is the shiny-gated
-- injection.
--
-- USAGE:
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstate7 \
--     bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_shiny_summon_fade_validate.lua \
--       --sstate ~/Tools/pcsx-redux/SCUS94254.sstate7 --frames 40
--
-- Output: scripts/pcsx-redux/out/summon_fade_{before,after}.{raw,meta}

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 40)

local ACTOR_TABLE = 0x801C9370
local FADE_OFF    = 0x226
local FORCE_FADE  = probe.getenv_num("LEGAIA_FORCE_FADE", 0x40)
-- Optional explicit summon slot override (auto-detect by default).
local FORCE_SLOT  = probe.getenv_num("LEGAIA_SUMMON_SLOT", -1)

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local function take_fb(stem, label)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then
        PCSX.log(string.format("[validate] %s takeScreenShot unavailable", label))
        return
    end
    local bpp_bits = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local w, h = tonumber(ss.width), tonumber(ss.height)
    local raw = probe.out_path(stem .. ".raw")
    local fh = io.open(raw, "wb")
    if fh ~= nil then
        fh:write(tostring(ss.data)); fh:close()
    end
    local mh = io.open(probe.out_path(stem .. ".meta"), "w")
    if mh ~= nil then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n", w, h, bpp_bits))
        mh:close()
    end
    PCSX.log(string.format("[validate] %s fb %dx%d %dbpp -> %s", label, w, h, bpp_bits, raw))
end

local summon_ptr = nil
local summon_slot = -1

local function dump_and_detect()
    PCSX.log("[validate] actor table slots 0..7:")
    local cands = {}
    for i = 0, 7 do
        local ptr = u32(ACTOR_TABLE + i * 4)
        if ptr >= 0x80000000 and ptr < 0x80200000 then
            local model = u32(ptr + 0x22c)
            local mark  = u16(ptr + 0x06)
            local t21c  = u8(ptr + 0x21c)
            local t21d  = u8(ptr + 0x21d)
            local fade  = u8(ptr + FADE_OFF)
            local act   = u8(ptr + 0x1df)
            PCSX.log(string.format(
                "  slot%d ptr=0x%08X model=0x%08X mark06=0x%04X 21c=0x%02X 21d=0x%02X fade=0x%02X act1df=0x%02X",
                i, ptr, model, mark, t21c, t21d, fade, act))
            if model ~= 0 and t21d == 0x02 then
                cands[#cands + 1] = { slot = i, ptr = ptr }
            end
        end
    end
    if FORCE_SLOT >= 0 then
        local ptr = u32(ACTOR_TABLE + FORCE_SLOT * 4)
        if ptr >= 0x80000000 and ptr < 0x80200000 then
            summon_slot = FORCE_SLOT; summon_ptr = ptr
            PCSX.log(string.format("[validate] summon = slot%d ptr=0x%08X (forced via env)",
                summon_slot, summon_ptr))
            return
        end
    end
    if #cands == 1 then
        summon_slot = cands[1].slot
        summon_ptr  = cands[1].ptr
        PCSX.log(string.format("[validate] summon = slot%d ptr=0x%08X (model!=0 && +0x21d==0x02)",
            summon_slot, summon_ptr))
    else
        PCSX.log(string.format("[validate] WARN: %d summon candidates (expected 1); "
            .. "falling back to marker +0x06==0x2008", #cands))
        for i = 0, 7 do
            local ptr = u32(ACTOR_TABLE + i * 4)
            if ptr >= 0x80000000 and ptr < 0x80200000 and u16(ptr + 0x06) == 0x2008
               and u32(ptr + 0x22c) ~= 0 then
                summon_slot = i; summon_ptr = ptr
                PCSX.log(string.format("[validate] summon (fallback) = slot%d ptr=0x%08X", i, ptr))
                break
            end
        end
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        PCSX.log("== shiny summon fade validate ==")
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if elapsed == 2 then
            dump_and_detect()
        end
        if summon_ptr == nil then
            if elapsed >= 4 then ctx.request_quit = true end
            return
        end
        -- Force fade every frame from elapsed>=2 (it persists; the point is to
        -- have it set on whichever frame the summon becomes ACTIVE, +0x4 != 0,
        -- since FUN_8004A908 only reads +0x226 on active frames).
        local active = u32(summon_ptr + 0x04)
        PCSX.log(string.format("[validate] f=%d slot%d +0x4=0x%08X mark06=0x%04X fade=0x%02X",
            elapsed, summon_slot, active, u16(summon_ptr + 0x06), u8(summon_ptr + FADE_OFF)))
        probe.write_u8(summon_ptr + FADE_OFF, FORCE_FADE)
        if elapsed == 2 then
            take_fb("summon_fade_before", "BEFORE")
        end
        -- screenshot a few snapshots so we catch an active frame
        if elapsed == 8 or elapsed == 16 or elapsed == 26 then
            take_fb("summon_fade_f" .. elapsed, "F" .. elapsed)
        end
        if elapsed >= 28 then
            take_fb("summon_fade_after", "AFTER")
            ctx.request_quit = true
        end
    end,
    on_summary = function() PCSX.log("[validate] done") end,
})
