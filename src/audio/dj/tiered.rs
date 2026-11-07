use crate::audio::dj::segments::Completed;
use crate::audio::dj::serde::{ShiftBack, deser_instant, ser_instant};
use crate::audio::dj::weighted_choice::{Weighted, WeightedPool, WeightedSelector};
use serde::{Deserialize, Serialize, Serializer};
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

pub struct TieredStateMachine<E: 'static, V> {
    state: TieredValueState<V>,
    background_rotation: WeightedSelector<E>,
    fallback: &'static E,
    last_applied: Option<TieredValue<V>>,
}

impl<E: Weighted, V: Clone + PartialEq> TieredStateMachine<E, V>
where
    for<'a> &'a E: Into<TimedValue<V>>,
{
    pub fn new(initial: V, fallback: &'static E) -> Self {
        Self::restore(
            TieredValueState {
                tiered_values: [None, None, None],
                background_value: TimedValue::temporary(initial, Duration::ZERO),
            },
            fallback,
        )
    }

    pub fn restore(state: TieredValueState<V>, fallback: &'static E) -> Self {
        Self {
            state,
            background_rotation: WeightedSelector::new(Arc::default(), None),
            fallback,
            last_applied: None,
        }
    }

    pub fn value(&self) -> TieredValue<V> {
        let (tier, value) = self.state.controlling();
        TieredValue {
            value: value.value.clone(),
            tier,
        }
    }

    pub fn completed(&self) -> Completed {
        self.state.completed()
    }

    pub fn update_override(&mut self, value: Option<TimedValue<V>>) {
        self.state.update(Tier::Override, value);
    }

    pub fn update_forced(&mut self, value: Option<TimedValue<V>>) {
        self.state.update(Tier::Forced, value);
    }

    pub fn update_manual(&mut self, value: Option<TimedValue<V>>) {
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
                let entry = self.background_rotation.choose().unwrap_or(self.fallback);
                self.state.background_value = entry.into();
            }
            tier => self.state.update(tier, None),
        }
    }

    pub fn changed(&mut self) -> Option<TieredChange<V>> {
        let current = self.value();
        let previous = self.last_applied.replace(current.clone());
        match &previous {
            Some(previous) if previous.value == current.value => None,
            _ => Some(TieredChange { previous, current }),
        }
    }
}

impl<E, V: Debug> Debug for TieredStateMachine<E, V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.state.fmt(f)
    }
}

impl<E, V: Serialize> Serialize for TieredStateMachine<E, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.state.serialize(serializer)
    }
}
