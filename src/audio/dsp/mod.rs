use crate::audio::presets::ListEntry;

pub mod chain;
pub mod effects;
pub mod lfo;

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

pub trait EntryProcessor: FrameProcessor {
    type Entry: ListEntry;

    fn new(sample_rate: u32, entry: &Self::Entry) -> Self;
    fn kind(&self) -> <Self::Entry as ListEntry>::Kind;
    fn update(&mut self, entry: &Self::Entry);
}

pub fn update_entries<P: EntryProcessor>(
    processors: &mut Vec<P>,
    entries: &[P::Entry],
    sample_rate: u32,
) {
    let mut unused = std::mem::take(processors);
    for entry in entries {
        match unused
            .iter()
            .position(|processor| processor.kind() == entry.kind())
        {
            Some(index) => {
                let mut processor = unused.remove(index);
                processor.update(entry);
                processors.push(processor);
            }
            None => processors.push(P::new(sample_rate, entry)),
        }
    }
}
