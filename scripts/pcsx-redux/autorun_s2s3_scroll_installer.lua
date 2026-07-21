-- autorun_s2s3_scroll_installer.lua
--
-- FINDING (clean negative that redirected the thread): across the whole
-- s2 -> s3 name-entry replay, NEITHER wrap-scroll installer opcode body
-- (0x1E / 0x45) executes and NO dispatch-4 tick fires, yet the F-variant
-- flip reproduces - so the extraction-0874 residue is NOT a parked
-- wrap-scroll phase. The successor probe autorun_s2s3_atlas_stamp.lua
-- pinned the true writer: the field-VM `4C 60` literal MoveImage stamp in
-- town01 MAN P2[3] (the opening timeline record). This probe remains the
-- template for trapping the move-VM scroll installers.
--
-- Original goal: name the installing event of the extraction-0874 s2
-- F-variant record: replay the s2 -> s3 name-entry walk (same phase
-- machine as autorun_s3_capture.lua) and catch, in the same run:
--
--   * exec 0x80023694 - move-VM opcode 0x1E body (JT 0x80010778[0x1E]), the
--     dispatch-4 VRAM wrap-scroll installer: `actor[+0x5A]=4` + 7 operand
--     u16s -> +0xC4 (reload), +0xCC/+0xCE (step), +0xD0..+0xD6 (rect).
--     Logs actor (s2), op ptr (s0), the operands, the actor's move-buffer
--     base +0x48 / PC +0x70, and the buffer's delta against the three
--     known record roots (_DAT_8007B8D0 prescript stager table,
--     _DAT_8007B888 MOVE, _DAT_8007B840 MOVE2).
--   * exec 0x8002409C - opcode 0x45 body, the dispatch-7 sibling (same rect
--     register file, swapped operand order).
--   * exec 0x80022CB8 - FUN_80021DF4's dispatch-4 arm entry: first tick per
--     unique scroll actor, rect logged; rect x in [832,880) flagged (the
--     Noa strip of the 0874 s2 atlas is (852,256) 20x128).
--   * exec 0x80021B04 (part spawn; a2 = stager record ptr) and 0x800252EC
--     (prescript stager install; a0 = record id) for spawn provenance.
--   * PCSX.getVRAM() poll of row 271 x 851..859 every vsync - timestamps
--     the F-variant flip against the install/tick events.
--
--   timeout --kill-after=60s 3000s \
--       bash scripts/pcsx-redux/run_probe.sh \
--       --scenario s2_rimelm_town01 \
--       --lua scripts/pcsx-redux/autorun_s2s3_scroll_installer.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local CURSOR     = 0x8007BB88
local PLAYER     = 0x8007C364
local SCENE_NAME = 0x8007050C

local OP1E_BODY  = 0x80023694
local OP45_BODY  = 0x8002409C
local DISP4_ARM  = 0x80022CB8
local SPAWN_FN   = 0x80021B04
local STAGER_FN  = 0x800252EC

