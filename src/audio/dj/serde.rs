use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::{Duration, Instant};

/// Writes the time since `instant` as a `Duration`; `deser_instant` rebuilds
/// the instant by subtracting that age from now.
pub fn ser_instant<S>(instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    instant.elapsed().serialize(serializer)
}

pub fn deser_instant<'de, D>(deserializer: D) -> Result<Instant, D::Error>
where
    D: Deserializer<'de>,
{
    let age = Duration::deserialize(deserializer)?;
    Ok(Instant::now().checked_sub(age).unwrap_or_else(Instant::now))
}
