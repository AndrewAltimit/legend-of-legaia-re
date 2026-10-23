-- autorun_effect_pool_film.lua
--
-- Film the battle effect pool (`_DAT_8007BD30`, the `FUN_801E0080` walker's
-- 128 child slots) one line per vsync, with a screenshot beside each line.
--
-- A capture's displayed frame is two game frames older than its RAM, so
-- the effect children it shows are not in the state; the retail-compare
-- image child rewinds them (`rewind_effect_child`). This probe is the
-- ground truth for that rewind: load an EARLIER state of the same cast, run
-- it forward with no input, and find the vsync whose pool equals the later
-- capture's RAM pool (`tools`: `scripts/pcsx-redux/effect_pool_film_match.py`).
-- The pool lines a few vsyncs before it are the children the capture's
-- displayed frame drew, and the screenshot at the match shows that frame.
--
-- Poll-only (no breakpoints) - safe to run --fast.
--
-- Env:
--   LEGAIA_SSTATE   save state (or --scenario on the runner)
--   LEGAIA_FRAMES   vsyncs to film after load (default 900)
--   LEGAIA_SHOT_FROM  first vsync to screenshot (default 0)
--
-- Output (out dir): `pool.txt` - `vsync <n> pool <addr> slot:fc:mirror:
-- cursor:wait:x:y:z:anim ...` for every slot whose frame count is non-zero;
-- `shot_<vsync>.raw` + `.raw.meta` (convert with raw2png.py).
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--       --iso <a PPF-free copy of the disc> \
--       --scenario gimard_summon_visible \
--       --lua scripts/pcsx-redux/autorun_effect_pool_film.lua --frames 900

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local SHOT_FROM = probe.getenv_num("LEGAIA_SHOT_FROM", 0)
local POOL_PTR = 0x8007BD30
local SLOTS = 128

local out = io.open(probe.out_path("pool.txt"), "w")

local function s32(v)
    if v >= 0x80000000 then return v - 0x100000000 end
    return v
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_effect_pool_film.lua",
            { sstate = SSTATE, frames = FRAMES })
        return {}
    end,
    on_capture = function(_, el)
        local pool = probe.read_u32(POOL_PTR)
        local parts = { string.format("vsync %d pool %08X", el, pool or 0) }
        if pool ~= nil and probe.in_ram(pool) then
            for s = 0, SLOTS - 1 do
                local c = pool + 0x10 + s * 0x20
                local fc = probe.mem.read_u8(c)
                if fc ~= nil and fc ~= 0 then
                    parts[#parts + 1] = string.format("%d:%d:%d:%d:%d:%d:%d:%d:%08X",
                        s, fc, probe.mem.read_u8(c + 1), probe.mem.read_u8(c + 2),
                        probe.mem.read_u8(c + 3),
                        s32(probe.read_u32(c + 0x0C)), s32(probe.read_u32(c + 0x10)),
                        s32(probe.read_u32(c + 0x14)), probe.read_u32(c + 0x1C))
                end
            end
        end
        out:write(table.concat(parts, " ") .. "\n")
        out:flush()
        if el >= SHOT_FROM then
            screenshot(string.format("shot_%05d", el))
        end
    end,
    on_done = function()
        out:close()
    end,
})
