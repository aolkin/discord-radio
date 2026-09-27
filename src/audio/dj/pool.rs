use crate::audio::dj::weighted_choice::WeightedSelector;
use std::sync::Arc;

/// A weight an item contributes to weighted random selection within a `Pool`.
pub trait Weighted {
    fn weight(&self) -> u32;
}

/// Picks an index into an owned item list: weighted by `Weighted::weight`,
/// with recent-history avoidance and a duplicate penalty from
/// `WeightedSelector`.
///
/// `set_items` swaps the owned list (e.g. on a config reload) without
/// resetting the selector, so recent-history state carries over across the
/// swap.
pub struct Pool<T> {
    selector: WeightedSelector,
    items: Arc<[T]>,
}

impl<T: Weighted> Pool<T> {
    pub fn new(
        recent_history_size: usize,
        duplicate_penalty_multiplier: f32,
        items: Arc<[T]>,
    ) -> Self {
        Self {
            selector: WeightedSelector::new(recent_history_size, duplicate_penalty_multiplier),
            items,
        }
    }

    pub fn set_items(&mut self, items: Arc<[T]>) {
        self.items = items;
    }

    pub fn next(&mut self) -> usize {
        self.selector.choose(&self.items, T::weight)
    }
}
