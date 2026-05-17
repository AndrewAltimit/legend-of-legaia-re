-- probe/pad.lua  -- PSX pad-button override helpers.
--
-- PCSX-Redux exposes setOverride / clearOverride on each pad object to
-- force a button held / released from script context. Useful for
-- driving title-menu / dialog-advance flows headlessly while a probe
-- captures hits.
--
-- All entry points pcall their underlying call because the pad slot
-- may not be populated in headless boots; a missing-pad exception
-- shouldn't crash the probe.
--
-- Usage:
--   local pad = require("probe.pad")
--   pad.force(pad.BTN.START)
--   ...
--   pad.release(pad.BTN.START)

local M = {}

-- PSX pad bit indices (D-pad + buttons in the 16-bit status word).
M.BTN = {
    SELECT = 0,  L3 = 1,  R3 = 2,  START = 3,
    UP     = 4,  RIGHT = 5,  DOWN = 6,  LEFT = 7,
    L2     = 8,  R2 = 9,  L1 = 10, R1 = 11,
    TRIANGLE = 12, CIRCLE = 13, CROSS = 14, SQUARE = 15,
}

function M.force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end

function M.release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

return M
