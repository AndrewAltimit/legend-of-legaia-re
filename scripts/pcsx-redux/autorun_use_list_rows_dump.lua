-- autorun_use_list_rows_dump.lua
--
-- Capture confirmation for the Use-list 0x800 dim-bit law: pad-walk from a
-- field state into the pause Items screen (SELECT -> CROSS on Items ->
-- CROSS on Use), and dump the kind-4 list rows of the live list window
-- (content id +0x1C == 3) at three moments: right after the Items screen
-- opens (command focus / kernel parked), right after the hand enters the
-- Use list, and ~60 vsyncs later. If the row entries are identical across
-- the dumps, the white->grey flip is purely the kernel's park override and
-- no state-dependent 0x800 write exists.
--
-- Live-window list head: gp[+0x148] = 0x8007B460 (FUN_800326AC allocates
-- the anchor there; nodes are 0x34-byte, +0x0 next, +0x8 window id u16,
-- +0x18 list node, +0x1C content id byte, +0x1D mode byte).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 460)

local LIST_HEAD_PTR = 0x8007B460
local GAME_MODE_VA = 0x8007b83c
local SUBMENU_VA = 0x801E46A4
local BAG_VA = 0x80085958

local function dump_windows(tag)
    local anchor = probe.read_u32(LIST_HEAD_PTR)
    if anchor == nil
        or bit.tobit(bit.band(anchor, 0xFF000000)) ~= bit.tobit(0x80000000) then
        PCSX.log(string.format("[uselist] %s: no window list (0x%08X)",
            tag, anchor or 0))
        return
    end
    local node = probe.read_u32(anchor)   -- first real node
    local steps = 0
    while node ~= nil and node ~= anchor and steps < 40 do
        local wid = probe.read_u16(node + 0x8)
        local cid = probe.read_u8(node + 0x1C)
        local mode = probe.read_u8(node + 0x1D)
        local lnode = probe.read_u32(node + 0x18)
        local desc = ""
        if lnode ~= nil
            and bit.tobit(bit.band(lnode, 0xFF000000)) == bit.tobit(0x80000000) then
            local top = probe.read_u16(lnode + 0)
            local vis = probe.read_u16(lnode + 2)
            local cnt = probe.read_u16(lnode + 4)
            local sel = probe.read_u16(lnode + 6)
            local rows = {}
            for i = 0, math.min(cnt - 1, 23) do
                local e = probe.read_u16(lnode + 0x28 + i * 2)
                local slot = bit.band(e, 0x3FF)
                local item = probe.read_u8(BAG_VA + slot * 2) or 0
                rows[#rows + 1] = string.format("%04X(i%02X)", e, item)
            end
            desc = string.format(" top=%d vis=%d cnt=%d sel=%d rows=[%s]",
                top, vis, cnt, sel, table.concat(rows, " "))
        end
        PCSX.log(string.format(
            "[uselist] %s win id=%d cid=0x%02X mode=%d%s",
            tag, wid or -1, cid or 0xFF, mode or 0xFF, desc))
        node = probe.read_u32(node)
        steps = steps + 1
    end
end

local script = {
    { at = 60, btn = pad.BTN.SELECT, name = "SELECT" },
    { at = 200, btn = pad.BTN.CROSS, name = "CROSS items" },
    { at = 320, btn = pad.BTN.CROSS, name = "CROSS use" },
}
local HOLD = 8
local dumps = { [270] = "items_open_cmd_focus", [390] = "use_list_focus",
    [450] = "use_list_focus_late" }
local last_mode = nil

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function() return {} end,

    on_capture = function(_ctx, tick)
        local mode = probe.read_u8(GAME_MODE_VA)
        local sub = probe.read_u32(SUBMENU_VA)
        local key = string.format("%02x/%08x", mode or 0xFF, sub or 0)
        if key ~= last_mode then
            PCSX.log(string.format(
                "[uselist] vsync=%d game_mode=0x%02x submenu=0x%x",
                tick, mode or 0xFF, sub or 0))
            last_mode = key
        end
        for _, s in ipairs(script) do
            if tick == s.at then
                pad.force(s.btn)
                PCSX.log(string.format("[uselist] vsync=%d press %s", tick, s.name))
            elseif tick == s.at + HOLD then
                pad.release(s.btn)
            end
        end
        local tag = dumps[tick]
        if tag ~= nil then dump_windows(tag) end
    end,

    on_done = function()
        for _, s in ipairs(script) do pad.release(s.btn) end
        PCSX.log("=== use_list_rows_dump done ===")
    end,
})
