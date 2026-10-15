use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::{Instant, SystemTime};

pub fn ser_instant<S>(instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let elapsed = instant.elapsed();
    let time = SystemTime::now()
        .checked_sub(elapsed)
        .unwrap_or_else(SystemTime::now);
    time.serialize(serializer)
}

pub fn deser_instant<'de, D>(deserializer: D) -> Result<Instant, D::Error>
where
    D: Deserializer<'de>,
{
    let time = SystemTime::deserialize(deserializer)?;
    // A backwards clock jump (future `time`) or a state persisted longer ago than the machine's
    // uptime (elapsed exceeds the monotonic clock) would otherwise fail the whole deserialize;
    // clamp both to "just started" instead.
    let elapsed = time.elapsed().unwrap_or_default();
    Ok(Instant::now()
        .checked_sub(elapsed)
        .unwrap_or_else(Instant::now))
}
