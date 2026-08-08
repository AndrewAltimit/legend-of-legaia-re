-- autorun_magic_menu_freeze.lua
--
-- Repro probe for "shiny-seru-patched ROM freezes on the pause-menu Magic
-- list". Loads a FIELD save state (the menu overlay is NOT resident in field
-- mode, so opening the pause menu re-reads PROT 0899 from the mounted disc -
-- which is how a patched disc's overlay edits reach a vanilla state), then
-- drives the pad: Triangle (open menu) -> Down x3 (Character/Equip/Items/
-- Magic) -> Cross (enter Magic) -> Cross (pick first character) -> browse.
--
-- Freeze detection is a per-frame heartbeat: an exec breakpoint on the pad
-- builder FUN_8001822C, which the retail main loop calls once per frame in
-- every mode. A software hang (tight loop / exception spin) stops calling it,
-- so "heartbeat delta over the last ~2s == 0" is the freeze verdict. The CSV
-- samples pc + game mode + heartbeat + the menu state cells every few frames
-- so a patched-vs-vanilla run pair diffs to the exact step they diverge.
--
-- Run (vanilla):
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_magic_menu_freeze.lua \
--     --scenario chapter2_garmel_pre_zeto --frames 900 \
--     --out /tmp/magic_freeze_vanilla.csv
-- Run (patched): add --iso <patched.bin>
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2000)

local GMODE = 0x8007B83C
local PAD_BUILD_FN = 0x8001822C -- per-frame pad builder (heartbeat)
local MENU_STATE_LO = 0x801E4690 -- menu overlay state cells (picker substate at +0x1C)
local PAD_CUR = 0x8007B850 -- current pad word (built by FUN_8001822C)

local heartbeat = 0
local f_hits = 0
local hook_hits = 0
local hb_ring = {}
local csvf = nil
local pressed = nil
local menu_t0 = nil -- elapsed vsync when gmode first read 0x17
local B = probe.BTN
local PRESS_HOLD = 12

