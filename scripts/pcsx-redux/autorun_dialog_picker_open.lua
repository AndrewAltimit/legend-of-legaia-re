-- autorun_dialog_picker_open.lua
--
-- Retail trace of the field pager (`FUN_801D84D0`, PROT 0897) around a
-- page that ends on a picker open byte (`0x27` / `0x28` / `0x29` / `0x2A`),
-- from a state whose conversation is already up
-- (e.g. `retock_innkeeper_talk_open`). The question: does the picker open as
-- soon as the prompt's page is shown, or only on the confirm press the page
-- waits for in state `0x19` (`0x801D8FD8..0x801D909C`)? And the automatic
-- press: while `_DAT_80073F00 > 0`, state `0x19` counts it down by the frame
-- step and presses for the player when it reaches zero (`0x801D8F4C..
-- 0x801D8F88`).
--
-- Per vsync, one CSV row whenever something changes:
--   v, dt, state (_DAT_801F2734), auto (_DAT_80073F00), cursor
--   (*(_DAT_801C6EA4) + 0xC), press, then the pager actor's picker slide
--   fields: count (+0x54), span (+0x50), target x/y (+0x14/+0x16), start x/y
--   (+0x3C/+0x3E), w/h (+0x24/+0x26), and the box origin the draw computes
--   from them (draw_x/draw_y, `0x801D9A20..0x801D9ADC`: target +
--   (start - target) * count / span; the box is skipped while count is the
--   `0x309` sentinel, and at the target once count is 0).
-- The pager actor is the one whose tick word (+0x0C) is 0x801D84D0 on the
-- actor lists at 0x8007C34C.. (spawned from the template at 0x801F2760).
--
-- Press schedule: CROSS is tapped (held 4 vsyncs) LEGAIA_PICKER_WAIT vsyncs
-- after the pager reaches state 0x19 or a picker input state (0x12 / 0x14 /
-- 0x16 / 0x18), up to LEGAIA_PICKER_PRESSES times in all; with
-- LEGAIA_PICKER_RETALK=N it also taps after N vsyncs idle in state 1, to open
-- the NPC's next talk. LEGAIA_PICKER_AUTO=N instead pokes _DAT_80073F00 = N on the first
-- 0x19 vsync and presses nothing, to watch the automatic press.
--
-- Vsync-driven, no breakpoints: run with --fast. Output <OUT_DIR>/picker.csv.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 400)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/dialog_picker_open")
local WAIT = probe.getenv_num("LEGAIA_PICKER_WAIT", 30)
local PRESSES = probe.getenv_num("LEGAIA_PICKER_PRESSES", 1)
local AUTO = probe.getenv_num("LEGAIA_PICKER_AUTO", 0)
local RETALK = probe.getenv_num("LEGAIA_PICKER_RETALK", 0)
local SHOTS = probe.getenv_num("LEGAIA_PICKER_SHOTS", 0)
local shots_taken = 0
local CKPT = probe.getenv_num("LEGAIA_PICKER_CKPT", 0)
local ckpt_done, last19, prev_st = false, nil, nil

