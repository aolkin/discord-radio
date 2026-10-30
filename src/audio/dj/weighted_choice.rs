use ambassador::delegatable_trait;
use field_trait::field_trait;
use rand::RngExt;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A weight an item contributes to weighted random selection.
#[delegatable_trait]
#[field_trait(Arc)]
pub trait Weighted {
    fn weight(&self) -> u32;
    /// Ignores the weight, so editing only an item's weight keeps its place
    /// in a selector's history.
    fn identity(&self) -> Identity;
}

#[field_trait]
pub trait WeightedPool<T: Weighted> {
    fn entries(&self) -> &Arc<[T]>;
    fn history_penalty_multiplier(&self) -> Option<f32>;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Identity(u64);

impl Identity {
    pub fn of(value: impl Hash) -> Self {
        let mut hasher = Fnv1a(0xcbf2_9ce4_8422_2325);
        value.hash(&mut hasher);
        Self(hasher.finish())
    }
}

struct Fnv1a(u64);

impl Hasher for Fnv1a {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
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
pub struct WeightedSelector<T> {
    items: Arc<[T]>,
    identities: Box<[Identity]>,
    recent_history: VecDeque<Identity>,
    penalty_multiplier: f32,
}

impl<T: Weighted, P: WeightedPool<T>> From<&P> for WeightedSelector<T> {
    fn from(pool: &P) -> Self {
        Self::new(pool.entries().clone(), pool.history_penalty_multiplier())
    }
}

impl<T: Weighted> WeightedSelector<T> {
    /// Remembers the last half of the item count in picks, at least 2.
    /// The penalty multiplier defaults to 1.
    pub fn new(items: Arc<[T]>, history_penalty_multiplier: Option<f32>) -> Self {
        let mut selector = Self {
            items,
            identities: Box::default(),
            recent_history: VecDeque::new(),
            penalty_multiplier: history_penalty_multiplier.unwrap_or(1.0),
        };
        selector.update_identities();
        selector
    }

    pub fn set_entries(&mut self, items: Arc<[T]>, history_penalty_multiplier: Option<f32>) {
        self.items = items;
        self.penalty_multiplier = history_penalty_multiplier.unwrap_or(1.0);
        self.update_identities();
    }

    pub fn set_pool<P: WeightedPool<T>>(&mut self, pool: &P) {
        self.set_entries(pool.entries().clone(), pool.history_penalty_multiplier());
    }

    fn update_identities(&mut self) {
        self.identities = self.items.iter().map(Weighted::identity).collect();
    }

    pub fn choose(&mut self) -> Option<&T> {
        let history_size = (self.items.len() / 2).max(2);
        let excess = self.recent_history.len().saturating_sub(history_size);
        self.recent_history.drain(..excess);

        let effective_weights: Vec<f32> = self
            .items
            .as_ref()
            .iter()
            .zip(&self.identities)
            .map(|(item, &identity)| item.weight() as f32 * self.penalty(identity))
            .collect();

        let idx = choose_index(&effective_weights)?;
        self.recent_history.push_back(self.identities[idx]);
        Some(&self.items[idx])
    }

    fn penalty(&self, identity: Identity) -> f32 {
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
