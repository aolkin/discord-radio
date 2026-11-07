use crate::audio::dj::segments::Completed;
use crate::audio::dj::serde::{ShiftBack, deser_instant, ser_instant};
use crate::audio::dj::weighted_choice::{Weighted, WeightedPool, WeightedSelector};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
pub struct TimedValue<T> {
    value: T,
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    started_at: Instant,
    duration: Option<Duration>,
}

impl<T> TimedValue<T> {
    fn new(value: T, duration: Option<Duration>) -> Self {
        Self {
            value,
            started_at: Instant::now(),
            duration,
        }
    }

    pub fn permanent(value: T) -> Self {
        Self::new(value, None)
    }

    pub fn temporary(value: T, duration: Duration) -> Self {
        Self::new(value, Some(duration))
    }

    pub fn completed(&self) -> Completed {
        if let Some(duration) = self.duration {
            Completed::at(self.started_at + duration)
        } else {
            Completed::never()
        }
    }

    pub fn shift_back(&mut self, by: Duration) {
        self.started_at.shift_back(by);
    }
}

/// Ordered from highest to lowest priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum Tier {
    Override,
    Forced,
    Manual,
    Background,
}

const TIERS: [Tier; 3] = [Tier::Override, Tier::Forced, Tier::Manual];

#[derive(Clone, Debug, PartialEq)]
pub struct TieredValue<T> {
    pub value: T,
    pub tier: Tier,
}

pub struct TieredChange<T> {
    pub previous: Option<TieredValue<T>>,
    pub current: TieredValue<T>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TieredValueState<T> {
    tiered_values: [Option<TimedValue<T>>; 3],
    background_value: TimedValue<T>,
}

impl<T> TieredValueState<T> {
    pub fn controlling(&self) -> (Tier, &TimedValue<T>) {
        TIERS
            .iter()
            .zip(&self.tiered_values)
            .find_map(|(tier, value)| value.as_ref().map(|value| (*tier, value)))
            .unwrap_or((Tier::Background, &self.background_value))
    }

    pub fn completed(&self) -> Completed {
        let (_, value) = self.controlling();
        value.completed()
    }

    fn update(&mut self, tier: Tier, value: Option<TimedValue<T>>) {
        if let Some(index) = TIERS.iter().position(|candidate| *candidate == tier) {
            self.tiered_values[index] = value;
        }
    }

    fn shift_back(&mut self, by: Duration) {
        for value in self.tiered_values.iter_mut().flatten() {
            value.shift_back(by);
        }
        self.background_value.shift_back(by);
    }
}

pub trait RotationEntry: Weighted + Sized + 'static {
    type Value: Clone + PartialEq + Debug + Serialize + DeserializeOwned;

    fn fallback() -> &'static Self;

    fn pick(&self) -> (Self::Value, Duration);
}

pub struct TieredStateMachine<E: RotationEntry> {
    state: TieredValueState<E::Value>,
    background_rotation: WeightedSelector<E>,
    last_applied: Option<TieredValue<E::Value>>,
}

impl<E: RotationEntry> TieredStateMachine<E> {
    pub fn new(initial: E::Value) -> Self {
        Self::from(TieredValueState {
            tiered_values: [None, None, None],
            background_value: TimedValue::temporary(initial, Duration::ZERO),
        })
    }

    pub fn value(&self) -> TieredValue<E::Value> {
        let (tier, value) = self.state.controlling();
        TieredValue {
            value: value.value.clone(),
            tier,
        }
    }

    pub fn completed(&self) -> Completed {
        self.state.completed()
    }

    pub fn update_override(&mut self, value: Option<TimedValue<E::Value>>) {
        self.state.update(Tier::Override, value);
    }

    pub fn update_forced(&mut self, value: Option<TimedValue<E::Value>>) {
        self.state.update(Tier::Forced, value);
    }

    pub fn update_manual(&mut self, value: Option<TimedValue<E::Value>>) {
        self.state.update(Tier::Manual, value);
    }

    pub fn set_pool<P: WeightedPool<E>>(&mut self, pool: &P) {
        self.background_rotation.set_pool(pool);
    }

    pub fn shift_back(&mut self, by: Duration) {
        self.state.shift_back(by);
    }

    pub fn advance(&mut self) {
        match self.state.controlling().0 {
            Tier::Background => {
                let (value, duration) = self
                    .background_rotation
                    .choose()
                    .unwrap_or(E::fallback())
                    .pick();
                self.state.background_value = TimedValue::temporary(value, duration);
            }
            tier => self.state.update(tier, None),
        }
    }

    pub fn changed(&mut self) -> Option<TieredChange<E::Value>> {
        let current = self.value();
        let previous = self.last_applied.replace(current.clone());
        match &previous {
            Some(previous) if previous.value == current.value => None,
            _ => Some(TieredChange { previous, current }),
        }
    }
}

impl<E: RotationEntry> From<TieredValueState<E::Value>> for TieredStateMachine<E> {
    fn from(state: TieredValueState<E::Value>) -> Self {
        Self {
            state,
            background_rotation: WeightedSelector::new(Arc::default(), None),
            last_applied: None,
        }
    }
}

impl<E: RotationEntry> Debug for TieredStateMachine<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.state.fmt(f)
    }
}

impl<E: RotationEntry> Serialize for TieredStateMachine<E> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.state.serialize(serializer)
    }
}

impl<'de, E: RotationEntry> Deserialize<'de> for TieredStateMachine<E> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TieredValueState::deserialize(deserializer).map(Self::from)
    }
}