-- LEGAIA_PICKER_CKPT=1: checkpoint the state (raw PCSX-Redux sstate; gzip it
-- on the host) on every entry into the page wait 0x19, and keep the one that
-- precedes the first picker open state (0x11 / 0x13 / 0x15 / 0x17) as
-- <OUT_DIR>/picker_prompt.rawsstate - a state parked on the prompt page, one
-- press short of the menu.
local function checkpoint(path)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(path, "TRUNCATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    PCSX.log(string.format("[picker] checkpoint %s ok=%s %s", path, tostring(ok), tostring(err or "")))
end
-- LEGAIA_PICKER_SHOTS=N: screenshot (raw + .raw.meta, for raw2png.py) on
-- each changed row while the pager is in a picker state (0x11..0x18), up to
-- N frames - the drawn box against the draw_x/draw_y column.
local function shot(v)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not (ok and ss) then return end
    local raw = tostring(ss.data)
    local base = string.format("%s/shot_%05d.raw", OUT_DIR, v)
    local h = io.open(base, "wb"); h:write(raw); h:close()
    local w, hh = tonumber(ss.width), tonumber(ss.height)
    local m = io.open(base .. ".meta", "w")
    m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", w, hh, (#raw >= w * hh * 3) and 24 or 16))
    m:close()
end
os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = io.open(OUT_DIR .. "/picker.csv", "w")
CSV:write("v,dt,state,auto,cursor,press,count,span,tx,ty,sx,sy,w,h,draw_x,draw_y\n")

local function s16(v) v = v or 0; if v >= 0x8000 then return v - 0x10000 end; return v end

local function find_pager()
    for head = 0x8007C34C, 0x8007C368, 4 do
        local a, n = probe.read_u32(head) or 0, 0
        while a >= 0x80000000 and a < 0x80200000 and n < 256 do
            if probe.read_u32(a + 0x0C) == 0x801D84D0 then return a end
            a = probe.read_u32(a) or 0
            n = n + 1
        end
    end
    return nil
end

local function slide()
    local a = find_pager()
    if not a then return "-,-,-,-,-,-,-,-,-,-" end
    local cnt, span = s16(probe.read_u16(a + 0x54)), s16(probe.read_u16(a + 0x50))
    local tx, ty = s16(probe.read_u16(a + 0x14)), s16(probe.read_u16(a + 0x16))
    local sx, sy = s16(probe.read_u16(a + 0x3C)), s16(probe.read_u16(a + 0x3E))
    local w, h = s16(probe.read_u16(a + 0x24)), s16(probe.read_u16(a + 0x26))
    local dx, dy = tx, ty
    local function tdiv(p, q) local r = p / q; return r >= 0 and math.floor(r) or math.ceil(r) end
    if cnt == 0x309 then dx, dy = "none", "none"
    elseif cnt ~= 0 and span ~= 0 then
        dx = tx + tdiv((sx - tx) * cnt, span)
        dy = ty + tdiv((sy - ty) * cnt, span)
    end
    return string.format("%d,%d,%d,%d,%d,%d,%d,%d,%s,%s", cnt, span, tx, ty, sx, sy, w, h, dx, dy)
end

local wait_from, idle_from = nil, nil
local first19, pressed, release_v, held, last = nil, 0, nil, false, nil
probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(_ctx, v)
        local st = probe.read_u32(0x801F2734) or 0
        local dt = probe.read_scratch_u8(0x1F800393)
        local ctx = probe.read_u32(0x801C6EA4) or 0
        local cur = 0
        if ctx >= 0x80000000 and ctx < 0x80200000 then cur = probe.read_u32(ctx + 0xC) or 0 end
        if st == 0x19 and first19 == nil then
            first19 = v
            if AUTO > 0 then
                probe.write_u16(0x80073F00, AUTO)
            end
        end
        if release_v and v >= release_v then pad.release(pad.BTN.CROSS); held = false; release_v = nil end
        -- A picker state that takes input (0x12/0x14/0x16/0x18) is pressed
        -- like a page wait, so the choice commits and the talk goes on.
        local waiting = st == 0x19 or st == 0x12 or st == 0x14 or st == 0x16 or st == 0x18
        if CKPT > 0 and not ckpt_done then
            -- One file per checkpoint, the kept one renamed at the end. Each
            -- is opened "TRUNCATE": "CREATE" is O_CREAT without O_TRUNC, so a
            -- rewrite over a longer earlier state keeps its tail and the
            -- result will not load.
            if st == 0x19 and prev_st ~= 0x19 then
                if last19 then os.remove(last19) end
                last19 = string.format("%s/wait19_%05d.rawsstate", OUT_DIR, v)
                checkpoint(last19)
            end
            if last19 and (st == 0x11 or st == 0x13 or st == 0x15 or st == 0x17) then
                os.remove(OUT_DIR .. "/picker_prompt.rawsstate")
                os.rename(last19, OUT_DIR .. "/picker_prompt.rawsstate")
                ckpt_done = true
                PCSX.log(string.format("[picker] v%d: state %X, prompt state kept", v, st))
            end
        end
        prev_st = st
        if waiting and wait_from == nil then wait_from = v end
        if not waiting then wait_from = nil end
        if AUTO == 0 and first19 and pressed < PRESSES and not held
            and waiting and v - wait_from >= WAIT then
            pad.force(pad.BTN.CROSS); held = true; release_v = v + 4; pressed = pressed + 1
            wait_from = nil -- a later wait waits WAIT again
        end
        -- LEGAIA_PICKER_RETALK=N: after N vsyncs with the pager idle in
        -- state 1 (the talk over, the player still facing the NPC), tap CROSS
        -- to talk again - the second talk of an innkeeper is the one that
        -- asks.
        if st == 1 then idle_from = idle_from or v else idle_from = nil end
        if RETALK > 0 and idle_from and not held and pressed < PRESSES
            and v - idle_from >= RETALK then
            pad.force(pad.BTN.CROSS); held = true; release_v = v + 4; pressed = pressed + 1
            idle_from = nil
        end
        local auto = s16(probe.read_u16(0x80073F00))
        local key = string.format("%d,%X,%d,%d,%d,%s", dt, st, auto, cur, held and 1 or 0, slide())
        if key ~= last then
            CSV:write(string.format("%d,%s\n", v, key))
            CSV:flush()
            if SHOTS > shots_taken and st >= 0x11 and st <= 0x18 then
                shot(v); shots_taken = shots_taken + 1
            end
            last = key
        end
    end,
    on_done = function()
        pad.release(pad.BTN.CROSS)
        CSV:close()
        PCSX.log("=== dialog_picker_open done ===")
    end,
})
