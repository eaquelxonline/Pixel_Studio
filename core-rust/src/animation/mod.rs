//! Frames, per-frame timing, animation tags and playback order.
//!
//! There is no fixed frame count: frames live in a growable list limited only by memory.

pub const DEFAULT_FRAME_MS: u32 = 100;
pub const MIN_FRAME_MS: u32 = 1;
pub const MAX_FRAME_MS: u32 = 60_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Stable identifier; survives reordering.
    pub id: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
    PingPong,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Direction> {
        match s.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "forward" | "loop" => Some(Direction::Forward),
            "reverse" | "backward" => Some(Direction::Reverse),
            "pingpong" => Some(Direction::PingPong),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Forward => "forward",
            Direction::Reverse => "reverse",
            Direction::PingPong => "pingpong",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Direction::Forward => 0,
            Direction::Reverse => 1,
            Direction::PingPong => 2,
        }
    }

    pub fn from_code(c: u8) -> Option<Direction> {
        match c {
            0 => Some(Direction::Forward),
            1 => Some(Direction::Reverse),
            2 => Some(Direction::PingPong),
            _ => None,
        }
    }
}

/// A named frame range such as "Idle", "Walk" or "Attack".
/// `from`/`to` are frame ids, so tags follow frames when they are reordered.
#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub name: String,
    pub from_id: u32,
    pub to_id: u32,
    pub direction: Direction,
    /// 0 = loop forever.
    pub repeat: u32,
}

/// Frame order of one playback cycle for a frame index range.
pub fn playback_order(from: usize, to: usize, direction: Direction) -> Vec<usize> {
    let (lo, hi) = (from.min(to), from.max(to));
    let forward: Vec<usize> = (lo..=hi).collect();
    match direction {
        Direction::Forward => forward,
        Direction::Reverse => forward.into_iter().rev().collect(),
        Direction::PingPong => {
            let mut seq = forward.clone();
            if hi > lo + 1 {
                seq.extend((lo + 1..hi).rev());
            } else if hi == lo + 1 {
                // two frames: a b (no duplicated end points)
            }
            seq
        }
    }
}

/// Timing summary for a playback sequence.
pub struct Timing {
    pub total_ms: u64,
    pub average_fps: f64,
}

pub fn timing(durations: &[u32]) -> Timing {
    let total: u64 = durations.iter().map(|&d| d as u64).sum();
    let average_fps = if total == 0 { 0.0 } else { durations.len() as f64 * 1000.0 / total as f64 };
    Timing { total_ms: total, average_fps }
}

pub fn validate_duration(ms: i64) -> Result<u32, String> {
    if ms < MIN_FRAME_MS as i64 || ms > MAX_FRAME_MS as i64 {
        return Err(format!("frame duration must be between {MIN_FRAME_MS} and {MAX_FRAME_MS} ms"));
    }
    Ok(ms as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders() {
        assert_eq!(playback_order(0, 3, Direction::Forward), vec![0, 1, 2, 3]);
        assert_eq!(playback_order(0, 3, Direction::Reverse), vec![3, 2, 1, 0]);
        assert_eq!(playback_order(0, 3, Direction::PingPong), vec![0, 1, 2, 3, 2, 1]);
        assert_eq!(playback_order(2, 3, Direction::PingPong), vec![2, 3]);
        let t = timing(&[120, 80, 80, 150, 100]);
        assert_eq!(t.total_ms, 530);
        assert!((t.average_fps - 9.4339).abs() < 0.001);
    }
}
