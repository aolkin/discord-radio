export const clamp = (value, low, high) => Math.min(Math.max(value, low), high);
const clamp01 = (value) => clamp(value, 0, 1);
export const isNumber = (value) => typeof value === 'number' && !Number.isNaN(value);
export const roundTo = (value, decimals) => Number(value.toFixed(decimals));

export function hertz(value) {
    return value >= 1000 ? `${roundTo(value / 1000, 2)} kHz` : `${Math.round(value)} Hz`;
}

function levelDb(amount) {
    if (!isNumber(amount) || amount <= 0) {
        return null;
    }
    return amount < 1 ? -60 + 60 * amount : (amount - 1) * 18;
}

function dbText(db) {
    if (db === null) {
        return 'off';
    }
    const rounded = Math.round(db);
    const sign = rounded > 0 ? '+' : rounded < 0 ? '−' : '';
    return `${sign}${Math.abs(rounded)} dB`;
}

export function every(perSecond) {
    const seconds = 1 / perSecond;
    return seconds >= 10 ? `${Math.round(seconds)} s` : `${seconds.toFixed(1)} s`;
}

const zeroStop = 0.04;

const control = (stage, key, name, field, options) => ({ id: `${stage}.${key}`, stage, key, name, field, ...options });

const level = (key, name, color) =>
    control('noise', key, name, `noise.${key}.level`, {
        min: 0,
        max: 2,
        scale: 'linear',
        low: 0,
        high: 2,
        decimals: 3,
        unit: '',
        color,
        meaning: (value) => dbText(levelDb(value)),
    });

const noiseControls = [
    level('white', 'Hiss', '--chain-teal'),
    level('pink', 'Static', '--chain-rose'),
    level('brown', 'Rumble', '--chain-amber'),
];

const filterControls = [
    control('filters', 'highpass', 'Low cut', 'filters.highpass.frequency', {
        min: 20,
        max: 20000,
        scale: 'log',
        low: 20,
        high: 20000,
        decimals: 0,
        unit: ' Hz',
        color: '--chain-teal',
        meaning: (value) => (value <= 20 ? 'keeps all bass' : `cuts below ${hertz(value)}`),
    }),
    control('filters', 'lowpass', 'High cut', 'filters.lowpass.frequency', {
        min: 20,
        max: 20000,
        scale: 'log',
        low: 20,
        high: 20000,
        decimals: 0,
        unit: ' Hz',
        color: '--chain-rose',
        meaning: (value) => (value >= 20000 ? 'keeps all treble' : `cuts above ${hertz(value)}`),
    }),
];

const tremoloControls = [
    control('tremolo', 'depth', 'Depth', 'tremolo.depth', {
        min: 0,
        max: 1,
        scale: 'linear',
        low: 0,
        high: 1,
        decimals: 2,
        unit: '',
        color: '--chain-indigo',
        meaning: (value) => (value <= 0 ? 'no fading' : `dips to ${Math.round((1 - value) * 100)}%`),
    }),
    control('tremolo', 'rate', 'Speed', 'tremolo.rate', {
        min: 0,
        max: 20,
        scale: 'log',
        low: 0.1,
        high: 20,
        decimals: 2,
        unit: ' Hz',
        color: '--chain-amber',
        meaning: (value) => (value <= 0 ? 'volume frozen' : `one fade every ${every(value)}`),
    }),
    control('tremolo', 'jitter', 'Unevenness', 'tremolo.jitter', {
        min: 0,
        max: 1,
        scale: 'linear',
        low: 0,
        high: 1,
        decimals: 2,
        unit: '',
        color: '--chain-rose',
        meaning: (value) =>
            value <= 0 ? 'every fade the same' : `fade size ±${Math.round(Math.min(value, 0.5) * 100)}%`,
    }),
];

const clipControls = [
    control('clip', 'pregain', 'Drive', 'clip.pregain', {
        min: 0,
        max: 100,
        scale: 'log',
        low: 0.1,
        high: 100,
        decimals: 2,
        unit: '×',
        color: '--chain-amber',
        meaning: (value) => {
            if (value === 1) {
                return 'no boost';
            }
            return value <= 0 ? 'silent' : dbText(20 * Math.log10(value));
        },
    }),
    control('clip', 'threshold', 'Clip level', 'clip.threshold', {
        min: 0,
        max: 2,
        scale: 'linear',
        low: 0,
        high: 2,
        decimals: 2,
        unit: '',
        color: '--chain-teal',
        meaning: (value) => (value <= 0 ? 'silent' : `${Math.round(value * 100)}% of full scale`),
    }),
];

