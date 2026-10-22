use field_trait::field_trait;

#[field_trait]
trait Receiver {
    fn by_value(self) -> u32;
    fn by_mut(&mut self) -> u32;
    fn extra_argument(&self, scale: u32) -> u32;
    fn no_receiver() -> u32;
}

fn main() {}
