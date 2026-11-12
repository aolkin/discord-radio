use ambassador::delegatable_trait;
use field_trait::field_trait;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::audio::dj::config::Order;

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
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
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

/// Both orders share one history, so switching order keeps the place.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WeightedSelector {
    recent_history: VecDeque<Played>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Played {
    identity: Identity,
    index: usize,
}

impl WeightedSelector {
    pub fn choose<'a, T: Weighted>(&mut self, order: Order, items: &'a [T]) -> Option<&'a T> {
        match order {
            Order::Weighted => self.choose_weighted(items),
            Order::Sequential => self.choose_sequential(items),
        }
    }

    fn choose_weighted<'a, T: Weighted>(&mut self, items: &'a [T]) -> Option<&'a T> {
        self.trim(items.len());

        let identities: Vec<Identity> = items.iter().map(Weighted::identity).collect();
        let effective_weights: Vec<f32> = items
            .iter()
            .zip(&identities)
            .map(|(item, &identity)| item.weight() as f32 * self.penalty(identity))
            .collect();

        let index = choose_index(&effective_weights)?;
        self.recent_history.push_back(Played {
            identity: identities[index],
            index,
        });
        Some(&items[index])
    }

    /// Returns the index of an entry not yet in the history, or `None` once
    /// none are left (weight 0 entries never play under `Weighted`).
    pub fn choose_unplayed<T: Weighted>(&mut self, order: Order, items: &[T]) -> Option<usize> {
        let index = match order {
            Order::Weighted => {
                let identities: Vec<Identity> = items.iter().map(Weighted::identity).collect();
                let effective_weights: Vec<f32> = items
                    .iter()
                    .zip(&identities)
                    .map(|(item, identity)| {
                        let played = self
                            .recent_history
                            .iter()
                            .any(|recent| recent.identity == *identity);
                        if played { 0.0 } else { item.weight() as f32 }
                    })
                    .collect();
                choose_index(&effective_weights)?
            }
            Order::Sequential => self.last_index(items).map_or(0, |index| index + 1),
        };
        let item = items.get(index)?;
        self.recent_history.push_back(Played {
            identity: item.identity(),
            index,
        });
        Some(index)
    }

    fn last_index<T: Weighted>(&self, items: &[T]) -> Option<usize> {
        self.recent_history.back().and_then(|last| {
            if items.get(last.index).map(Weighted::identity) == Some(last.identity) {
                Some(last.index)
            } else {
                items
                    .iter()
                    .position(|item| item.identity() == last.identity)
            }
        })
    }

    fn choose_sequential<'a, T: Weighted>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        self.trim(items.len());

        let index = self
            .last_index(items)
            .map_or(0, |index| (index + 1) % items.len());
        self.recent_history.push_back(Played {
            identity: items[index].identity(),
            index,
        });
        Some(&items[index])
    }

    fn trim(&mut self, item_count: usize) {
        let excess = self.recent_history.len().saturating_sub(item_count);
        self.recent_history.drain(..excess);
    }

    fn penalty(&self, identity: Identity) -> f32 {
        match self
            .recent_history
            .iter()
            .rposition(|recent| recent.identity == identity)
        {
            Some(history_idx) => 1.0 - (history_idx as f32 / self.recent_history.len() as f32),
            None => 1.0,
        }
    }
}
