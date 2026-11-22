use crate::audio::presets::Layer;

pub mod chain;
pub mod filter;
pub mod modulation;
pub mod noise;

pub trait FrameProcessor {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2];
}

impl<P: FrameProcessor> FrameProcessor for Vec<P> {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.iter_mut()
            .fold(frame, |frame, processor| processor.process_frame(frame))
    }
}

impl<P: FrameProcessor> FrameProcessor for Option<P> {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        match self {
            Some(processor) => processor.process_frame(frame),
            None => frame,
        }
    }
}

pub trait LayerProcessor: FrameProcessor {
    type Layer: Layer;

    fn new(sample_rate: u32, layer: &Self::Layer) -> Self;
    fn kind(&self) -> <Self::Layer as Layer>::Kind;
    fn update(&mut self, layer: &Self::Layer);
}

pub fn update_layers<P: LayerProcessor>(
    processors: &mut Vec<P>,
    layers: &[P::Layer],
    sample_rate: u32,
) {
    let mut unused = std::mem::take(processors);
    for layer in layers {
        match unused
            .iter()
            .position(|processor| processor.kind() == layer.kind())
        {
            Some(index) => {
                let mut processor = unused.remove(index);
                processor.update(layer);
                processors.push(processor);
            }
            None => processors.push(P::new(sample_rate, layer)),
        }
    }
}
