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
    let mut instant = Instant::now();
    instant.shift_back(age);
    Ok(instant)
}

pub trait ShiftBack {
    fn shift_back(&mut self, by: Duration);
}

impl ShiftBack for Instant {
    fn shift_back(&mut self, by: Duration) {
        *self = self.checked_sub(by).unwrap_or_else(Instant::now);
    }
}

pub mod optional_duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(value: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        value.map(|d| d.as_secs_f64()).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        Option::<f64>::deserialize(d)?
            .map(Duration::try_from_secs_f64)
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}
