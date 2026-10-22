use field_trait::field_trait;

#[field_trait]
trait Unsupported<'a, const N: usize> {
    fn value(&self) -> u32;
}

fn main() {}
