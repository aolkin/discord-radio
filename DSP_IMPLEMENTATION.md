# Audio DSP Implementation

This document describes the real-time audio DSP (Digital Signal Processing) system implemented for simulating weak radio signals with tuning behavior.

## Architecture Overview

The DSP system processes audio in real-time to simulate radio signal effects like noise, interference, and signal degradation. It's designed to work alongside the existing Songbird-based audio playback system.

### Core Components

```
Audio Files (Symphonia) → Custom Mixer → DSP Effects Chain → Songbird → Discord
                              ↑               ↑
                         Track Manager   Preset System
```

## Component Details

### 1. Custom Mixer (`audio/mixer.rs`)

Mixes multiple audio sources with independent volume controls.

- **AudioSource trait**: Interface for any audio input
- **MixerTrack**: Individual track with volume and loop settings
- **Mixer**: Combines multiple tracks frame-by-frame

### 2. Audio Decoder (`audio/decoder.rs`)

Wraps Symphonia decoder as an AudioSource.

- Decodes various audio formats (WAV, MP3, etc.)
- Handles mono→stereo conversion

### 3. DSP Effects Chain (`audio/dsp/chain.rs`)

Applies radio signal effects in sequence:

1. **Bandpass Filter** - Limits frequency range (typically 500Hz-5kHz for AM radio)
2. **Noise Mixing** - Adds white, pink, or brown noise
3. **Tremolo/Amplitude Modulation** - Simulates signal fading with frequency and amplitude jitter
4. **Soft Clipping** - Signal distortion
5. **Bitcrushing** (optional) - Reduces bit depth
6. **Random Dropouts** - Simulates signal interruptions

### 4. DSP Modules

#### Noise Generation (`audio/dsp/noise.rs`)

- **White Noise**: Uniform random noise across all frequencies
- **Pink Noise**: 1/f noise using Voss-McCartney algorithm
- **Brown Noise**: Brownian noise (random walk), lower frequency emphasis than pink noise

**Perceptual Noise Level Scaling:**

Noise levels use perceptual (dB-based) scaling for natural-sounding volume control:

- Config values range from 0.0 to 2.0 (typical usage 0.0-1.0)
- Mapping to perceived loudness:
  - `0.0` = silence (amplitude 0)
  - `0.5` = moderate noise (≈-30dB, amplitude ≈ 0.032)
  - `1.0` = reference level (0dB, amplitude = 1.0)
  - `2.0` = boosted (+18dB, amplitude ≈ 7.9)
- Internally converted to amplitude using: `amplitude = 10^(dB/20)`
- Preset transitions interpolate linearly in the perceptual (0.0-2.0) space. The dB-per-unit slope changes at 1.0, so a transition is linear in dB only while both levels stay on the same side of it

#### Modulation (`audio/dsp/modulation.rs`)

- **LFO**: Low-frequency oscillator for tremolo with dual jitter:
  - Frequency jitter: Randomly varies LFO rate (±70% at 50-200ms intervals)
  - Amplitude jitter: Randomly varies LFO depth (±30% at 30-150ms intervals) to make tremolo less predictable
- **Bitcrusher**: Reduces sample bit depth
- **DropoutGenerator**: Random silence periods

### 5. Preset System (`audio/presets.rs`)

JSON-based effect presets with smooth transitions.

- **SignalParameters**: Complete effect parameter set
- **PresetManager**: Loads presets from the `SIGNAL_PRESETS_PATH` directory (default `signal_presets/`; the value must end with a slash), under both the content root and the object storage bucket root
- **Preset Interpolation**: Smooth transitions between presets

### 6. Audio Processing Thread (`audio/processing_thread.rs`)

Continuous audio generation at 48kHz stereo, 20ms frames (960 samples).

- **AudioProcessor**: Owns mixer and DSP chain
- **ProcessingThread**: Async loop generating frames
- **SignalParametersTransition**: Smooth parameter interpolation

### 7. Songbird Integration (`audio/raw_adapter.rs`)

Bridges the DSP output to Songbird's voice system.

- **ProcessedAudioAdapter**: Manages processor lifecycle
- **State Integration**: Per-guild processors in BotState

