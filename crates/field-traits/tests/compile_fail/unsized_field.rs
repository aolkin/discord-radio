use field_traits::{field_trait, with_trait_fields};

#[field_trait]
trait Tagged {
    fn tags(&self) -> &[String];
}

#[with_trait_fields(Tagged)]
struct Track {
    weight: u32,
}

fn main() {}
