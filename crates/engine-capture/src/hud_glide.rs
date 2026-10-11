//! [`HudGlideSeat`]: one battle HUD widget glide at the capture instant,
//! with its environment hand-off.

/// One HUD widget glide in flight, as a capture's **displayed** frame shows
/// it: the record's target seat (`+0x04` / `+0x06`, which names the widget -
/// the actor plaque lands on `(16, 12)`, the readout bar on `(16, 192)`, the
/// combo cluster's anchor on `(168, 168)`) and its `elapsed` byte less the
/// display lag (`display_lag_vsyncs`). `FUN_801D9BBC` adds the frame step
/// to `elapsed` a battle pass, so the byte counts vsyncs, the unit the lag is
/// in; a glide younger than the lag had not left its start on the displayed
/// frame (`0`).
///
/// The image child seats the engine's glides on these
/// (`LEGAIA_SEAT_HUD_GLIDES`): the replay reaches an action's phase on its
/// own clock, which the plates' sixteen-vsync raise does not share
/// (`nivora_duel_mid_blazing_slash` holds plaque and bar ten vsyncs into the
/// raise, six on screen, where the engine's had landed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudGlideSeat {
    pub target: [i16; 2],
    pub elapsed: u8,
    pub total: u8,
}

impl HudGlideSeat {
    /// `x:y:elapsed:total`, comma-separated - the form
    /// [`Self::list_from_env`] reads back.
    pub fn to_env(seats: &[Self]) -> String {
        seats
            .iter()
            .map(|g| format!("{}:{}:{}:{}", g.target[0], g.target[1], g.elapsed, g.total))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The inverse of [`Self::to_env`]; malformed entries are dropped.
    pub fn list_from_env(v: &str) -> Vec<Self> {
        v.split(',')
            .filter_map(|e| {
                let mut it = e.trim().split(':').map(str::parse::<i32>);
                let (Some(Ok(x)), Some(Ok(y)), Some(Ok(elapsed)), Some(Ok(total))) =
                    (it.next(), it.next(), it.next(), it.next())
                else {
                    return None;
                };
                Some(Self {
                    target: [x as i16, y as i16],
                    elapsed: elapsed.clamp(0, 255) as u8,
                    total: total.clamp(0, 255) as u8,
                })
            })
            .collect()
    }
}
