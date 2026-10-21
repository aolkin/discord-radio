use rand::RngExt;
use std::collections::VecDeque;

/// A weight an item contributes to weighted random selection.
pub trait Weighted {
    fn weight(&self) -> f32;

    /// Identifies the item in recent-history across list changes.
    fn key(&self) -> &str;
}

/// Picks an index at random in proportion to `weights`, or `None` when they
/// sum to zero.
pub fn choose_index(weights: &[f32]) -> Option<usize> {
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
pub struct WeightedSelector {
    recent_history: VecDeque<String>,
    history_size: usize,
    penalty_multiplier: f32,
}

impl WeightedSelector {
    pub fn new(history_size: usize, penalty_multiplier: f32) -> Self {
        Self {
            recent_history: VecDeque::with_capacity(history_size),
            history_size,
            penalty_multiplier,
        }
    }

    pub fn choose<'a, T: Weighted>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let effective_weights: Vec<f32> = items
            .iter()
            .map(|item| item.weight() * self.get_penalty_for_key(item.key()))
            .collect();

        let Some(idx) = choose_index(&effective_weights) else {
            return items.first();
        };
        let item = &items[idx];
        self.add_to_history(item);
        Some(item)
    }

    fn get_penalty_for_key(&self, key: &str) -> f32 {
        for (history_idx, item_key) in self.recent_history.iter().enumerate() {
            if item_key == key {
                let recency_factor = 1.0 - (history_idx as f32 / self.recent_history.len() as f32);
                return self.penalty_multiplier * recency_factor + (1.0 - self.penalty_multiplier);
            }
        }
        1.0
    }

    /// Records `item` into recent-history without selecting, so a
    /// subsequent `choose` treats it as already having just been picked.
    pub fn add_to_history<T: Weighted>(&mut self, item: &T) {
        if self.recent_history.len() >= self.history_size {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(item.key().to_owned());
    }
}