-- Sweep: attempt k = press Down k times, Cross (enter), Cross again (char
-- pick, if any), then Circle x2 to back out. The 0899 hook-site BP fires iff
-- an attempt reached the Magic spell list. ~220 frames per attempt.
local ATTEMPT_LEN = 220
local function nav_events_for(k)
    local ev = {}
    for i = 0, k - 1 do ev[#ev + 1] = { 15 * i + 10, B.DOWN } end
    local t = 15 * k + 30
    ev[#ev + 1] = { t, B.CROSS }
    ev[#ev + 1] = { t + 60, B.CROSS }
    ev[#ev + 1] = { t + 130, B.CIRCLE }
    ev[#ev + 1] = { t + 155, B.CIRCLE }
    return ev
end

local function u8(a) return probe.read_u8(a) or 0xFF end

local function pc_now()
    local ok, r = pcall(PCSX.getRegisters)
    if not ok or not r then return 0 end
    local v = tonumber(r.pc) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

local function press(btn, elapsed, why)
    probe.pad_force(btn)
    pressed = { btn = btn, at = elapsed }
    PCSX.log(string.format("[pad] frame %d press %s (gmode=0x%02X)", elapsed, why, u8(GMODE)))
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== magic-menu freeze probe == gmode=0x%02X", u8(GMODE)))
        csvf = probe.csv_open(probe.out_path("magic_menu_freeze.csv"),
            "frame,pc,gmode,pad,heartbeat,menu_state_hex")
        probe.arm_breakpoint(PAD_BUILD_FN, "Exec", 4, "heartbeat", function()
            heartbeat = heartbeat + 1
        end)
        -- Oracle BPs: the shiny F routine (SCUS arena 1) + its 0899 hook site.
        -- On a shiny-patched disc these fire iff the Magic spell list draws.
        probe.arm_breakpoint(0x8007AED0, "Exec", 4, "F-routine", function()
            f_hits = f_hits + 1
            if f_hits <= 3 then
                PCSX.log(string.format("[F] hit %d", f_hits))
            end
        end)
        probe.arm_breakpoint(0x801D2FA0, "Exec", 4, "menu-hook-site", function()
            hook_hits = hook_hits + 1
            if hook_hits <= 3 then
                PCSX.log(string.format("[hook] hit %d", hook_hits))
            end
        end)
        return {}
    end,
    on_capture = function(_, elapsed)
        -- Give the lead a spell list so the Magic screen has rows to draw
        -- (the F hook under test runs once per drawn spell row). Vahn's live
        -- record is 0x80084708: count +0x13C, ids +0x13D.., levels +0x161..
        if elapsed == 1 then
            local R = 0x80084708
            probe.write_u8(R + 0x13C, 3)
            probe.write_u8(R + 0x13D, 0x81) -- Meta line
            probe.write_u8(R + 0x13E, 0x84)
            probe.write_u8(R + 0x13F, 0x87)
            probe.write_u8(R + 0x161, 1)
            probe.write_u8(R + 0x162, 2)
            probe.write_u8(R + 0x163, 1)
            PCSX.log("[poke] gave Vahn 3 seru spells")
        end
        -- release any held button
        if pressed and elapsed >= pressed.at + PRESS_HOLD then
            probe.pad_release(pressed.btn)
            pressed = nil
        end
        local gm = u8(GMODE)
        -- open the menu: Triangle at 40, START fallback at 130
        if menu_t0 == nil then
            if gm == 0x17 then
                menu_t0 = elapsed
                PCSX.log(string.format(
                    "[nav] menu open at frame %d; RAM[0x801D2FA0]=0x%08X (patched=0x0801EBB4 vanilla=0x8C6346B0)",
                    elapsed, probe.read_u32(0x801D2FA0) or 0))
            elseif elapsed == 40 then
                press(B.TRIANGLE, elapsed, "TRIANGLE (open menu)")
            elseif elapsed == 130 then
                press(B.START, elapsed, "START (fallback open)")
            end
        else
            local rel = elapsed - menu_t0 - 30
            if rel >= 0 then
                local k = math.floor(rel / ATTEMPT_LEN)
                local sub = rel % ATTEMPT_LEN
                if k <= 7 then
                    if sub == 0 then
                        PCSX.log(string.format(
                            "[sweep] attempt k=%d starting (gmode=0x%02X hook_hits=%d ram_hook=0x%08X)",
                            k, gm, hook_hits, probe.read_u32(0x801D2FA0) or 0))
                    end
                    -- menu fell closed (Circle at top exits) -> reopen first
                    if sub == 2 and gm ~= 0x17 then
                        press(B.TRIANGLE, elapsed, string.format("k=%d reopen menu", k))
                    end
                    for _, ev in ipairs(nav_events_for(k)) do
                        if sub == ev[1] then
                            press(ev[2], elapsed, string.format("k=%d btn %d", k, ev[2]))
                        end
                    end
                end
            end
        end
        -- heartbeat ring (window of 120 vsyncs)
        hb_ring[(elapsed % 120) + 1] = heartbeat
        if elapsed % 4 == 0 then
            local bytes = probe.read_bytes(MENU_STATE_LO, 32)
            csvf:row("%d,0x%08X,0x%02X,0x%04X,%d,%s",
                elapsed, pc_now(), gm, probe.read_u16(PAD_CUR) or 0, heartbeat,
                bytes and probe.bytes_to_hex(bytes) or "??")
        end
        if elapsed == FRAMES - 1 then
            local oldest = hb_ring[((elapsed + 1) % 120) + 1] or 0
            local delta = heartbeat - oldest
            PCSX.log(string.format(
                "[verdict] heartbeat delta over last ~119 vsyncs = %d (%s), final pc=0x%08X gmode=0x%02X menu_t0=%s f_hits=%d hook_hits=%d",
                delta, (delta == 0) and "FROZEN" or "alive", pc_now(), u8(GMODE),
                tostring(menu_t0), f_hits, hook_hits))
        end
    end,
    on_done = function()
        if csvf then csvf:close() end
    end,
})
