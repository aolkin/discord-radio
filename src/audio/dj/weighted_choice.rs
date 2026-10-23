use field_trait::field_trait;
use rand::RngExt;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::Arc;

/// A weight an item contributes to weighted random selection.
#[field_trait]
pub trait Weighted {
    fn weight(&self) -> u32;
}

#[field_trait]
pub trait WeightedPool<T: Weighted> {
    fn entries(&self) -> &Arc<[T]>;
    fn history_penalty_multiplier(&self) -> Option<f32>;
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
    recent_history: VecDeque<usize>,
    history_size: usize,
    penalty_multiplier: f32,
    item: PhantomData<fn() -> T>,
}

impl<T: Weighted, P: WeightedPool<T>> From<&P> for WeightedSelector<T> {
    fn from(pool: &P) -> Self {
        Self::new(pool.entries().clone(), pool.history_penalty_multiplier())
    }
}

impl<T: Weighted, S: AsRef<[T]>> WeightedSelector<T, S> {
    /// Remembers the last half of the item count in picks, at least 2.
    /// The penalty multiplier defaults to 1.
    pub fn new(items: S, history_penalty_multiplier: Option<f32>) -> Self {
        let history_size = (items.as_ref().len() / 2).max(2);
        Self {
            items,
            recent_history: VecDeque::with_capacity(history_size),
            history_size,
            penalty_multiplier: history_penalty_multiplier.unwrap_or(1.0),
            item: PhantomData,
        }
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
            .enumerate()
            .map(|(idx, item)| item.weight() as f32 * self.penalty(idx))
            .collect();

        let idx = choose_index(&effective_weights)?;
        if self.recent_history.len() >= self.history_size {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(idx);
        Some(idx)
    }

    fn penalty(&self, idx: usize) -> f32 {
        match self
            .recent_history
            .iter()
            .rposition(|&recent| recent == idx)
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
