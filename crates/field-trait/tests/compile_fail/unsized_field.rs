use field_trait::{field_trait, with_trait_fields};

#[field_trait]
trait Tagged {
    fn tags(&self) -> &[String];
}

#[with_trait_fields(Tagged)]
struct Widget {
    weight: u32,
}

fn main() {}
