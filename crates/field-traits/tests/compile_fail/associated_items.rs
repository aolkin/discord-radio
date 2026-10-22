use field_traits::field_trait;

#[field_trait]
trait AssociatedItems {
    type Item;
    const LIMIT: u32;
}

fn main() {}
