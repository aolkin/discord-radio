use super::*;
use tokio::sync::oneshot;

struct Constant;

impl AudioSource for Constant {
    fn next_frame(&mut self) -> Option<[f32; 2]> {
        Some([1.0, 1.0])
    }
    fn seek(&mut self, _: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    fn duration(&self) -> Option<Duration> {
        None
    }
    fn reset(&mut self) {}
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn mixer_with_track() -> (Mixer, TrackHandle, oneshot::Receiver<StopReason>) {
    let mut mixer = Mixer::new(1000);
    let (callback, ended) = TrackCallback::oneshot();
    let handle = mixer.add_track(
        Box::new(Constant),
        1.0,
        vec![TrackCommand::AddStopTrigger(callback)],
    );
    (mixer, handle, ended)
}

fn pull(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    mixer.drain_commands();
    (0..frames).map(|_| mixer.mix_next_frame()[0]).collect()
}

#[test]
fn volume_ramp_started_late_ends_after_its_own_length() {
    let (mut mixer, handle, _) = mixer_with_track();
    pull(&mut mixer, 1000);
    handle
        .send(TrackCommand::SetVolume {
            volume: 0.0,
            fade: ms(1000),
            at: None,
        })
        .unwrap();
    let ramp = pull(&mut mixer, 1000);
    assert!(ramp[500] > 0.0 && ramp[500] < 1.0);
    assert_eq!(pull(&mut mixer, 1), [0.0]);
}

#[test]
fn stop_fades_then_removes_the_track() {
    let (mut mixer, handle, mut ended) = mixer_with_track();
    handle
        .send(TrackCommand::Stop {
            fade: ms(100),
            at: Some(ms(200)),
        })
        .unwrap();
    let out = pull(&mut mixer, 300);
    assert_eq!(out[150], 1.0);
    assert!(out[250] > 0.0 && out[250] < 1.0);
    assert!(ended.try_recv().is_err());
    assert_eq!(pull(&mut mixer, 1), [0.0]);
    assert_eq!(ended.try_recv(), Ok(StopReason::Stopped));

    let (mut mixer, handle, mut ended) = mixer_with_track();
    pull(&mut mixer, 500);
    handle
        .send(TrackCommand::Stop {
            fade: ms(100),
            at: Some(ms(100)),
        })
        .unwrap();
    let out = pull(&mut mixer, 100);
    assert!(out[50] > 0.0 && out[50] < 1.0);
    assert!(ended.try_recv().is_err());
    pull(&mut mixer, 1);
    assert!(ended.try_recv().is_ok());

    let (mut mixer, handle, mut ended) = mixer_with_track();
    pull(&mut mixer, 10);
    handle
        .send(TrackCommand::Stop {
            fade: Duration::ZERO,
            at: None,
        })
        .unwrap();
    assert_eq!(pull(&mut mixer, 1), [0.0]);
    assert_eq!(ended.try_recv(), Ok(StopReason::Stopped));
    assert!(!handle.send(TrackCommand::SetLoops(true)).unwrap());
}

#[test]
fn trigger_fires_at_its_frame_even_when_a_stop_lands_there() {
    let (mut mixer, handle, _) = mixer_with_track();
    let (callback, mut fired) = TrackCallback::oneshot();
    handle
        .send(TrackCommand::AddTrigger {
            at: ms(100),
            callback,
        })
        .unwrap();
    pull(&mut mixer, 100);
    assert!(fired.try_recv().is_err());
    pull(&mut mixer, 1);
    assert!(fired.try_recv().is_ok());

    let (mut mixer, handle, mut ended) = mixer_with_track();
    let (callback, mut fired) = TrackCallback::oneshot();
    handle
        .send(TrackCommand::AddTrigger {
            at: ms(100),
            callback,
        })
        .unwrap();
    handle
        .send(TrackCommand::Stop {
            fade: Duration::ZERO,
            at: Some(ms(100)),
        })
        .unwrap();
    pull(&mut mixer, 101);
    assert!(fired.try_recv().is_ok());
    assert_eq!(ended.try_recv(), Ok(StopReason::Stopped));
}
