-- autorun_clut_vram_watch.lua
--
-- Catches the battle-form party CLUT band upload by polling live VRAM.
-- The band (rows 490..497, x=0..255) is uploaded via a non-LZS GPU
-- transfer from a RAM source that is freed within the upload frame
-- (proven by the disc-read + LZS-decode probes). This probe reads VRAM
-- every vsync via PCSX.getVRAM(); the instant row 492 (Noa) transitions
-- from empty to populated, it (a) lifts the just-uploaded palette bytes
-- straight from VRAM as the search needle, (b) scans main RAM for that
-- needle to locate the live source buffer, and (c) dumps full main RAM
-- so the source can be pinned offline even if the in-Lua scan misses.
--
-- Run from a state that is about to render the battle characters with
-- the band still absent (slot 5 = battle initiating; rows 492/494 empty).
-- Hold CROSS to advance past the Begin/Run menu so the characters render:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--   LEGAIA_FRAMES=3000 LEGAIA_HOLD=CROSS LEGAIA_HOLD_FRAMES=2800 \
--   LEGAIA_OUT_DIR=/tmp/clutprobe/vramwatch \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_clut_vram_watch.lua \
--       timeout --kill-after=30s 700s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE    = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES    = probe.getenv_num("LEGAIA_FRAMES", 3000)
local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "CROSS")
local HOLD_FR   = probe.getenv_num("LEGAIA_HOLD_FRAMES", 2800)
local OUT_DIR   = probe.getenv("LEGAIA_OUT_DIR", "/tmp/clutprobe/vramwatch")
local WATCH_ROW = probe.getenv_num("LEGAIA_WATCH_ROW", 492)  -- Noa
local ROW_BYTES = 2048

os.execute(string.format("mkdir -p %q", OUT_DIR))
local HOLD_BTN = pad.BTN[HOLD_NAME] or pad.BTN.CROSS

local function get_vram()
    local ok, data = pcall(function()
        if PCSX.getVRAM ~= nil then return PCSX.getVRAM() end
        if PCSX.GPU and PCSX.GPU.getVRAM then return PCSX.GPU.getVRAM() end
        return nil
    end)
    if not ok or data == nil then return nil end
    return tostring(data)
end

-- nonzero halfword count in a VRAM row sub-slice (x=0..255 => 512 bytes)
local function band_fill(vram, row)
    local s = vram:sub(row * ROW_BYTES + 1, row * ROW_BYTES + 512)
    local nz = 0
    for i = 1, #s - 1, 2 do
        if s:byte(i) ~= 0 or s:byte(i + 1) ~= 0 then nz = nz + 1 end
    end
    return nz, s
end

local function dump_full_ram(path)
    local mf = PCSX.getMemoryAsFile()
    local fh = io.open(path, "wb")
    if not fh then return end
    local off = 0
    local SIZE = 0x200000
    while off < SIZE do
        local n = math.min(0x40000, SIZE - off)
        local chunk = mf:readAt(n, off)
        if chunk == nil then break end
        fh:write(tostring(chunk))
        off = off + n
    end
    fh:close()
end

-- scan main RAM for a needle; return first virtual addr or nil
local function scan_ram(needle)
    local mf = PCSX.getMemoryAsFile()
    local off = 0
    local SIZE = 0x200000
    local STEP = 0x40000
    while off < SIZE do
        local n = math.min(STEP + #needle, SIZE - off)
        local buf = mf:readAt(n, off)
        if buf == nil then break end
        local idx = tostring(buf):find(needle, 1, true)
        if idx then return 0x80000000 + off + idx - 1 end
        off = off + STEP
    end
    return nil
end

local prev_fill = -1
local fired = false

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    hold_button    = HOLD_BTN,
    hold_frames    = HOLD_FR,
    out_path       = OUT_DIR .. "/result.txt",

    on_arm = function() return {} end,

    on_capture = function(ctx, elapsed)
        if fired then return end
        local vram = get_vram()
        if vram == nil or #vram < 0x100000 then return end
        local fill, rowbytes = band_fill(vram, WATCH_ROW)
        if prev_fill < 0 then prev_fill = fill end
        if (elapsed % 120) == 0 then
            PCSX.log(string.format("[vramwatch] vsync %d: vram=%dB row%d fill=%d",
                elapsed, #vram, WATCH_ROW, fill))
        end
        -- rising edge: row goes from (near-)empty to populated
        if prev_fill < 32 and fill >= 128 then
            fired = true
            PCSX.log(string.format(
                "[vramwatch] row %d filled at vsync %d (fill %d -> %d) - searching RAM for source",
                WATCH_ROW, elapsed, prev_fill, fill))
            -- needle = colors 8..23 of the freshly-uploaded row (skip slot0/transparent)
            local needle = rowbytes:sub(17, 17 + 47)
            local hexn = needle:gsub(".", function(c) return string.format("%02x", c:byte()) end)
            local src = scan_ram(needle)
            local fh = io.open(OUT_DIR .. "/result.txt", "w")
            fh:write(string.format("watch_row=%d vsync=%d fill=%d\n", WATCH_ROW, elapsed, fill))
            fh:write("needle_hex=" .. hexn .. "\n")
            fh:write("ram_source=" .. (src and string.format("0x%08X", src) or "NOT FOUND") .. "\n")
            fh:close()
            -- dump full RAM + the VRAM band for offline analysis
            dump_full_ram(OUT_DIR .. "/ram_at_fill.bin")
            local vf = io.open(OUT_DIR .. "/vram_at_fill.bin", "wb")
            if vf then vf:write(vram); vf:close() end
            if src then
                PCSX.log(string.format("[vramwatch] *** RAM SOURCE for row %d at 0x%08X ***", WATCH_ROW, src))
            else
                PCSX.log("[vramwatch] needle not in RAM at fill frame (freed sub-frame); see ram_at_fill.bin")
            end
            ctx.request_quit = true
        end
        prev_fill = fill
    end,

    on_done = function()
        PCSX.log("=== clut_vram_watch done ===")
    end,
})
