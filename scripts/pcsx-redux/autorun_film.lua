-- autorun_film.lua
--
-- Film a retail sequence: load a save state, drive the pad from a schedule,
-- and take a screenshot every N vsyncs. The retail side of a "does the port
-- look like this?" question, shaped like play-window's own
-- `--pad-script` + `--screenshot-every` so the two runs line up.
--
-- Poll-only (no breakpoints) - safe to run --fast.
--
-- Env:
--   LEGAIA_SSTATE      save state (or --scenario on the runner)
--   LEGAIA_FRAMES      vsyncs to film after load (default 1200)
--   LEGAIA_PAD_SCRIPT  comma-separated `V:BTN` (tap, held 4 vsyncs) or
--                      `A-B:BTN` (held from vsync A to B), vsyncs counted
--                      from the load; BTN is a probe.BTN name (CROSS, UP, ...)
--   LEGAIA_SHOT_EVERY  vsyncs between screenshots (default 4)
--   LEGAIA_SHOT_FROM   first vsync to film (default 0)
--   LEGAIA_POKES       comma-separated `V:SLOT:OFF:VAL` - at vsync V, write
--                      the u16 VAL at battle actor `DAT_801C9370[SLOT] + OFF`
--                      (hex OFF / VAL), e.g. `900:3:14C:1` sets the first
--                      monster's HP to 1 so a short film reaches the kill
--
-- Output: `shot_<vsync>.raw` + `.raw.meta` in the out dir; convert with
-- `python3 scripts/pcsx-redux/raw2png.py <dir>/*.raw`.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--       --iso <a PPF-free copy of the disc> \
--       --scenario rikuroa_pre_caruban \
--       --lua scripts/pcsx-redux/autorun_film.lua --frames 3000

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
local EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 4)
local FROM = probe.getenv_num("LEGAIA_SHOT_FROM", 0)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")

-- Parse the schedule into {from, to, btn} spans.
local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%a+)%s*$")
    if a == nil then
        a, name = string.match(item, "^%s*(%d+):(%a+)%s*$")
        if a ~= nil then b = tonumber(a) + 4 end
    end
    local btn = name and probe.BTN[string.upper(name)]
    if a ~= nil and btn ~= nil then
        spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn }
    else
        PCSX.log("[film] ignoring pad item '" .. item .. "'")
    end
end

local POKES = probe.getenv("LEGAIA_POKES", "")
local ACTOR_TABLE = 0x801C9370
local pokes = {}
for item in string.gmatch(POKES, "[^,]+") do
    local v, slot, off, val = string.match(item,
        "^%s*(%d+):(%d+):(%x+):(%x+)%s*$")
    if v ~= nil then
        pokes[#pokes + 1] = { at = tonumber(v), slot = tonumber(slot),
            off = tonumber(off, 16), val = tonumber(val, 16) }
    else
        PCSX.log("[film] ignoring poke '" .. item .. "'")
    end
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return false end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
    return true
end

local held = {}

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_film.lua",
            { sstate = SSTATE, frames = FRAMES, pad = SCRIPT, every = EVERY })
        return {}
    end,
    on_capture = function(_, el)
        -- A button is held while any of its spans covers this vsync.
        local want = {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
        end
        for _, p in ipairs(pokes) do
            if el == p.at then
                local base = probe.read_u32(ACTOR_TABLE + p.slot * 4)
                if base ~= nil and probe.in_ram(base) then
                    probe.write_u16(base + p.off, p.val)
                    PCSX.log(string.format("[film] poke %08X = %04X",
                        base + p.off, p.val))
                end
            end
        end
        if el >= FROM and (el - FROM) % EVERY == 0 then
            screenshot(string.format("shot_%05d", el))
        end
    end,
})
