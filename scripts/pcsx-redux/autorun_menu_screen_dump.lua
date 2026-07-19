-- autorun_menu_screen_dump.lua
--
-- Pad-walk to a pause-menu screen and dump ground-truth pixels + RAM at
-- named checkpoints. The generic capture tool behind the Items / Magic
-- screen pins: autorun_pad_walk.lua's scripted walk, plus per-checkpoint
-- framebuffer screenshots and full main-RAM dumps so window structs,
-- submenu ids and on-screen ink can all be pinned from one run.
--
-- What it writes per checkpoint <name>:
--   <out>/<name>_fb.raw    raw framebuffer bytes (BGR555 or RGB24)
--   <out>/<name>_fb.meta   width / height / bpp / bytes_per_pixel lines
--   <out>/<name>_ram.bin   full 2 MiB main RAM
--
-- The screenshot LAGS the draw (it returns the displayed buffer), so
-- checkpoints must sit well after the last input once the screen has
-- parked - schedule them a few dozen vsyncs past the final press.
--
-- Env vars:
--   LEGAIA_SSTATE      sstate path
--   LEGAIA_OUT_DIR     output dir
--   LEGAIA_FRAMES      vsyncs to run (default 600)
--   LEGAIA_PAD_SCRIPT  comma list of <vsync>:<BUTTON>[:<hold>] steps
--                      (hold defaults to 6), e.g. "60:SELECT,240:CROSS"
--   LEGAIA_DUMP_AT     comma list of <vsync>:<name> checkpoints, e.g.
--                      "500:items_browse"
--
-- No breakpoints are armed, so --fast (dynarec) runs are fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local OUT_LOG = probe.out_path("menu_screen_dump.log")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 600)
local SCRIPT  = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local DUMPS   = probe.getenv("LEGAIA_DUMP_AT", "")

local GAME_MODE_VA  = 0x8007b83c
local SCENE_NAME_VA = 0x8007050c
local PAD_DATA_VA   = 0x800840f8
-- Menu-overlay screen/submenu state words (menu overlay = PROT 0899):
-- current / settled submenu id, plus the two cursor words.
local MENU_SUB_A_VA = 0x801e46a4
local MENU_SUB_B_VA = 0x801e46a8
local MENU_CUR_0_VA = 0x801e46c0
local MENU_CUR_1_VA = 0x801e46c4

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[menu_dump] " .. s)
end

local function parse_pad(text)
    local steps = {}
    for chunk in string.gmatch(text, "[^,]+") do
        local parts = {}
        for p in string.gmatch(chunk, "[^:]+") do parts[#parts + 1] = p end
        local at   = tonumber(parts[1])
        local name = parts[2] and string.upper(parts[2])
        local hold = tonumber(parts[3] or "6")
        local btn  = name and probe.BTN[name]
        if at == nil or btn == nil then
            logf("BAD PAD STEP %q", chunk)
        else
            steps[#steps + 1] = { at = at, btn = btn, hold = hold, name = name }
        end
    end
    return steps
end

local function parse_dumps(text)
    local marks = {}
    for chunk in string.gmatch(text, "[^,]+") do
        local at, name = string.match(chunk, "^%s*(%d+):(%S+)%s*$")
        if at == nil then
            logf("BAD DUMP MARK %q (want <vsync>:<name>)", chunk)
        else
            marks[#marks + 1] = { at = tonumber(at), name = name }
        end
    end
    return marks
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function dump_checkpoint(name, vsync)
    local mode = probe.read_u8(GAME_MODE_VA) or 0xFF
    logf("DUMP %s vsync=%d game_mode=0x%02x scene=%q sub=(%04x,%04x) cur=(%04x,%04x)",
        name, vsync, mode, scene_name(),
        probe.read_u16(MENU_SUB_A_VA) or 0xFFFF,
        probe.read_u16(MENU_SUB_B_VA) or 0xFFFF,
        probe.read_u16(MENU_CUR_0_VA) or 0xFFFF,
        probe.read_u16(MENU_CUR_1_VA) or 0xFFFF)

    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss ~= nil then
        local bpp_bits = 16
        if (tonumber(ss.bpp) or 0) > 16 then bpp_bits = 24 end
        local w, h = tonumber(ss.width), tonumber(ss.height)
        local fh = io.open(probe.out_path(name .. "_fb.raw"), "wb")
        if fh ~= nil then
            fh:write(tostring(ss.data))
            fh:close()
        end
        local mh = io.open(probe.out_path(name .. "_fb.meta"), "w")
        if mh ~= nil then
            mh:write(string.format(
                "width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
                w, h, bpp_bits, bpp_bits / 8))
            mh:close()
        end
        logf("  fb %dx%d %dbpp", w, h, bpp_bits)
    else
        logf("  takeScreenShot() unavailable")
    end

    local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
    if buf ~= nil then
        local fh = io.open(probe.out_path(name .. "_ram.bin"), "wb")
        if fh ~= nil then
            fh:write(tostring(buf))
            fh:close()
            logf("  ram 2MiB written")
        end
    end
end

local steps = parse_pad(SCRIPT)
local marks = parse_dumps(DUMPS)
local releases = {}
local last_mode = nil
local pad_seen = {}

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(_)
        logf("sstate=%s frames=%d steps=%d dumps=%d",
            SSTATE_PATH, FRAMES, #steps, #marks)
        return {}
    end,

    on_capture = function(_, vsync)
        local mode = probe.read_u8(GAME_MODE_VA)
        if mode ~= last_mode then
            logf("vsync=%d game_mode=0x%02x scene=%q", vsync, mode or 0xFF,
                scene_name())
            last_mode = mode
        end
        local pw = probe.read_u32(PAD_DATA_VA)
        if pw ~= nil and not pad_seen[pw] then pad_seen[pw] = vsync end

        for _, s in ipairs(steps) do
            if s.at == vsync then
                probe.pad.force(s.btn)
                logf("vsync=%d press %s", vsync, s.name)
                local rel = vsync + s.hold
                releases[rel] = releases[rel] or {}
                table.insert(releases[rel], s)
            end
        end
        local rel = releases[vsync]
        if rel ~= nil then
            for _, s in ipairs(rel) do
                probe.pad.release(s.btn)
                logf("vsync=%d release %s", vsync, s.name)
            end
            releases[vsync] = nil
        end

        for _, m in ipairs(marks) do
            if m.at == vsync then dump_checkpoint(m.name, vsync) end
        end
    end,

    on_done = function(_, _)
        for _, s in ipairs(steps) do probe.pad.release(s.btn) end
        local n = 0
        for _ in pairs(pad_seen) do n = n + 1 end
        logf("pad words seen: %d%s", n,
            n <= 1 and "  <-- PAD INJECTION NOT REACHING THE GAME" or "")
        local fh = io.open(OUT_LOG, "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
