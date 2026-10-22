use field_traits::{field_impl, field_trait};

#[field_trait]
trait AssociatedItems {
    type Item;
    const LIMIT: u32;
    fn weight(&self) -> u32;
}

#[field_impl(AssociatedItems)]
struct Track {
    weight: u32,
}

fn main() {}
