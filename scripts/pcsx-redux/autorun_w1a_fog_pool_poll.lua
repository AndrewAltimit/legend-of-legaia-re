-- autorun_w1a_fog_pool_poll.lua
--
-- Per-vsync poll of the field fog pool (`_DAT_8007B7E0`) from a loaded
-- field state: the script gate `_DAT_8007B854`, the live count the render
-- walk `FUN_8003F348` writes (`_DAT_8007BCA8`, `0x990(gp)`), the spawner's
-- cap (`_DAT_8007BCB0`), the free-stack top (`pool+0`) and the number of
-- records whose alive byte (`+0x05`) is set. A pure observer: no BPs, so it
-- runs under `--fast`. Pair with a VRAM extract of the same state
-- (`extract_vram_from_sstate.py`) for the frame the pool draws into.
--
-- Env: LEGAIA_SSTATE (run_probe.sh --scenario / --sstate), LEGAIA_FRAMES
-- (vsyncs to poll after the load, default 1800), LEGAIA_OUT_DIR.
-- Output: fog_pool.csv (vsync,scene,mode,gate,live,cap,free_top,alive,dt,
-- walk). `live` is the word as the vsync IRQ finds it, so a sample taken
-- while the render walk is mid-pool reads a partial count (the walk zeroes it
-- first, `sw zero,0x990(gp)` at 0x8003F398); `alive` is the alive-byte count,
-- the population. `dt` is the frame step `DAT_1F800393` and `walk` is 1 on
-- the vsyncs where the alive population changed, i.e. a frame ran.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")

local GAME_MODE  = 0x8007B83C
local SCENE_NAME = 0x8007050C
local FOG_GATE   = 0x8007B854
local FOG_POOL   = 0x8007B7E0
local FOG_LIVE   = 0x8007BCA8
local FOG_CAP    = 0x8007BCB0
local SLOTS, REC0, STRIDE = 0x50, 0xA4, 0x18
local FRAME_STEP = 0x1F800393

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1800)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/w1a_fog_pool_poll")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = probe.csv_open(probe.out_path("fog_pool.csv"),
    "vsync,scene,mode,gate,live,cap,free_top,alive,dt,walk")

local function scene_name()
    local s = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7F then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

local vsync, loaded, done = 0, nil, false
local last_sig = nil
local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if loaded == nil then
        if vsync >= 60 then
            if SSTATE ~= "" and not probe.load_save_state(SSTATE) then
                PCSX.log("[fog_poll] load failed"); done = true; PCSX.quit(1); return
            end
            loaded = vsync
        end
        return
    end
    local pool = (mem.read_u32(FOG_POOL) or 0) % 0x100000000
    local top, alive = -1, 0
    if pool >= 0x80000000 and pool < 0x80200000 then
        top = mem.read_u16(pool) or 0
        if top >= 0x8000 then top = top - 0x10000 end
        for i = 0, SLOTS - 1 do
            if (mem.read_u8(pool + REC0 + i * STRIDE + 5) or 0) ~= 0 then alive = alive + 1 end
        end
    end
    -- A frame ran when any record's age word moved: the walk ages every
    -- live record by rate * dt, so this is a per-frame signature.
    local sig = 0
    if pool >= 0x80000000 and pool < 0x80200000 then
        for i = 0, SLOTS - 1 do
            sig = (sig * 31 + (mem.read_u16(pool + REC0 + i * STRIDE) or 0)) % 0x7FFFFFFF
        end
    end
    local walk = (last_sig ~= nil and sig ~= last_sig) and 1 or 0
    last_sig = sig
    CSV:row("%d,%s,0x%02X,%d,%d,%d,%d,%d,%d,%d", vsync - loaded, scene_name(),
        mem.read_u8(GAME_MODE) or 0, mem.read_u32(FOG_GATE) or 0,
        mem.read_u32(FOG_LIVE) or 0, mem.read_u32(FOG_CAP) or 0, top, alive,
        mem.read_scratch_u8(FRAME_STEP), walk)
    if vsync - loaded >= FRAMES then
        done = true; CSV:close(); PCSX.quit(0)
    end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
