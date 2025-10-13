# Audio DSP Implementation

This document describes the real-time audio DSP (Digital Signal Processing) system implemented for simulating weak radio signals with tuning behavior.

## Architecture Overview

The DSP system processes audio in real-time to simulate radio signal effects like noise, interference, and signal degradation. It's designed to work alongside the existing Songbird-based audio playback system.

### Core Components

```
Audio Files (Symphonia) → Custom Mixer → DSP Effects Chain → Songbird → Discord
                              ↑               ↑
                         Track Manager   Profile System
```

## Component Details

### 1. Custom Mixer (`audio/custom_mixer.rs`)

Mixes multiple audio sources with independent volume controls.

- **AudioSource trait**: Interface for any audio input
- **MixerTrack**: Individual track with volume and loop settings
- **CustomMixer**: Combines multiple tracks frame-by-frame

### 2. Audio Decoder (`audio/decoder.rs`)

Wraps Symphonia decoder as an AudioSource.

- Decodes various audio formats (WAV, MP3, FLAC, etc.)
- Handles mono→stereo conversion
- Sample rate conversion (placeholder for future implementation)

### 3. DSP Effects Chain (`audio/dsp/chain.rs`)

Applies radio signal effects in sequence:

1. **Bandpass Filter** - Limits frequency range (typically 500Hz-5kHz for AM radio)
2. **Noise Mixing** - Adds white, pink, or brown noise
3. **Tremolo/Amplitude Modulation** - Simulates signal fading
4. **Pitch Shifting** (optional) - Frequency detuning
5. **Soft Clipping** - Signal distortion
6. **Bitcrushing** (optional) - Reduces bit depth
7. **Random Dropouts** - Simulates signal interruptions

### 4. DSP Modules

#### Noise Generation (`audio/dsp/noise.rs`)

- **White Noise**: Uniform random noise across all frequencies
- **Pink Noise**: 1/f noise using Voss-McCartney algorithm
- **Brown Noise**: Brownian noise (random walk), lower frequency emphasis than pink noise

#### Modulation (`audio/dsp/modulation.rs`)

- **LFO**: Low-frequency oscillator for tremolo
- **PitchShifter**: Simple time-domain pitch shifting
- **Bitcrusher**: Reduces sample bit depth
- **DropoutGenerator**: Random silence periods

### 5. Profile System (`audio/profiles.rs`)

JSON-based effect presets with smooth transitions.

- **SignalProfile**: Complete effect parameter set
- **ProfileManager**: Loads profiles from `audio_profiles/` directory
- **Profile Interpolation**: Smooth transitions between profiles

### 6. Audio Processing Thread (`audio/processing_thread.rs`)

Continuous audio generation at 48kHz stereo, 20ms frames (960 samples).

- **AudioProcessor**: Owns mixer and DSP chain
- **ProcessingThread**: Async loop generating frames
- **ProfileTransition**: Smooth parameter interpolation

### 7. Songbird Integration (`audio/raw_adapter.rs`)

Bridges the DSP output to Songbird's voice system.

- **ProcessedAudioAdapter**: Manages processor lifecycle
- **State Integration**: Per-guild processors in BotState

## Signal Profiles

Pre-configured effect presets in `audio_profiles/`:

### `clear.json`

- No effects, full bandwidth (20Hz-20kHz)
- For clean audio playback

### `weak_signal.json`

- Heavy degradation: 50% pink noise, 60% tremolo
- Frequent dropouts (30% probability)
- 8-bit bitcrushing
- Simulates very poor reception

### `detuned.json`

- -50 cents pitch shift
- 3Hz frequency warble
- 40% white noise
- Moderate tremolo (40% depth @ 1.5Hz)

### `tuning.json`

- Moderate effects (20% pink noise, 20% tremolo)
- Rare dropouts (5% probability)
- Simulates active tuning

### `locked.json`

- Narrow bandpass (800Hz-4.5kHz)
- Minimal noise (5%)
- Clean locked signal

## Usage

### Discord Command

```
/signal_profile profile:clear fade_duration:2.0
```

**Parameters:**

- `profile`: Profile name (clear, weak_signal, detuned, tuning, locked)
- `fade_duration`: Transition time in seconds (default: 2.0)

### Profile Format

```json
{
  "name": "profile_name",
  "bandpass_low": 500.0,
  "bandpass_high": 5000.0,
  "noise_level": 0.3,
  "noise_type": "pink",
  "tremolo_depth": 0.4,
  "tremolo_rate": 2.0,
  "clip_threshold": 0.8,
  "bitcrush_bits": null,
  "dropout_probability": 0.1,
  "dropout_duration_ms": [50.0, 150.0],
  "pitch_shift_cents": null,
  "frequency_warble_hz": null
}
```

## Technical Specifications

- **Sample Rate**: 48,000 Hz (Discord native)
- **Channels**: 2 (stereo)
- **Frame Size**: 960 samples (20ms)
- **Processing**: Real-time, lock-free parameter updates
- **Bit Depth**: 16-bit PCM output

## Implementation Status

### ✅ Completed

- All DSP components implemented and compiling
- Profile system with JSON loading
- Command interface (`/signal_profile`)
- State management in BotState
- Profile switching with smooth transitions

### 🔧 Remaining Work

- **TrackManager Integration**: Connect custom mixer to existing track system
- **Processor Initialization**: Create processors when joining voice channels
- **Audio Pipeline**: Wire ProcessedAudioAdapter output to Songbird

The DSP infrastructure is complete and ready for integration with the existing audio playback system.

## Dependencies

```toml
dasp = { version = "0.11", features = ["signal", "interpolate", "ring_buffer"] }
dasp_signal = "0.11"
dasp_interpolate = "0.11"
dasp_sample = "0.11"
biquad = "0.4"
rubato = "0.15"
atomic_float = "1.0"
```

## Performance Considerations

- Uses `StdRng` for thread-safe random number generation
- Atomic float operations for lock-free parameter updates
- DirectForm2Transposed biquad filters for efficiency
- Custom noise/LFO implementations to avoid non-Sync dasp internals
- Frame-based processing minimizes overhead

## Future Enhancements

Potential improvements:

- Actual resampling implementation in decoder
- More sophisticated pitch shifting (FFT-based)
- Additional filter types (high-pass, low-pass, notch)
- Compression/limiting
- Reverb/echo effects
- Profile scheduling/automation
- Real-time spectrum analysis