const bitcrushControls = [
    control('bitcrush', 'bits', 'Bits', 'bitcrush.bits', {
        min: 1,
        max: 16,
        scale: 'linear',
        low: 1,
        high: 16,
        decimals: 0,
        unit: '',
        integer: true,
        nullable: true,
        color: '--chain-violet',
        meaning: (value) => (value === null ? 'off' : `steps of 1/${2 ** (value - 1)}`),
    }),
];

const dropoutLength = (key, name, color) =>
    control('dropout', key, name, `dropout.${key}`, {
        min: 0,
        max: 2000,
        scale: 'log',
        low: 1,
        high: 2000,
        decimals: 0,
        unit: ' ms',
        color,
        meaning: () => '',
    });

const dropoutControls = [
    control('dropout', 'probability', 'Rate', 'dropout.probability', {
        min: 0,
        max: 1,
        scale: 'linear',
        low: 0,
        high: 1,
        decimals: 2,
        unit: '/s',
        color: '--chain-indigo',
        meaning: (value) => (value <= 0 ? 'no dropouts' : `about one every ${every(value)}`),
    }),
    dropoutLength('min_ms', 'Shortest', '--chain-teal'),
    dropoutLength('max_ms', 'Longest', '--chain-rose'),
];

const allowsZero = (spec) => spec.scale === 'log' && spec.min <= 0;

export function toPosition(spec, value) {
    if (!isNumber(value)) {
        return 0;
    }
    if (spec.scale === 'linear') {
        return clamp01((value - spec.low) / (spec.high - spec.low));
    }
    if (value <= 0) {
        return 0;
    }
    const t = clamp01(Math.log(value / spec.low) / Math.log(spec.high / spec.low));
    return allowsZero(spec) ? zeroStop + (1 - zeroStop) * t : t;
}

export function fromPosition(spec, position) {
    const t = clamp01(position);
    let value;
    if (spec.scale === 'linear') {
        value = spec.low + t * (spec.high - spec.low);
    } else if (allowsZero(spec) && t < zeroStop / 2) {
        value = 0;
    } else {
        const stretched = allowsZero(spec) ? clamp01((t - zeroStop) / (1 - zeroStop)) : t;
        value = spec.low * (spec.high / spec.low) ** stretched;
    }
    if (spec.integer || (spec.scale !== 'linear' && value >= 100)) {
        return Math.round(value);
    }
    return roundTo(value, spec.decimals);
}

export function formatValue(spec, value) {
    if (value === null) {
        return 'Off';
    }
    return isNumber(value) ? `${roundTo(value, spec.decimals)}${spec.unit}` : '—';
}

export function limitText(spec) {
    return `${spec.field} · server limit ${spec.min}–${spec.max}${spec.nullable ? ' or off' : ''}`;
}

const anyNoise = (noise) => noiseControls.some(({ key }) => noise[key] > 0);

export const stages = [
    {
        id: 'noise',
        name: 'Noise',
        description: 'Adds hiss, static and rumble under the music.',
        faders: true,
        controls: noiseControls,
        off: { white: 0, pink: 0, brown: 0 },
        on: { pink: 0.4 },
        isOn: anyNoise,
        readout: (noise) =>
            anyNoise(noise)
                ? noiseControls
                      .filter(({ key }) => noise[key] > 0)
                      .map((spec) => `${spec.name} ${dbText(levelDb(noise[spec.key]))}`)
                      .join(' · ')
                : 'off',
    },
    {
        id: 'filters',
        name: 'Frequency range',
        description: 'Keeps only the frequencies between the low cut and the high cut.',
        controls: filterControls,
        off: { highpass: 20, lowpass: 20000 },
        on: { highpass: 500, lowpass: 5000 },
        isOn: (filters) => filters.highpass > 20 || filters.lowpass < 20000,
        readout: (filters) =>
            filters.highpass > 20 || filters.lowpass < 20000
                ? `${hertz(filters.highpass)} – ${hertz(filters.lowpass)}`
                : 'full range',
    },
    {
        id: 'tremolo',
        name: 'Fading',
        description: 'Makes the volume dip and swell like a drifting signal.',
        controls: tremoloControls,
        off: { depth: 0 },
        on: { depth: 0.3 },
        isOn: (tremolo) => tremolo.depth > 0,
        afterOn: (tremolo) => {
            if (!(tremolo.rate > 0)) {
                tremolo.rate = 1;
            }
        },
        readout: (tremolo) =>
            tremolo.depth > 0
                ? `dips to ${Math.round((1 - tremolo.depth) * 100)}% · ${tremolo.rate > 0 ? `every ${every(tremolo.rate)}` : 'frozen'}`
                : 'off',
    },
    {
        id: 'clip',
        name: 'Clipping',
        description: 'Drive boosts the music, and the clip level squashes anything louder into distortion.',
        controls: clipControls,
        off: { pregain: 1, threshold: 1 },
        on: { threshold: 0.8 },
        isOn: (clip) => clip.pregain !== 1 || clip.threshold !== 1,
        readout: (clip) =>
            clip.pregain !== 1 || clip.threshold !== 1 ? `drive ×${clip.pregain} · clip ${clip.threshold}` : 'off',
    },
    {
        id: 'bitcrush',
        name: 'Bit depth',
        description: 'Rounds every sample to a coarse step for a grainy digital sound.',
        controls: bitcrushControls,
        off: { bits: null },
        on: { bits: 8 },
        isOn: (bitcrush) => bitcrush.bits !== null,
        readout: (bitcrush) => (bitcrush.bits === null ? 'off' : `${bitcrush.bits} bits`),
    },
    {
        id: 'dropout',
        name: 'Dropouts',
        description: 'Cuts the sound to silence for short moments.',
        controls: dropoutControls,
        off: { probability: 0 },
        on: { probability: 0.1 },
        isOn: (dropout) => dropout.probability > 0,
        afterOn: (dropout) => {
            if (!(dropout.max_ms > 0)) {
                Object.assign(dropout, { min_ms: 25, max_ms: 100 });
            }
        },
        readout: (dropout) =>
            dropout.probability > 0
                ? `one every ${every(dropout.probability)} · ${dropout.min_ms}–${dropout.max_ms} ms`
                : 'off',
    },
];

