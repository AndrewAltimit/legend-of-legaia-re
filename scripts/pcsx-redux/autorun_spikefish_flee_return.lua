-- autorun_spikefish_flee_return.lua - does a Spikefish Flute escape hang
-- the return to the field, and does it depend on the water flag byte?
--
-- Backs docs/reference/glitches/field-oddities.md ("The Spikefish Flute
-- freeze in Bio Castle"). Starts from a field state (game mode 0x03). What
-- is POKED, once, LEGAIA_DELAY vsyncs after the state runs in the field:
--   * bits 0x0E of byte 0x8008575B (system flags 0x1C / 0x1D / 0x1E)
--     replaced by LEGAIA_BYTE (bit 0x01, flag 0x1F, is left alone)
--   * bag slot 0 = Spikefish Flute (item 0x99) x9
--   * the step counter 0x8007B5FC = 1, so the next rolling step starts a
--     random battle
--   * optionally (LEGAIA_FORM) the region's formation rows, see below
-- Everything after that is the game's own code driven by the pad:
--   LEGAIA_ESCAPE=flute  Begin, then every member picks Item -> slot 0;
--                        the first flute to act flees (PROT 0925 arm 8)
--   LEGAIA_ESCAPE=run    Run (RIGHT) -> confirm (LEFT), repeated until the roll succeeds
-- Logged on change (emulator log, [st] lines): game mode 0x8007B83C, the
-- return marker 0x8007B8B8, DAT_8007BD60, the loader-B id 0x8007BC4C, the
-- flag byte, and the battle flow / action state bytes. Once the game mode
-- has not changed for 300 vsyncs outside the field (2400 in battle), [pc] lines sample
-- pc / ra / sp every 20 vsyncs (the hang site). Back in field mode 0x03
-- for 300 vsyncs = no hang, and the run quits early. Screenshot every 150
-- vsyncs into the output directory. At action state 0x65 (escape wait) a
-- raw save state escape_wait.rawsstate is written (gzip it to load it), and
-- from state 0x67 on [pcv] lines log pc / ra every vsync until the field.
--
-- Poll-only (no breakpoints) - run it --fast:
--   LEGAIA_BYTE=0x0A LEGAIA_ESCAPE=flute timeout 900 xvfb-run -a \
--     bash scripts/pcsx-redux/run_probe.sh --fast --iso "$LEGAIA_DISC_BIN" \
--     --sstate <jouind field state> \
--     --lua scripts/pcsx-redux/autorun_spikefish_flee_return.lua
-- The Bio Castle state is a cold boot of the retail disc plus a card load
-- of playthrough-endgame-7saves.mcr block 6 (scene jouind), made with
-- autorun_boot_continue.lua.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local pad   = require("probe.pad")

local GM, RET, BD60, LOADB = 0x8007B83C, 0x8007B8B8, 0x8007BD60, 0x8007BC4C
local BYTE, STEP, INV, CTXP = 0x8008575B, 0x8007B5FC, 0x80085958, 0x8007BD24

-- PROT 0978's field-back read: buffer pointer and sector count, and the
-- first word of the side-band buffer's caption string (0x800B995C in the
-- captured fights)
local FBR_BUF, FBR_SEC = 0x8007B8C4, 0x8007B6FC
local CAPTION = probe.getenv_num("LEGAIA_CAPTION", 0x800B995C)
local VAL    = probe.getenv_num("LEGAIA_BYTE", 0x0A)
-- LEGAIA_FORM="n:id,id,id": rewrite every resident formation row that
-- names LEGAIA_FORM_FROM (default 0xA1, Gilium Lv3) to n monsters of these
-- ids, so the forced battle is a chosen formation (jouind's random rows
-- are 3x 0x9E and 3x 0xA1)
local FORM      = probe.getenv("LEGAIA_FORM", "")
local FORM_FROM = probe.getenv_num("LEGAIA_FORM_FROM", 0xA1)
local MODE   = probe.getenv("LEGAIA_ESCAPE", "flute")
local DELAY  = probe.getenv_num("LEGAIA_DELAY", 10)
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)

local function shot(stem)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local h = io.open(probe.out_path(stem .. ".raw"), "wb")
    if h then h:write(tostring(ss.data)); h:close() end
    local m = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if m then
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height), bpp))
        m:close()
    end
end

local last_line, last_mode, mode_since = nil, -1, 0
local poked, battled, back_in_field = false, false, 0
local held, held_at, wait_until = nil, 0, 0
local dirs = { pad.BTN.UP, pad.BTN.RIGHT, pad.BTN.DOWN, pad.BTN.LEFT }

