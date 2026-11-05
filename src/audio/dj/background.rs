use crate::audio::dj::segments::Completed;
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::dj::weighted_choice::{Weighted, WeightedPool, WeightedSelector};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
pub enum Background<T> {
    Manual(T),
    Rotating {
        value: T,
        #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
        started_at: Instant,
        duration: Duration,
    },
}

impl<T: Clone> Background<T> {
    pub fn due_now(value: T) -> Self {
        Self::Rotating {
            value,
            started_at: Instant::now(),
            duration: Duration::ZERO,
        }
    }

    pub fn value(&self) -> &T {
        match self {
            Self::Manual(value) | Self::Rotating { value, .. } => value,
        }
    }

    pub fn tier(&self) -> Tier {
        match self {
            Self::Manual(_) => Tier::Manual,
            Self::Rotating { .. } => Tier::Rotating,
        }
    }

    pub fn completed(&self) -> Completed {
        match self {
            Self::Manual(_) => Completed::never(),
            Self::Rotating {
                started_at,
                duration,
                ..
            } => Completed::at(*started_at + *duration),
        }
    }

    pub fn set_manual(&mut self, value: Option<T>) {
        match value {
            Some(value) => *self = Self::Manual(value),
            None => {
                if let Self::Manual(value) = self {
                    *self = Self::due_now(value.clone());
                }
            }
        }
    }

    pub fn shift_back(&mut self, by: Duration) {
        if let Self::Rotating { started_at, .. } = self {
            shift_instant_back(started_at, by);
        }
    }
}

pub fn shift_instant_back(instant: &mut Instant, by: Duration) {
    *instant = instant.checked_sub(by).unwrap_or_else(Instant::now);
}

/// Ordered from lowest to highest priority.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum Tier {
    Rotating,
    Manual,
    Segment,
    Override,
}

pub struct Resolved<T> {
    pub value: T,
    pub tier: Tier,
}

pub fn resolve<T: Clone, U: From<T>>(
    override_value: Option<U>,
    segment: Option<T>,
    background: &Background<T>,
) -> Resolved<U> {
    if let Some(value) = override_value {
        return Resolved {
            value,
            tier: Tier::Override,
        };
    }
    if let Some(value) = segment {
        return Resolved {
            value: value.into(),
            tier: Tier::Segment,
        };
    }
    Resolved {
        value: background.value().clone().into(),
        tier: background.tier(),
    }
}

pub trait RotationEntry: Weighted + 'static {
    type Value: Clone;

    fn pick(&self) -> (Self::Value, Duration);
}

pub struct Rotation<E: 'static> {
    selector: WeightedSelector<E>,
    fallback: &'static E,
}

impl<E: RotationEntry> Rotation<E> {
    pub fn new(fallback: &'static E) -> Self {
        Self {
            selector: WeightedSelector::new(Arc::default(), None),
            fallback,
        }
    }

    pub fn set_pool<P: WeightedPool<E>>(&mut self, pool: &P) {
        self.selector.set_pool(pool);
    }

    pub fn step(&mut self, background: &mut Background<E::Value>) {
        let (value, duration) = self.selector.choose().unwrap_or(self.fallback).pick();
        *background = Background::Rotating {
            value,
            started_at: Instant::now(),
            duration,
        };
    }
}