export const controls = stages.flatMap((stage) => stage.controls);

export function keepOrdered(form, spec, value) {
    const stage = form[spec.stage];
    if (spec.id === 'filters.highpass' && isNumber(stage.lowpass)) {
        return Math.min(value, stage.lowpass - 1);
    }
    if (spec.id === 'filters.lowpass' && isNumber(stage.highpass)) {
        return Math.max(value, stage.highpass + 1);
    }
    if (spec.id === 'dropout.min_ms' && isNumber(stage.max_ms)) {
        return Math.min(value, stage.max_ms);
    }
    if (spec.id === 'dropout.max_ms' && isNumber(stage.min_ms)) {
        return Math.max(value, stage.min_ms);
    }
    return value;
}

function controlError(spec, value) {
    if (value === null && spec.nullable) {
        return null;
    }
    if (!isNumber(value)) {
        return `${spec.name} needs a number.`;
    }
    if (spec.integer && !Number.isInteger(value)) {
        return `${spec.name} must be a whole number.`;
    }
    if (value < spec.min || value > spec.max) {
        return `${spec.name} must be between ${spec.min} and ${spec.max}.`;
    }
    return null;
}

export function formErrors(form) {
    const errors = controls
        .map((spec) => ({ ids: [spec.id], text: controlError(spec, form[spec.stage][spec.key]) }))
        .filter(({ text }) => text !== null);
    const { filters, dropout } = form;
    if (isNumber(filters.highpass) && isNumber(filters.lowpass) && filters.highpass >= filters.lowpass) {
        errors.push({ ids: ['filters.highpass', 'filters.lowpass'], text: 'Low cut must be below high cut.' });
    }
    if (isNumber(dropout.min_ms) && isNumber(dropout.max_ms) && dropout.min_ms > dropout.max_ms) {
        errors.push({ ids: ['dropout.min_ms', 'dropout.max_ms'], text: 'Shortest must not be longer than longest.' });
    }
    return errors;
}

const clone = (value) => JSON.parse(JSON.stringify(value));

const layerStages = { noise: 'level', filters: 'frequency' };

const neutral = (id) => stages.find((stage) => stage.id === id).off;

export function toForm(name, preset) {
    const form = { name, ...clone(preset) };
    for (const [id, value] of Object.entries(layerStages)) {
        form[id] = { ...neutral(id), ...Object.fromEntries(preset[id].map((layer) => [layer.kind, layer[value]])) };
    }
    form.bitcrush = { bits: preset.bitcrush?.bits ?? null };
    return form;
}

export function toBody(form) {
    const { name: _, ...body } = clone(form);
    for (const [id, value] of Object.entries(layerStages)) {
        body[id] = Object.entries(form[id])
            .filter(([kind, amount]) => amount !== neutral(id)[kind])
            .map(([kind, amount]) => ({ kind, [value]: amount }));
    }
    body.bitcrush = form.bitcrush.bits === null ? null : { bits: form.bitcrush.bits };
    return body;
}

export function blankForm() {
    return {
        name: '',
        noise: { white: 0, pink: 0, brown: 0 },
        filters: { highpass: 20, lowpass: 20000 },
        tremolo: { depth: 0, rate: 0, jitter: 0 },
        clip: { pregain: 1, threshold: 1 },
        bitcrush: { bits: null },
        dropout: { probability: 0, min_ms: 0, max_ms: 0 },
    };
}