## Signal Presets

The bot ships seven default presets, embedded from `src/audio/default_presets/`: `clear_signal`, `strong_signal`, `fading_signal`, `weak_signal`, `noise_pink`, `noise_brown` and `noise_white`. Defaults are read-only: saving over one returns 409 (use Save as), and a stored file with a default's name is skipped with a warning.

More sample presets are in `examples/signal_presets/`. The bot loads presets from the `SIGNAL_PRESETS_PATH` directory (default `signal_presets/`, under the content root); it does not load them from `examples/`.

### `clear_signal`

- No effects, full bandwidth (20Hz-20kHz)
- For clean audio playback

### `weak_signal`

- Heavy degradation: moderate pink noise (≈-24dB), 30% tremolo
- Frequent dropouts (30% probability)
- 8-bit bitcrushing
- Simulates very poor reception

### `detuned.json`

- Light white noise (≈-40dB)

### `fading_signal`

- Moderate effects: light pink noise (≈-34dB), 20% tremolo
- Rare dropouts (5% probability)
- Simulates a signal that is fading

### `strong_signal`

- Narrow bandpass (800Hz-4.5kHz)
- Minimal white noise (≈-40dB)

## Usage

### Discord Command

```
/dj signal_preset preset:clear_signal fade_duration:2.0
```

**Parameters:**

- `preset`: Preset name (for example clear_signal, strong_signal, fading_signal, weak_signal)
- `fade_duration`: Transition time in seconds (default: 2.0)

### Preset Format

```json
{
  "name": "preset_name",
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
  "dropout_duration_ms": [50.0, 150.0]
}
```

**Noise Level Parameters:**

Noise levels (`white_noise_level`, `pink_noise_level`, `brown_noise_level`) use **perceptual (dB-based) scaling**:

- Range: `0.0` to `2.0` (typical usage: `0.0` to `1.0`)
- `0.0` = silence (amplitude 0)
- `0.5` = moderate noise (≈-30dB) 
- `1.0` = reference level (0dB)
- `2.0` = boosted (+18dB)

This provides natural-feeling volume control matching human loudness perception. Values are internally converted to linear amplitude for mixing using the formula: `amplitude = 10^(dB/20)`.

## Technical Specifications

- **Sample Rate**: 48,000 Hz (Discord native)
- **Channels**: 2 (stereo)
- **Frame Size**: 960 samples (20ms)
- **Processing**: Real-time, lock-free parameter updates

## Implementation Status

### ✅ Completed

- All DSP components implemented and compiling
- Preset system with JSON loading
- Command interface (`/dj signal_preset`)
- State management in BotState
- Preset switching with smooth transitions

## Performance Considerations

- Uses `StdRng` for thread-safe random number generation
- Atomic float operations for lock-free parameter updates
- Custom noise/LFO implementations to avoid non-Sync dasp internals
- Frame-based processing minimizes overhead

## Volume Scaling

The audio system uses **perceptual (logarithmic) volume scaling** to provide more natural volume control that matches human loudness perception.

### User Volume Range

- **0.0**: Silence (amplitude 0)
- **1.0**: Unity gain (0 dB, original volume)
- **2.0**: Maximum boost (+18 dB, approximately 8x amplitude)

### Implementation

Volume values are converted to amplitude multipliers using the formula:

```
For volume in [0.0, 1.0]:
  dB = -60 + volume * 60
  
For volume in [1.0, 2.0]:
  dB = (volume - 1.0) * 18

amplitude = 10^(dB/20)
```

This provides:
- **Perceptually uniform changes**: Moving from 0.5 to 0.6 feels similar to 0.8 to 0.9
- **Boost capability**: Allows amplifying quiet tracks beyond their original volume
- **Industry-standard scaling**: Matches behavior of professional audio software

### Usage

All volume parameters in the system (config files, Discord commands, API calls) accept the 0.0-2.0 range:

```json
{
  "volume": 1.5  // +9dB boost above original
}
```

The conversion to amplitude happens internally in the mixer, so all user-facing interfaces remain simple and consistent.