local ROOT_STAGER = 0x8007B8D0
local ROOT_MOVE   = 0x8007B888
local ROOT_MOVE2  = 0x8007B840

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s2s3_scroll")
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "4200")) or 4200
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/installer.log", "w")
local function log(s) PCSX.log("[scr] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end

local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function reg(r, name)
    local v = tonumber(r.GPR.n[name]) or 0
    if v < 0 then v = v + 0x100000000 end
    return bit.band(v, 0xFFFFFFFF)
end
local function read_scene()
    local s = {}
    for i = 0, 7 do local b = ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function engaged()
    local pp = ru32(PLAYER); if pp == nil then return nil end
    local fl = ru32(pp+0x10); if fl == nil then return nil end
    return math.floor(fl/0x80000) % 2 == 1
end

local vsync, loaded = 0, false

-- ---------------- VRAM flip watch (row 271, x 851..859) ----------------
local ROW_BYTES = 2048
local function get_vram()
    local ok, data = pcall(function()
        if PCSX.getVRAM ~= nil then return PCSX.getVRAM() end
        if PCSX.GPU and PCSX.GPU.getVRAM then return PCSX.GPU.getVRAM() end
        return nil
    end)
    if not ok or data == nil then return nil end
    return tostring(data)
end
local function band271(vram)
    return vram:sub(271 * ROW_BYTES + 851 * 2 + 1, 271 * ROW_BYTES + 859 * 2)
end
local band0 = nil
local flip_logged = false

-- ---------------- breakpoint state ----------------
local seen_disp4 = {}
local spawn_hits, stager_hits = 0, 0
local install_hits = 0

local function hexdump16(addr, count)
    local out = {}
    for i = 0, count - 1 do
        out[#out+1] = string.format("%04X", ru16(addr + i*2) or 0)
    end
    return table.concat(out, " ")
end

local function log_install(tag)
    local r = PCSX.getRegisters()
    local actor = reg(r, "s2")
    local opptr = reg(r, "s0")
    install_hits = install_hits + 1
    local buf = ru32(actor + 0x48) or 0
    local pc  = ru16(actor + 0x70) or 0
    local stager = ru32(ROOT_STAGER) or 0
    local move   = ru32(ROOT_MOVE) or 0
    local move2  = ru32(ROOT_MOVE2) or 0
    log(string.format(
        "[v%d] %s INSTALL actor=0x%08X opptr=0x%08X ops=[%s] buf=0x%08X vmpc=0x%04X " ..
        "d_stager=0x%X d_move=0x%X d_move2=0x%X",
        vsync, tag, actor, opptr, hexdump16(opptr, 8), buf, pc,
        bit.band(opptr - stager, 0xFFFFFFFF),
        bit.band(opptr - move, 0xFFFFFFFF),
        bit.band(opptr - move2, 0xFFFFFFFF)))
    -- context dump: the record around the opcode, for disc matching
    log(string.format("    ctx -0x20: %s", hexdump16(opptr - 0x20, 16)))
    log(string.format("    ctx +0x00: %s", hexdump16(opptr, 16)))
end

bp.arm(OP1E_BODY, "Exec", 4, "op1e_install", function() log_install("op1E") end)
bp.arm(OP45_BODY, "Exec", 4, "op45_install", function() log_install("op45") end)

bp.arm(DISP4_ARM, "Exec", 4, "disp4_tick", function()
    local r = PCSX.getRegisters()
    local actor = reg(r, "s5")
    if (ru16(actor + 0x5A) or 0) ~= 4 then return end
    if seen_disp4[actor] then return end
    seen_disp4[actor] = true
    local x = ru16(actor + 0xD0) or 0
    local y = ru16(actor + 0xD2) or 0
    local w = ru16(actor + 0xD4) or 0
    local h = ru16(actor + 0xD6) or 0
    local noa = (x >= 832 and x < 880) and "  <<< ATLAS BAND" or ""
    log(string.format(
        "[v%d] disp4 tick actor=0x%08X rect=(%d,%d,%d,%d) step=(%d,%d) reload=%d buf=0x%08X%s",
        vsync, actor, x, y, w, h,
        ru16(actor + 0xCC) or 0, ru16(actor + 0xCE) or 0,
        ru16(actor + 0xC4) or 0, ru32(actor + 0x48) or 0, noa))
end)

bp.arm(SPAWN_FN, "Exec", 4, "part_spawn", function()
    if spawn_hits >= 400 then return end
    spawn_hits = spawn_hits + 1
    local r = PCSX.getRegisters()
    local rec = reg(r, "a2")
    log(string.format("[v%d] spawn FUN_80021B04 a0=0x%X a1=0x%X rec=0x%08X ra=0x%08X rec[0..7]=[%s]",
        vsync, reg(r, "a0"), reg(r, "a1"), rec, reg(r, "ra"), hexdump16(rec, 8)))
end)

bp.arm(STAGER_FN, "Exec", 4, "stager_install", function()
    if stager_hits >= 400 then return end
    stager_hits = stager_hits + 1
    local r = PCSX.getRegisters()
    log(string.format("[v%d] stager FUN_800252EC id=%d ra=0x%08X",
        vsync, reg(r, "a0"), reg(r, "ra")))
end)

-- ---------------- vsync listener: state load + VRAM poll ----------------
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
    if not loaded then return end
    local vram = get_vram()
    if vram ~= nil then
        local cur = band271(vram)
        if band0 == nil then
            band0 = cur
            local hex = {}
            for i = 1, #cur, 2 do
                hex[#hex+1] = string.format("%02x%02x", cur:byte(i+1) or 0, cur:byte(i) or 0)
            end
            log(string.format("[v%d] band(271,851..859) initial: %s", vsync, table.concat(hex, " ")))
        elseif not flip_logged and cur ~= band0 then
            flip_logged = true
            local hex = {}
            for i = 1, #cur, 2 do
                hex[#hex+1] = string.format("%02x%02x", cur:byte(i+1) or 0, cur:byte(i) or 0)
            end
            log(string.format("[v%d] band(271,851..859) FLIPPED: %s", vsync, table.concat(hex, " ")))
        end
    end
end)

-- ---------------- s2 -> s3 phase machine (from autorun_s3_capture.lua) ----------------
local pulse_btns, pulse_until = {}, 0
local function pulse(btns, frame, dur)
    for _, b in ipairs(btns) do pad.force(pad.BTN[b]) end
    pulse_btns = btns; pulse_until = frame + (dur or 3)
end

local phase = "SETTLE"
local frame = 0
local cooldown = 0
local freeroam_since = nil

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if pulse_until > 0 and frame >= pulse_until then
        for _, b in ipairs(pulse_btns) do pad.release(pad.BTN[b]) end
        pulse_until = 0; cooldown = frame + 10
    end
    if frame >= MAX_FRAMES then
        log(string.format("MAX_FRAMES reached (phase=%s flip=%s)", phase, tostring(flip_logged)))
        if LOG then LOG:close() end; PCSX.quit(0)
    end

    local cur = ru16(CURSOR)
    local eng = engaged()
    if (frame % 240) == 0 then
        log(string.format("[f%d/v%d] phase=%s cursor=%s eng=%s scene=%q installs=%d",
            frame, vsync, phase, cur and string.format("0x%04X", cur) or "nil",
            tostring(eng), read_scene(), install_hits))
    end

    if pulse_until > 0 or frame < cooldown then return end

    if phase == "SETTLE" then
        if frame >= 760 and eng and cur ~= nil then phase = "SELECT_END"; log("phase -> SELECT_END") end
        return
    end
    if phase == "SELECT_END" then
        if cur == nil then return end
        if cur >= 116 and cur <= 118 then
            pulse({"CROSS"}, frame, 3); phase = "CONFIRM"; log("selected End (CROSS) -> CONFIRM")
        else
            local row, col = math.floor(cur/17), cur%17
            if row < 6 then pulse({"DOWN"}, frame, 3)
            elseif col < 14 then pulse({"RIGHT"}, frame, 3)
            else pulse({"LEFT"}, frame, 3) end
        end
        return
    end
    if phase == "CONFIRM" then
        if eng == false then
            phase = "FREEROAM"; log(string.format("[f%d] engaged cleared -> FREEROAM", frame)); return
        end
        mem.write_u8(0x8007B458, 0)
        pulse({"CROSS"}, frame, 3)
        return
    end
    if phase == "FREEROAM" then
        if freeroam_since == nil then freeroam_since = frame end
        -- run a settle tail so post-name-entry events (and the flip) land
        if frame - freeroam_since >= 300 then
            log(string.format("[f%d] done (flip=%s installs=%d)", frame, tostring(flip_logged), install_hits))
            if LOG then LOG:close() end; PCSX.quit(0)
        end
        return
    end
end)

log("s2s3 scroll-installer probe armed")
