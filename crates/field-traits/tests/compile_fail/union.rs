use field_traits::{field_impl, field_trait};

#[field_trait]
trait Weighted {
    fn weight(&self) -> u32;
}

#[field_impl(Weighted)]
union Bits {
    weight: u32,
}

fn main() {}
