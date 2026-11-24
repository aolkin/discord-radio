pub mod chain;
pub mod effects;
pub mod filter;
pub mod lfo;

use ambassador::delegatable_trait;

#[delegatable_trait]
pub trait FrameProcessor {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2];
}

pub trait Effect: FrameProcessor {
    type Params;

    fn new(sample_rate: u32, params: &Self::Params) -> Self;
    fn update(&mut self, params: &Self::Params);
}
