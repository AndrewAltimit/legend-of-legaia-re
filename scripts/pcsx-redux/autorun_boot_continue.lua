-- autorun_boot_continue.lua
--
-- Cold-boot the disc under test, navigate title -> CONTINUE -> memory
-- card load screen -> load a save, checkpoint at field-run. This is the
-- ONLY faithful session anchor for testing disc content that loads once
-- per session (the player battle files among it): a save state carries
-- the resident copies of ITS OWN disc era, and a fresh encounter does
-- NOT re-stream them - a state-based battle silently fights the models
-- of whatever disc the state was made on.
--
-- Run under the RECOMPILER with -fastboot (vsync events deliver through
-- the title XA there, and the interpreter+debugger cold-boot segfault
-- never arises):
--   pcsx-redux -dynarec -fastboot -bios <bios> -iso <disc> -run -stdout --       -dofile scripts/pcsx-redux/autorun_boot_continue.lua
--
-- Input model: mash START+DOWN (never CROSS - START confirms Continue
-- once the menu is up, DOWN keeps the cursor off NEW GAME) until
-- LEGAIA_MASH_UNTIL, then the typed LEGAIA_SEQ ("tick:BTN,...") single
-- presses. Calibrated sequence for save slot No.1 on memory card 1
-- (vsync clock, -fastboot):
--   LEGAIA_MASH_UNTIL=1500
--   LEGAIA_SEQ="2400:CROSS,3400:CROSS,4200:CROSS,5000:CROSS,5600:UP,5900:CROSS"
-- (CROSS: slot picker -> save list -> preview -> Yes/No dialog; the
-- dialog defaults to No - the UP moves to Yes.) Re-calibrate with
-- LEGAIA_CAL_CKPTS="t1,t2,..." raw checkpoints, inspected by gzipping
-- and loading each in a --fast peek probe (BP-context GPU screenshots
-- segfault this build - never screenshot from a tick here).
-- On mode 3 (field-run) + LEGAIA_SETTLE ticks, writes
-- <out>/<LEGAIA_CKPT_LABEL>.rawsstate; host-gzip it into a loadable
-- .sstate for the fast e2e battle stages.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local bp  = require("probe.bp")

local GM = 0x8007B83C
local SCENE_NAME = 0x8007050C
local OUT_DIR   = env.getenv("LEGAIA_OUT_DIR", "captures/bootcont")
local CONFIRM_AT = tonumber(env.getenv("LEGAIA_CONFIRM_AT", "0")) or 0
local SHOT_EVERY = tonumber(env.getenv("LEGAIA_SHOT_EVERY", "200")) or 200
local MAX_TICKS  = tonumber(env.getenv("LEGAIA_MAX_TICKS", "9000")) or 9000
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "40")) or 40
local LABEL      = env.getenv("LEGAIA_CKPT_LABEL", "boot_continue")
local cal = {}
for tok in string.gmatch(env.getenv("LEGAIA_CAL_CKPTS", ""), "[^,%s]+") do
    cal[#cal+1] = tonumber(tok)
end
-- LEGAIA_SEQ: "tick:BTN,tick:BTN,..." single presses after the mash phase.
local BTN = { UP=4, RIGHT=5, DOWN=6, LEFT=7, START=3, TRI=12, CIRC=13, CROSS=14 }
local seq = {}
for tok in string.gmatch(env.getenv("LEGAIA_SEQ", ""), "[^,%s]+") do
    local t, b = string.match(tok, "(%d+):(%a+)")
    if t and BTN[b] then seq[#seq+1] = { tick = tonumber(t), btn = BTN[b], name = b } end
end
local MASH_UNTIL = tonumber(env.getenv("LEGAIA_MASH_UNTIL", "0")) or 0
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/boot.log", "w")
local function log(s) PCSX.log("[boot] "..s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function shot(stem)
    do return end -- exec-BP-context GPU screenshot segfaults; use checkpoints
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(OUT_DIR.."/"..stem..".raw","wb"); if fh==nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(OUT_DIR.."/"..stem..".raw.meta","w")
    if mh then mh:write(string.format("width=%d\nheight=%d\nbpp=16\n", ss.width or 320, ss.height or 228)); mh:close() end
end
local function read_scene()
    local s = {}
    for i=0,7 do local b=mem.read_u8(SCENE_NAME+i) or 0
        if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function checkpoint()
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(OUT_DIR.."/"..LABEL..".rawsstate", "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log("checkpoint "..tostring(ok).." "..tostring(err))
    return ok
end

local tick = 0
local last_mode = -1
local settled = 0
local done = false
local function on_tick()
    if done then return end
    tick = tick + 1
    local m = mem.read_u8(GM) or 255
    if m ~= last_mode then
        log(string.format("tick %d: mode 0x%02X scene=%s", tick, m, read_scene()))
        shot(string.format("mode_%02X_t%d", m, tick))
        last_mode = m
    end
    for _, t in ipairs(cal) do
        if tick == t then
            local ok = pcall(function()
                local w = PCSX.createSaveState()
                local fh = Support.File.open(OUT_DIR.."/cal_t"..t..".rawsstate", "CREATE")
                fh:writeMoveSlice(w); fh:close()
            end)
            log("cal checkpoint t"..t.." "..tostring(ok))
        end
    end
    -- title phase input: START+DOWN pulses (never CROSS) until MASH_UNTIL,
    -- then the typed LEGAIA_SEQ presses.
    if MASH_UNTIL == 0 or tick < MASH_UNTIL then
        local ph = tick % 30
        if ph == 0 then pad.force(pad.BTN.START); pad.force(pad.BTN.DOWN)
        elseif ph == 8 then pad.release(pad.BTN.START); pad.release(pad.BTN.DOWN) end
    elseif tick == MASH_UNTIL then
        pad.release(pad.BTN.START); pad.release(pad.BTN.DOWN)
    else
        for _, e in ipairs(seq) do
            if tick == e.tick then pad.force(e.btn); log("seq "..e.name.." at "..tick)
            elseif tick == e.tick + 8 then pad.release(e.btn) end
        end
    end
    -- target: field-run mode 3 after the load
    if MASH_UNTIL > 0 and tick > MASH_UNTIL and m == 3 then
        settled = settled + 1
        if settled >= SETTLE then
            done = true
            log(string.format("field-run reached, scene=%s; checkpointing", read_scene()))
            shot("field_reached_t"..tick)
            checkpoint()
            PCSX.quit(0)
        end
    else
        settled = 0
    end
    if tick >= MAX_TICKS then
        done = true; log("max ticks; quitting"); shot("timeout_t"..tick); PCSX.quit(0)
    end
end

-- Vsync-driven (recompiler-safe): under --fast the GPU::Vsync events
-- deliver through the title XA (verified by the --fast peek runs), and
-- the recompiler avoids the debugger-mode cold-boot segfault entirely.
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_tick)
log("vsync driver armed; waiting for boot")
