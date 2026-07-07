-- autorun_town01_vram_upload_census.lua
--
-- VRAM-upload census across the opening scene chain, to pin the runtime
-- writer of the town01 CLUT row 510 (the `(64,510)` CLUT) and the
-- `(960,256)` texture-page rows the engine's disc-TIM sweep can't source
-- (23 town01 env-mesh tile instances sample them; retail VRAM has the
-- data, no disc TIM supplies it).
--
-- Mechanism: resume the S1 checkpoint (opdeene field-run) and CROSS-mash
-- forward through opstati -> opurud -> map01 -> town01 (the S2 chain,
-- exec-BP driven exactly like autorun_play_from_boot.lua), with exec
-- breakpoints on BOTH libgpu VRAM writers the whole way:
--
--   LoadImage  FUN_800583C8  (RECT* a0, u_long* a1: RAM -> VRAM)
--   MoveImage  FUN_80058490  (RECT* a0, int a1, int a2: VRAM -> VRAM)
--
-- Every fire is aggregated per (kind, ra, rect, dst, scene) - first/last
-- tick + count - so the per-frame overworld CLUT-cycle doesn't bloat the
-- output. The aggregate lands as CSV on target-reach (or the frame cap).
--
--   LEGAIA_SSTATE=saves/library/pcsx-redux/01ea5175...sstate \
--   LEGAIA_OUT_DIR=/tmp/vramcensus \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_town01_vram_upload_census.lua \
--       timeout --kill-after=30s 1500s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local GM         = 0x8007B83C
local SCENE_NAME = 0x8007050C
local LOAD_IMAGE = 0x800583C8
local MOVE_IMAGE = 0x80058490

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/vramcensus")
local CKPT_SCENE = env.getenv("LEGAIA_CKPT_SCENE", "town01")
-- Delay-arm the GPU breakpoints until this scene is active. The prologue
-- scenes stream XA / play FMV stretches where a hot LoadImage exec-BP
-- segfaults the emulator; arming at map01 leaves only the short
-- map01 -> town01 load window instrumented. Empty = arm immediately.
local ARM_SCENE  = env.getenv("LEGAIA_ARM_SCENE", "map01")
if ARM_SCENE == "" then ARM_SCENE = nil end
local MASH_EVERY = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "120")) or 120
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "9000")) or 9000
local FIELD_BP   = tonumber(env.getenv("LEGAIA_FIELD_BP", "0x8001698C")) or 0
local START_DELAY = tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/census.log", "w")
local function log(s)
    PCSX.log("[census] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end

local function read_scene()
    if not mem.in_ram(SCENE_NAME) then return "" end
    local s = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

-- Aggregate: key -> {count, first_tick, last_tick}. Keys embed the scene
-- so the same rect uploaded by two scene loads shows as two rows.
local agg = {}
local agg_order = {}
local g_tick = 0

local function on_fire(kind)
    return function()
        local r = PCSX.getRegisters()
        local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
        local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
        local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
        local a2 = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)
        local x = mem.read_u16(a0) or 0xFFFF
        local y = mem.read_u16(a0 + 2) or 0xFFFF
        local w = mem.read_u16(a0 + 4) or 0xFFFF
        local h = mem.read_u16(a0 + 6) or 0xFFFF
        local dx, dy
        if kind == "move" then
            dx, dy = bit.band(a1, 0xFFFF), bit.band(a2, 0xFFFF)
        else
            dx, dy = x, y -- LoadImage dest IS the rect
        end
        local key = string.format("%s,0x%08X,%d,%d,%d,%d,%d,%d,%s",
            kind, ra, x, y, w, h, dx, dy, read_scene())
        local e = agg[key]
        if e == nil then
            agg[key] = { count = 1, first = g_tick, last = g_tick }
            agg_order[#agg_order + 1] = key
        else
            e.count = e.count + 1
            e.last = g_tick
        end
    end
end

local function dump_csv()
    local f = io.open(OUT_DIR .. "/uploads.csv", "w")
    if not f then log("cannot open uploads.csv"); return end
    f:write("kind,ra,x,y,w,h,dst_x,dst_y,scene,count,first_tick,last_tick\n")
    for _, key in ipairs(agg_order) do
        local e = agg[key]
        f:write(string.format("%s,%d,%d,%d\n", key, e.count, e.first, e.last))
    end
    f:close()
    log(string.format("uploads.csv: %d unique upload keys", #agg_order))
end

local PHASE = "ADVANCE"
local vsync = 0
local start_loaded = false
local g_mash_until = 0
local g_target_since = nil
local g_quit_at = nil

local function cross_press() pad.force(pad.BTN.CROSS); pad.force(pad.BTN.CIRCLE) end
local function cross_release() pad.release(pad.BTN.CROSS); pad.release(pad.BTN.CIRCLE) end

local gpu_armed = false
local function arm_gpu_bps()
    if gpu_armed then return end
    gpu_armed = true
    pcall(function()
        bp.arm(LOAD_IMAGE, "Exec", 4, "loadimage", on_fire("load"))
    end)
    pcall(function()
        bp.arm(MOVE_IMAGE, "Exec", 4, "moveimage", on_fire("move"))
    end)
    log(string.format("[tick %d] GPU BPs armed (scene=%q)", g_tick, read_scene()))
end

local function field_tick()
    g_tick = g_tick + 1
    if not gpu_armed and (ARM_SCENE == nil or read_scene() == ARM_SCENE) then
        arm_gpu_bps()
    end
    if PHASE == "DONE" then
        if g_quit_at and g_tick >= g_quit_at then
            if LOG then LOG:close() end
            PCSX.quit(0)
        end
        return
    end
    if g_tick >= MAX_FRAMES then
        log(string.format("frame cap %d hit (scene=%q); dumping partial census",
            MAX_FRAMES, read_scene()))
        cross_release()
        dump_csv()
        PHASE = "DONE"
        g_quit_at = g_tick + 2
        return
    end
    if (g_tick % 120) == 0 then
        log(string.format("[tick %d] scene=%q keys=%d",
            g_tick, read_scene(), #agg_order))
        -- periodic dump so an emulator crash still leaves the census on disk
        if gpu_armed then dump_csv() end
    end
    if g_mash_until > 0 and g_tick >= g_mash_until then
        cross_release(); g_mash_until = 0
    elseif (g_tick % MASH_EVERY) == 0 and g_mash_until == 0 then
        cross_press(); g_mash_until = g_tick + 5
    end
    if read_scene() == CKPT_SCENE then
        if g_target_since == nil then
            g_target_since = g_tick
        elseif g_tick - g_target_since >= SETTLE then
            cross_release()
            log(string.format("settled at %s (tick %d); dumping census",
                CKPT_SCENE, g_tick))
            dump_csv()
            PHASE = "DONE"
            g_quit_at = g_tick + 2
        end
    else
        g_target_since = nil
    end
end

local function on_vsync()
    vsync = vsync + 1
    if not start_loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        start_loaded = true
        if sstate.load(START_SAVE) then
            log("resumed from " .. START_SAVE)
        else
            log("FAILED to load " .. START_SAVE)
        end
    end
end

if FIELD_BP ~= 0 then
    pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick", field_tick) end)
end
log(string.format(
    "armed: field=0x%08X; GPU BPs (load=0x%08X move=0x%08X) arm at scene=%s; target=%s",
    FIELD_BP, LOAD_IMAGE, MOVE_IMAGE, tostring(ARM_SCENE), CKPT_SCENE))
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
