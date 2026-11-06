use crate::audio::dj::segments::Completed;
use crate::audio::dj::serde::{ShiftBack, deser_instant, ser_instant};
use crate::audio::dj::weighted_choice::{Weighted, WeightedPool, WeightedSelector};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self, Debug, Formatter};
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
            started_at.shift_back(by);
        }
    }
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound(serialize = "U: Serialize", deserialize = "U: Deserialize<'de>"))]
pub struct TieredResolver<E: RotationEntry, U = <E as RotationEntry>::Value> {
    pub rotation: Rotation<E>,
    pub override_value: Option<U>,
}

impl<E: RotationEntry, U: Clone + From<E::Value>> TieredResolver<E, U> {
    pub fn new(rotation: Rotation<E>) -> Self {
        Self {
            rotation,
            override_value: None,
        }
    }

    pub fn resolve(&self, segment: Option<E::Value>) -> Resolved<U> {
        if let Some(value) = &self.override_value {
            return Resolved {
                value: value.clone(),
                tier: Tier::Override,
            };
        }
        if let Some(value) = segment {
            return Resolved {
                value: value.into(),
                tier: Tier::Segment,
            };
        }
        let (value, tier) = match self.rotation.background() {
            Background::Manual(value) => (value, Tier::Manual),
            Background::Rotating { value, .. } => (value, Tier::Rotating),
        };
        Resolved {
            value: value.clone().into(),
            tier,
        }
    }
}

pub trait RotationEntry: Weighted + Sized + 'static {
    type Value: Clone + Debug + Serialize + DeserializeOwned;

    fn fallback() -> &'static Self;

    fn pick(&self) -> (Self::Value, Duration);
}

pub struct Rotation<E: RotationEntry> {
    background: Background<E::Value>,
    selector: WeightedSelector<E>,
}

impl<E: RotationEntry> Rotation<E> {
    pub fn new(initial: E::Value) -> Self {
        Self::from(Background::due_now(initial))
    }

    pub fn background(&self) -> &Background<E::Value> {
        &self.background
    }

    pub fn completed(&self) -> Completed {
        self.background.completed()
    }

    pub fn set_manual(&mut self, value: Option<E::Value>) {
        self.background.set_manual(value);
    }

    pub fn shift_back(&mut self, by: Duration) {
        self.background.shift_back(by);
    }

    pub fn set_pool<P: WeightedPool<E>>(&mut self, pool: &P) {
        self.selector.set_pool(pool);
    }

    pub fn step(&mut self) {
        let (value, duration) = self.selector.choose().unwrap_or(E::fallback()).pick();
        self.background = Background::Rotating {
            value,
            started_at: Instant::now(),
            duration,
        };
    }
}

impl<E: RotationEntry> From<Background<E::Value>> for Rotation<E> {
    fn from(background: Background<E::Value>) -> Self {
        Self {
            background,
            selector: WeightedSelector::new(Arc::default(), None),
        }
    }
}

impl<E: RotationEntry> Debug for Rotation<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.background.fmt(f)
    }
}

impl<E: RotationEntry> Serialize for Rotation<E> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.background.serialize(serializer)
    }
}

impl<'de, E: RotationEntry> Deserialize<'de> for Rotation<E> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Background::deserialize(deserializer).map(Self::from)
    }
}
