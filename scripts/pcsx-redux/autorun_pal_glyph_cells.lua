-- autorun_pal_glyph_cells.lua
--
-- Cold-boot capture on an official PAL disc (French SCES_019.44 by default):
-- what does retail draw for the text bytes 0xD7 and 0xF8?
--
-- PAL text writes `Î` as 0xD7 and `°` as 0xF8, and the PAL font page carries a
-- labelled placeholder box in both cells (docs/formats/dialog-font.md#the-pal-page).
-- The SCES text renderer (FR 0x80037088, the SCUS FUN_80036888 sibling) runs
-- its string through the preprocessor at FR 0x80036BB4 into the buffer at
-- 0x800749CC and then addresses each glyph cell straight from the byte
-- (`u = (b & 0xF) << 4`, `v = (b & 0xF0) - 0x20`, FR 0x800373B4..0x800373E0).
-- This probe makes that visible: an exec breakpoint just after the
-- preprocessor returns (FR 0x800370F0, where s0 still holds the buffer)
-- rewrites the first three letters of every string drawn to 0xD7, 0xF8 and
-- 0xDD (the cell the PAL page draws `Î` in - the control), so the screenshot
-- shows the three cells side by side in real retail text. LEGAIA_POKE=0 runs
-- the same boot unmodified (the baseline frame).
--
-- The breakpoint address is the French executable's. The other PAL builds
-- carry the same renderer body at other addresses: find the instruction after
-- the preprocessor `jal` in that image and pass it as LEGAIA_GLYPH_BP (the
-- buffer is read from s0 there, so no buffer address is needed).
--
-- Launch (cold boot; stage a SCRATCH copy of the PAL .bin - never the NAS path):
--   LEGAIA_NO_SSTATE=1 LEGAIA_FASTBOOT=1 LEGAIA_FRAMES=2400 LEGAIA_FB_EVERY=100 \
--   timeout 900 bash scripts/pcsx-redux/run_probe.sh --isolate-config \
--     --iso <scratch>/fr.bin --lua scripts/pcsx-redux/autorun_pal_glyph_cells.lua \
--     --out-dir <scratch>/pal_glyph
--
-- Outputs: strings.csv (tick, pc, ra, buffer hex before the poke), fb_*.screen
-- (+ .meta; decode with scripts/pcsx-redux/decode_pcsx_screen.py), summary.txt.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GLYPH_BP   = probe.getenv_num("LEGAIA_GLYPH_BP", 0x800370F0)
local POKE       = probe.getenv("LEGAIA_POKE", "1") == "1"
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 2400)
local FB_EVERY   = probe.getenv_num("LEGAIA_FB_EVERY", 100)
local HOLD       = probe.getenv_num("LEGAIA_PAD_HOLD", 6)
local PAD_RAW    = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local MAX_ROWS   = probe.getenv_num("LEGAIA_MAX_ROWS", 400)
local POKE_BYTES = { 0xD7, 0xF8, 0xDD }

require("probe.sstate").load = function(_)
    PCSX.log("[pal-glyph] cold boot; sstate load skipped")
    return true
end

local PAD = {}
for entry in string.gmatch(PAD_RAW, "[^,]+") do
    local t, name = string.match(entry, "^%s*(%d+)%s*:%s*(%a+)%s*$")
    if t ~= nil and probe.BTN[string.upper(name)] ~= nil then
        PAD[#PAD + 1] = { tick = tonumber(t), bit = probe.BTN[string.upper(name)] }
    end
end

local strings_csv = probe.csv_open(probe.out_path("strings.csv"), "tick,pc,ra,buf,hex")
local g_tick, hits, poked, rows = 0, 0, 0, 0
local seen = {}

local function grab_fb(tag)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return end
    local data = tostring(ss.data)
    local w, h = tonumber(ss.width), tonumber(ss.height)
    -- takeScreenShot's bpp field is unreliable (it reads 16 over 24-bit
    -- MDEC frames); the byte count is not.
    local bpp = (#data >= w * h * 3) and 24 or 16
    local fh = io.open(probe.out_path(string.format("fb_%s.screen", tag)), "wb")
    if fh == nil then return end
    fh:write(data); fh:close()
    local mh = io.open(probe.out_path(string.format("fb_%s.screen.meta", tag)), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
            w, h, bpp, bpp / 8))
        mh:close()
    end
end

probe.run({
    sstate = "unused",
    capture_frames = FRAMES,
    snapshot_path = probe.out_path("pal_glyph.snapshot.txt"),
    on_arm = function()
        local d = { addr = GLYPH_BP, name = "pal_text_after_preprocess",
                    hits_ref = { n = 0 } }
        probe.arm_breakpoint(GLYPH_BP, "Exec", 4, d.name, function()
            local r = PCSX.getRegisters()
            local gp = r.GPR.n
            local buf = bit.band(tonumber(gp.s0), 0xFFFFFFFF)
            hits = hits + 1
            d.hits_ref.n = d.hits_ref.n + 1
            local raw = probe.read_bytes(buf, 64)
            local hex = raw and probe.bytes_to_hex(raw) or ""
            local key = string.sub(hex, 1, 64)
            if rows < MAX_ROWS and not seen[key] then
                seen[key] = true
                rows = rows + 1
                strings_csv:row("%d,0x%08X,0x%08X,0x%08X,%s", g_tick,
                    bit.band(tonumber(r.pc), 0xFFFFFFFF),
                    bit.band(tonumber(gp.ra), 0xFFFFFFFF), buf, hex)
            end
            if not POKE then return end
            local i, n = 0, 1
            while i < 64 and n <= #POKE_BYTES do
                local b = probe.read_u8(buf + i)
                if b == nil or b == 0 then break end
                if b >= 0xC0 and b <= 0xCF then
                    i = i + 2
                else
                    if (b >= 0x41 and b <= 0x5A) or (b >= 0x61 and b <= 0x7A) then
                        probe.write_u8(buf + i, POKE_BYTES[n])
                        n = n + 1
                    end
                    i = i + 1
                end
            end
            if n > 1 then poked = poked + 1 end
        end)
        return { d }
    end,
    on_capture = function(_, elapsed)
        g_tick = elapsed
        for _, s in ipairs(PAD) do
            if elapsed == s.tick then probe.pad_force(s.bit)
            elseif elapsed == s.tick + HOLD then probe.pad_release(s.bit) end
        end
        if FB_EVERY > 0 and elapsed % FB_EVERY == 0 then
            grab_fb(string.format("%05d", elapsed))
        end
    end,
    on_summary = function()
        PCSX.log("=== probe hits ===")
        PCSX.log(string.format("  text draws: %d, poked: %d, distinct strings: %d",
            hits, poked, rows))
        PCSX.log("=== end ===")
    end,
    on_done = function()
        local fh = io.open(probe.out_path("summary.txt"), "w")
        if fh then
            fh:write(string.format("bp=0x%08X poke=%s draws=%d poked=%d distinct=%d\n",
                GLYPH_BP, tostring(POKE), hits, poked, rows))
            fh:close()
        end
        strings_csv:close()
    end,
})
