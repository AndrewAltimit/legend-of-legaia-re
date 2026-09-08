-- autorun_results_window_emitter.lua
--
-- Who emits the post-battle results windows (the level-up window + the spoils
-- window and their gold frame band)?
--
-- `docs/subsystems/level-up.md` records the output - the two window rects, the
-- text pen and the two numeral columns are measured off a 320x240 retail frame
-- - and records where the emitter is *not*: a jal-target sweep over the 84
-- extracted overlay images finds five callers of the SCUS window emitter
-- `FUN_8002C69C` and battle overlay 0898 is not one of them, so these windows
-- are not the menu's 9-slice skin drawn from battle code. Whatever draws them
-- builds its own primitives.
--
-- Every primitive in this engine is linked into the ordering table by one
-- helper: `AddPrim` = `FUN_8003D2C4(ot_slot /*a0*/, prim /*a1*/)`, a 15-
-- instruction pointer swap (`see ghidra/scripts/funcs/8003d2c4.txt`). So an
-- Exec breakpoint there sees **every** emitted packet, and `ra` is the emitter.
--
-- The contrast is taken over TIME, not over a phase gate. The scenario resolves
-- the fight on its own within the first hundred-odd vsyncs and then returns to
-- the field, so a "wait until the enemies are at zero HP, then arm" design
-- misses the very frames it is after (measured: the battle is already over by
-- the first poll). Instead the breakpoint is armed for the whole capture and
-- every hit is folded into a per-`ra` aggregate carrying its first and last
-- vsync; the results-window emitters are the callers whose first vsync is late
-- and whose span is short. A per-(vsync, ra) timeline CSV makes that readable
-- directly, and each caller keeps sample packets - GP0 command byte, colour
-- word, screen XY - so the gold frame band is identified by its own bytes.
--
-- Run (retail disc, no pad input needed - the scenario resolves on its own):
--   timeout --kill-after=30s 1800s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario rim_elm_gimard_victory \
--       --lua scripts/pcsx-redux/autorun_results_window_emitter.lua \
--       --frames 400 \
--       --out-dir captures/w3c/results_window_emitter

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 400)
local OUT_PATH = probe.out_path("results_window_emitter.csv")

-- Vsync ranges in which EVERY packet is recorded, not just the first per
-- caller. Two ranges make the results frame a contrast against a mid-fight
-- one: the window chrome is whatever is present in the late range and absent
-- from the early one. Defaults straddle the measured slide-in (the results
-- widgets appear around vsync 80 and park by 110).
local FULL_A_LO = probe.getenv_num("LEGAIA_FULL_A_LO", 20)
local FULL_A_HI = probe.getenv_num("LEGAIA_FULL_A_HI", 23)
local FULL_B_LO = probe.getenv_num("LEGAIA_FULL_B_LO", 130)
local FULL_B_HI = probe.getenv_num("LEGAIA_FULL_B_HI", 133)

local function full_frame(f)
    return (f >= FULL_A_LO and f <= FULL_A_HI)
        or (f >= FULL_B_LO and f <= FULL_B_HI)
end

-- AddPrim(ot_slot, prim): the single OT linker every emitter calls.
local ADD_PRIM = 0x8003D2C4

-- ra -> { n, first, last, samples = {...} }
local callers = {}
-- Distinct callers seen in the current vsync, emitted as a timeline row.
local this_frame = {}
local frame = 0
local armed = false
local rows = 0
local ROW_CAP = 60000
local SAMPLES_PER_CALLER = 10

local csv = probe.csv_open(OUT_PATH,
    "vsync,ra,packet_va,code,bgr,x,y,word3,word4")

local function u32(a) return probe.read_u32(a) or 0 end

--- One AddPrim hit. Cheap: fold `ra` into the aggregate, and emit one detail
--- row the first time each caller is seen in a given vsync (so the CSV is a
--- caller timeline with geometry, not a per-packet firehose).
local function on_add_prim()
    local r = PCSX.getRegisters()
    local ra = (tonumber(r.GPR.n.ra) or 0) % 0x100000000
    local e = callers[ra]
    if e == nil then
        e = { n = 0, first = frame, last = frame, samples = {} }
        callers[ra] = e
    end
    e.n = e.n + 1
    e.last = frame
    local full = full_frame(frame)
    if this_frame[ra] and not full then return end
    this_frame[ra] = true
    if rows >= ROW_CAP then return end
    local prim = (tonumber(r.GPR.n.a1) or 0) % 0x100000000
    -- Packet layout: word0 tag, word1 code<<24|bgr, word2 y<<16|x.
    local w1 = u32(prim + 4)
    local w2 = u32(prim + 8)
    local code = math.floor(w1 / 0x1000000) % 0x100
    local bgr = w1 % 0x1000000
    local x = w2 % 0x10000
    local y = math.floor(w2 / 0x10000) % 0x10000
    if x >= 0x8000 then x = x - 0x10000 end
    if y >= 0x8000 then y = y - 0x10000 end
    rows = rows + 1
    if #e.samples < SAMPLES_PER_CALLER then
        e.samples[#e.samples + 1] = string.format(
            "v%d:code=%02X bgr=%06X xy=(%d,%d)", frame, code, bgr, x, y)
    end
    csv:row("%d,0x%08X,0x%08X,0x%02X,0x%06X,%d,%d,0x%08X,0x%08X",
        frame, ra, prim, code, bgr, x, y, u32(prim + 12), u32(prim + 16))
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(ADD_PRIM, "Exec", 4, "addprim", on_add_prim)
        armed = true
        return { { addr = ADD_PRIM, name = "addprim" } }
    end,

    on_capture = function(_ctx, elapsed)
        frame = elapsed
        this_frame = {}
    end,

    on_done = function()
        if armed then probe.disarm_all(); armed = false end
        csv:close()
        local keys = {}
        for ra in pairs(callers) do keys[#keys + 1] = ra end
        table.sort(keys, function(a, b)
            if callers[a].first ~= callers[b].first then
                return callers[a].first < callers[b].first
            end
            return a < b
        end)
        PCSX.log(string.format(
            "=== AddPrim callers over %d vsyncs: %d distinct, %d timeline rows ===",
            FRAMES, #keys, rows))
        for _, ra in ipairs(keys) do
            local e = callers[ra]
            PCSX.log(string.format("[caller] ra=0x%08X n=%d first=%d last=%d span=%d | %s",
                ra, e.n, e.first, e.last, e.last - e.first,
                table.concat(e.samples, " | ")))
        end
    end,
})