local ckpt_done, tail = false, false
local function checkpoint(stem)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(stem .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    PCSX.log(string.format("[ckpt] %s %s %s", stem, tostring(ok), tostring(err)))
end

local function press(b, el) pad.force(b); held = b; held_at = el end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(ctx, el)
        local gm = mem.read_u8(GM) or 0xFF
        if gm ~= last_mode then last_mode = gm; mode_since = el end
        local c = mem.read_u32(CTXP) or 0
        local flow, st = -1, -1
        if gm == 0x15 and c >= 0x80000000 and c < 0x80200000 then
            flow, st = mem.read_u8(c + 6) or 0, mem.read_u8(c + 7) or 0
        end
        local line = string.format(
            "gm=%02x ret=%d bd60=%02x loadB=%d byte=%02x flow=%02x st=%02x"
            .. " fbr_buf=%08x fbr_sec=%d cap=%08x",
            gm, mem.read_u32(RET) or -1, mem.read_u8(BD60) or 0,
            mem.read_u32(LOADB) or -1, mem.read_u8(BYTE) or 0,
            flow % 256, st % 256, mem.read_u32(FBR_BUF) or 0,
            mem.read_u32(FBR_SEC) or 0, mem.read_u32(CAPTION) or 0)
        if line ~= last_line then
            PCSX.log(string.format("[st] f=%d %s", el, line)); last_line = line
        end
        if el % 150 == 0 then shot(string.format("frame_%05d", el)) end
        -- Checkpoint at the escape wait (state 0x65) so the return can be
        -- replayed from just before it; then log pc / ra every vsync from
        -- the teardown on, so the last line before an emulator stop names
        -- where the CPU was.
        if st == 0x65 and not ckpt_done then ckpt_done = true; checkpoint("escape_wait") end
        if st == 0x67 then tail = true end
        if tail and back_in_field == 0 then
            local r = PCSX.getRegisters()
            PCSX.log(string.format("[pcv] f=%d gm=%02x pc=%08x ra=%08x",
                el, gm, tonumber(r.pc), tonumber(r.GPR.n.ra)))
        end
        if held and el >= held_at + 4 then pad.release(held); held = nil end

        if not poked and gm == 0x03 and el >= DELAY then
            poked = true
            local b = mem.read_u8(BYTE) or 0
            mem.write_u8(BYTE, bit.bor(bit.band(b, 0xF1), VAL))
            mem.write_u8(INV, 0x99); mem.write_u8(INV + 1, 9)
            mem.write_u32(STEP, 1)
            if FORM ~= "" then
                local n, list = string.match(FORM, "(%d+):(.*)")
                local ids = {}
                for t in string.gmatch(list or "", "[^,]+") do ids[#ids + 1] = tonumber(t) end
                local hits = 0
                for base = 0x80000000, 0x801F0000, 0x10000 do
                    local blob = mem.read_bytes(base, 0x10000)
                    local str = blob and tostring(blob) or ""
                    local pat = string.char(0, 0, 0, 3, FORM_FROM, FORM_FROM, FORM_FROM, 0)
                    local i = string.find(str, pat, 1, true)
                    while i do
                        local a = base + i - 1
                        mem.write_u8(a + 3, tonumber(n))
                        for k = 0, 3 do mem.write_u8(a + 4 + k, ids[k + 1] or 0) end
                        hits = hits + 1
                        PCSX.log(string.format("[form] row at %08x -> %s", a, FORM))
                        i = string.find(str, pat, i + 1, true)
                    end
                end
                -- the other random row of the region too (3x 0x9E in jouind)
                for base = 0x80000000, 0x801F0000, 0x10000 do
                    local blob = mem.read_bytes(base, 0x10000)
                    local str = blob and tostring(blob) or ""
                    local pat = string.char(0, 0, 0, 3, 0x9E, 0x9E, 0x9E, 0)
                    local i = string.find(str, pat, 1, true)
                    while i and FORM_FROM == 0xA1 do
                        local a = base + i - 1
                        mem.write_u8(a + 3, tonumber(n))
                        for k = 0, 3 do mem.write_u8(a + 4 + k, ids[k + 1] or 0) end
                        PCSX.log(string.format("[form] row at %08x -> %s", a, FORM))
                        i = string.find(str, pat, i + 1, true)
                    end
                end
                if hits == 0 then PCSX.log("[form] no row found") end
            end
            PCSX.log(string.format("[poke] f=%d byte %02x -> %02x, bag0=99 x9, step=1",
                el, b, mem.read_u8(BYTE) or 0))
        end

        if gm == 0x03 and poked and not battled then
            -- walk until the roll starts a battle
            if el % 40 == 0 then
                if held then pad.release(held) end
                held = dirs[math.floor(el / 40) % 4 + 1]; pad.force(held); held_at = el + 30
            end
        elseif gm == 0x15 and flow >= 0 then
            battled = true
            if not held and el >= wait_until then
                local B = pad.BTN
                if MODE == "run" then
                    if flow == 0x1E then press(B.RIGHT, el); wait_until = el + 30
                    elseif flow == 0x32 then press(B.LEFT, el); wait_until = el + 30 end
                else
                    if flow == 0x1E then press(B.LEFT, el); wait_until = el + 30
                    elseif flow == 0x28 then press(B.UP, el); wait_until = el + 30
                    elseif flow == 0x3C or (flow >= 0x5B and flow <= 0x5E)
                        or flow == 0x64 or flow == 0x6E then
                        press(B.CROSS, el); wait_until = el + 30
                    end
                end
            end
        end

        if battled and gm == 0x03 then
            back_in_field = back_in_field + 1
            if back_in_field == 1 then shot("back_in_field") end
            if back_in_field >= 300 then
                PCSX.log(string.format("[result] f=%d back in field, no hang", el))
                ctx.request_quit = true
            end
        end
        local dur = el - mode_since
        local stuck_after = (gm == 0x15) and 2400 or 300
        if battled and gm ~= 0x03 and dur >= stuck_after and el % 20 == 0 then
            local r = PCSX.getRegisters()
            PCSX.log(string.format("[pc] f=%d gm=%02x pc=%08x ra=%08x sp=%08x",
                el, gm, tonumber(r.pc), tonumber(r.GPR.n.ra), tonumber(r.GPR.n.sp)))
            if dur < stuck_after + 20 then shot("stuck") end
        end
        if el >= FRAMES - 2 then
            PCSX.log(string.format("[result] f=%d out of frames, gm=%02x", el, gm))
            ctx.request_quit = true
        end
    end,
})
