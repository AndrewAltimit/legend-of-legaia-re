-- autorun_pause_vram_upload_trace.lua
--
-- Pin the pause-menu-path writer of the extraction-0874 s2 (player.lzs)
-- F-variant pixels: 3 words of VRAM row 271 (x = 853 / 856 / 857) that every
-- pause-menu capture holds changed from the disc TIM bytes, each equal to the
-- disc word two rows down at (x, 273). Trace every libgpu LoadImage /
-- MoveImage request from a field state while a scripted SELECT press opens
-- the pause menu; the offline filter then names the rect + caller RA that
-- covers row 271 in the x window.
--
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_pause_vram_upload_trace.lua \
--   LEGAIA_FRAMES=500 \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --scenario field_walled_collision_pin \
--       --lua scripts/pcsx-redux/autorun_pause_vram_upload_trace.lua
--
-- Env:
--   LEGAIA_SSTATE      save state (run_probe.sh --scenario fills this)
--   LEGAIA_FRAMES      capture vsyncs (default 500)
--   LEGAIA_OUT_DIR     output dir (default /tmp/pausevram)
--   LEGAIA_SELECT_AT   vsync of the SELECT press (default 60, hold 8)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem = require("probe.mem")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 500)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "/tmp/pausevram")
local SELECT_AT = probe.getenv_num("LEGAIA_SELECT_AT", 60)
local SELECT_HOLD = 8

-- libgpu wrappers (SCUS-resident, stable VAs).
local LOAD_IMAGE = 0x800583C8   -- LoadImage(RECT *rect, u_long *data)
local MOVE_IMAGE = 0x80058490   -- MoveImage(RECT *rect, int dst_x, int dst_y)

local GAME_MODE_VA = 0x8007b83c

os.execute(string.format("mkdir -p %q", OUT_DIR))

local csv = probe.csv_open(OUT_DIR .. "/uploads.csv",
    "tick,kind,ra,src,src_x,src_y,w,h,dst_x,dst_y")
local hits = 0
local tick = 0

local function on_fire(kind)
    return function()
        local r = PCSX.getRegisters()
        local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
        local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
        local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
        local a2 = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)
        local sx = mem.read_u16(a0) or 0xFFFF
        local sy = mem.read_u16(a0 + 2) or 0xFFFF
        local w = mem.read_u16(a0 + 4) or 0xFFFF
        local h = mem.read_u16(a0 + 6) or 0xFFFF
        hits = hits + 1
        -- For LoadImage a1 = source main-RAM pointer; for MoveImage a1/a2 =
        -- destination x/y and the rect is the SOURCE rect.
        csv:row("%d,%s,0x%08X,0x%08X,%d,%d,%d,%d,%d,%d",
            tick, kind, ra, a1, sx, sy, w, h,
            bit.band(a1, 0x3FF), bit.band(a2, 0x1FF))
    end
end

local last_mode = nil
local released = false

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    out_path       = OUT_DIR .. "/uploads.csv",

    on_arm = function()
        probe.arm_breakpoint(LOAD_IMAGE, "Exec", 4, "loadimage_entry",
            on_fire("load"))
        probe.arm_breakpoint(MOVE_IMAGE, "Exec", 4, "moveimage_entry",
            on_fire("move"))
        return {}
    end,

    on_capture = function(_ctx, elapsed)
        tick = elapsed
        local mode = probe.read_u8(GAME_MODE_VA)
        if mode ~= last_mode then
            PCSX.log(string.format(
                "[pausevram] vsync=%d game_mode=0x%02x", tick, mode or 0xFF))
            last_mode = mode
        end
        if tick == SELECT_AT then
            pad.force(pad.BTN.SELECT)
            PCSX.log(string.format("[pausevram] vsync=%d press SELECT", tick))
        elseif tick == SELECT_AT + SELECT_HOLD and not released then
            pad.release(pad.BTN.SELECT)
            released = true
            PCSX.log(string.format("[pausevram] vsync=%d release SELECT", tick))
        end
    end,

    on_done = function()
        pad.release(pad.BTN.SELECT)
        csv:close()
        PCSX.log(string.format(
            "=== pause_vram_upload_trace: %d upload(s) logged ===", hits))
    end,
})
