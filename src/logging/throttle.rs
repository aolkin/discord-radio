use std::time::{Duration, Instant};

pub struct LogThrottle {
    interval: Duration,
    last: Option<Instant>,
    suppressed: u64,
}

impl LogThrottle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
            suppressed: 0,
        }
    }

    pub fn allow(&mut self) -> Option<u64> {
        let now = Instant::now();
        match self.last {
            Some(last) if now.duration_since(last) < self.interval => {
                self.suppressed += 1;
                None
            }
            _ => {
                self.last = Some(now);
                Some(std::mem::take(&mut self.suppressed))
            }
        }
    }
}
