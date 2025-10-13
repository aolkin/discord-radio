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

**Perceptual Noise Level Scaling:**

Noise levels use perceptual (dB-based) scaling for natural-sounding volume control:

- Config values range from 0.0 to 2.0 (typical usage 0.0-1.0)
- Mapping to perceived loudness:
  - `0.0` = silence (≈-60dB, amplitude ≈ 0.001)
  - `0.5` = moderate noise (≈-30dB, amplitude ≈ 0.032)
  - `1.0` = reference level (0dB, amplitude = 1.0)
  - `2.0` = boosted (+6dB, amplitude ≈ 2.0)
- Internally converted to amplitude using: `amplitude = 10^(dB/20)`
- Profile transitions interpolate linearly in the perceptual (0.0-2.0) space, which is equivalent to linear interpolation in dB space

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

- Heavy degradation: moderate pink noise (≈-24dB), 30% tremolo
- Frequent dropouts (30% probability)
- 8-bit bitcrushing
- Simulates very poor reception

### `detuned.json`

- 3Hz frequency warble
- Light white noise (≈-40dB)
- Moderate tremolo (40% depth @ 1.5Hz)

### `tuning.json`

- Moderate effects: light pink noise (≈-34dB), 20% tremolo
- Rare dropouts (5% probability)
- Simulates active tuning

### `locked.json`

- Narrow bandpass (800Hz-4.5kHz)
- Minimal white noise (≈-40dB)
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
  "white_noise_level": 0.3,
  "pink_noise_level": 0.0,
  "brown_noise_level": 0.0,
  "tremolo_depth": 0.4,
  "tremolo_rate": 2.0,
  "clip_pregain": 1.0,
  "clip_threshold": 0.8,
  "bitcrush_bits": null,
  "dropout_probability": 0.1,
  "dropout_duration_ms": [50.0, 150.0],
  "pitch_shift_cents": null,
  "frequency_warble_hz": null
}
```

**Noise Level Parameters:**

Noise levels (`white_noise_level`, `pink_noise_level`, `brown_noise_level`) use **perceptual (dB-based) scaling**:

- Range: `0.0` to `2.0` (typical usage: `0.0` to `1.0`)
- `0.0` = silence (≈-60dB)
- `0.5` = moderate noise (≈-30dB) 
- `1.0` = reference level (0dB)
- `2.0` = boosted (+6dB)

This provides natural-feeling volume control matching human loudness perception. Values are internally converted to linear amplitude for mixing using the formula: `amplitude = 10^(dB/20)`.

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

## Performance Considerations

- Uses `StdRng` for thread-safe random number generation
- Atomic float operations for lock-free parameter updates
- DirectForm2Transposed biquad filters for efficiency
- Custom noise/LFO implementations to avoid non-Sync dasp internals
- Frame-based processing minimizes overhead
