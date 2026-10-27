use field_trait::field_trait;
use rand::RngExt;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;

/// A weight an item contributes to weighted random selection.
#[field_trait]
pub trait Weighted {
    fn weight(&self) -> u32;
    /// Ignores the weight, so editing only an item's weight keeps its place
    /// in a selector's history.
    fn identity(&self) -> u64;
}

#[field_trait]
pub trait WeightedPool<T: Weighted> {
    fn entries(&self) -> &Arc<[T]>;
    fn history_penalty_multiplier(&self) -> Option<f32>;
}

/// Hashes `value` with FNV-1a.
pub fn identity_of(value: impl Hash) -> u64 {
    let mut hasher = Fnv1a(0xcbf2_9ce4_8422_2325);
    value.hash(&mut hasher);
    hasher.finish()
}

struct Fnv1a(u64);

impl Hasher for Fnv1a {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Picks an index at random in proportion to `weights`, or `None` when they
/// sum to zero.
fn choose_index(weights: &[f32]) -> Option<usize> {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return None;
    }

    let roll = rand::rng().random::<f32>() * total;
    let mut cumulative = 0.0;
    let idx = weights
        .iter()
        .position(|weight| {
            cumulative += weight;
            roll < cumulative
        })
        .unwrap_or(weights.len() - 1);
    Some(idx)
}

/// A weighted random selector with history-based penalty to avoid repetition
pub struct WeightedSelector<T, S = Arc<[T]>> {
    items: S,
    identities: Box<[u64]>,
    recent_history: VecDeque<u64>,
    history_size: usize,
    penalty_multiplier: f32,
    item: PhantomData<fn() -> T>,
}

impl<T: Weighted, P: WeightedPool<T>> From<&P> for WeightedSelector<T> {
    fn from(pool: &P) -> Self {
        Self::new(pool.entries().clone(), pool.history_penalty_multiplier())
    }
}

impl<T: Weighted> WeightedSelector<T> {
    pub fn set_pool<P: WeightedPool<T>>(&mut self, pool: &P) {
        self.set_entries(pool.entries().clone(), pool.history_penalty_multiplier());
    }
}

impl<T: Weighted, S: AsRef<[T]>> WeightedSelector<T, S> {
    /// Remembers the last half of the item count in picks, at least 2.
    /// The penalty multiplier defaults to 1.
    pub fn new(items: S, history_penalty_multiplier: Option<f32>) -> Self {
        let mut selector = Self {
            items,
            identities: Box::default(),
            recent_history: VecDeque::new(),
            history_size: 0,
            penalty_multiplier: 1.0,
            item: PhantomData,
        };
        selector.apply_entries(history_penalty_multiplier);
        selector
    }

    /// Keeps the recent history, trimmed to the new item count.
    pub fn set_entries(&mut self, items: S, history_penalty_multiplier: Option<f32>) {
        self.items = items;
        self.apply_entries(history_penalty_multiplier);
    }

    pub fn map_entries(&mut self, map: impl FnOnce(S) -> S, history_penalty_multiplier: Option<f32>)
    where
        S: Default,
    {
        let items = map(std::mem::take(&mut self.items));
        self.set_entries(items, history_penalty_multiplier);
    }

    fn apply_entries(&mut self, history_penalty_multiplier: Option<f32>) {
        let items = self.items.as_ref();
        self.identities = items.iter().map(Weighted::identity).collect();
        self.history_size = (items.len() / 2).max(2);
        while self.recent_history.len() > self.history_size {
            self.recent_history.pop_front();
        }
        self.penalty_multiplier = history_penalty_multiplier.unwrap_or(1.0);
    }

    pub fn choose(&mut self) -> Option<&T> {
        let idx = self.pick_index()?;
        Some(&self.items.as_ref()[idx])
    }

    fn pick_index(&mut self) -> Option<usize> {
        let effective_weights: Vec<f32> = self
            .items
            .as_ref()
            .iter()
            .zip(&self.identities)
            .map(|(item, &identity)| item.weight() as f32 * self.penalty(identity))
            .collect();

        let idx = choose_index(&effective_weights)?;
        if self.recent_history.len() >= self.history_size {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(self.identities[idx]);
        Some(idx)
    }

    fn penalty(&self, identity: u64) -> f32 {
        match self
            .recent_history
            .iter()
            .rposition(|&recent| recent == identity)
        {
            Some(history_idx) => {
                let recency_factor = 1.0 - (history_idx as f32 / self.recent_history.len() as f32);
                self.penalty_multiplier * recency_factor + (1.0 - self.penalty_multiplier)
            }
            None => 1.0,
        }
    }
}

impl<T: Weighted, S: AsRef<[T]> + AsMut<[T]>> WeightedSelector<T, S> {
    pub fn choose_mut(&mut self) -> Option<&mut T> {
        let idx = self.pick_index()?;
        Some(&mut self.items.as_mut()[idx])
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.as_mut().iter_mut()
    }
}
