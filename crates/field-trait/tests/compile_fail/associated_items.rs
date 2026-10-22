use field_trait::{field_trait, field_trait_impl};

#[field_trait]
trait AssociatedItems {
    type Item;
    const LIMIT: u32;
    fn weight(&self) -> u32;
}

#[field_trait_impl(AssociatedItems)]
struct Widget {
    weight: u32,
}

fn main() {}
