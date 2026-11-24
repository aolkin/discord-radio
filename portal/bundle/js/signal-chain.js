export const clamp = (value, low, high) => Math.min(Math.max(value, low), high);
const clamp01 = (value) => clamp(value, 0, 1);
export const isNumber = (value) => typeof value === 'number' && !Number.isNaN(value);
export const roundTo = (value, decimals) => Number(value.toFixed(decimals));
export const clone = (value) => JSON.parse(JSON.stringify(value));

export const maxEffects = 16;

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

const control = (key, name, options) => ({ key, name, ...options });

const frequency = (color, meaning) =>
    control('frequency', 'Frequency', {
        min: 20,
        max: 20000,
        scale: 'log',
        low: 20,
        high: 20000,
        decimals: 0,
        unit: ' Hz',
        color,
        meaning,
    });

const dropoutLength = (key, name, color) =>
    control(key, name, {
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

export const noiseColors = ['white', 'pink', 'brown'];

export const kinds = [
    {
        kind: 'noise',
        name: 'Noise',
        description: 'Adds white, pink or brown noise under the music.',
        faders: true,
        defaults: { color: 'pink', level: 0.4 },
        controls: [
            control('level', 'Level', {
                min: 0,
                max: 2,
                scale: 'linear',
                low: 0,
                high: 2,
                decimals: 3,
                unit: '',
                color: '--chain-rose',
                meaning: (value) => dbText(levelDb(value)),
            }),
        ],
        readout: (params) => `${params.color} · ${dbText(levelDb(params.level))}`,
    },
    {
        kind: 'highpass',
        name: 'Low cut',
        description: 'Removes the frequencies below the cutoff.',
        defaults: { frequency: 500 },
        controls: [
            frequency('--chain-teal', (value) => (value <= 20 ? 'keeps all bass' : `cuts below ${hertz(value)}`)),
        ],
        readout: (params) => `below ${hertz(params.frequency)}`,
    },
    {
        kind: 'lowpass',
        name: 'High cut',
        description: 'Removes the frequencies above the cutoff.',
        defaults: { frequency: 5000 },
        controls: [
            frequency('--chain-rose', (value) => (value >= 20000 ? 'keeps all treble' : `cuts above ${hertz(value)}`)),
        ],
        readout: (params) => `above ${hertz(params.frequency)}`,
    },
    {
        kind: 'tremolo',
        name: 'Fading',
        description: 'Makes the volume dip and swell like a drifting signal.',
        defaults: { depth: 0.3, rate: 2, jitter: 0.3 },
        controls: [
            control('depth', 'Depth', {
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
            control('rate', 'Speed', {
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
            control('jitter', 'Unevenness', {
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
        ],
        readout: (params) =>
            `dips to ${Math.round((1 - params.depth) * 100)}% · ${params.rate > 0 ? `every ${every(params.rate)}` : 'frozen'}`,
    },
    {
        kind: 'clip',
        name: 'Clipping',
        description: 'Drive boosts the music, and the clip level squashes anything louder into distortion.',
        defaults: { pregain: 1, threshold: 0.8 },
        controls: [
            control('pregain', 'Drive', {
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
            control('threshold', 'Clip level', {
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
        ],
        readout: (params) => `drive ×${params.pregain} · clip ${params.threshold}`,
    },
    {
        kind: 'bitcrush',
        name: 'Bit depth',
        description: 'Rounds every sample to a coarse step for a grainy digital sound.',
        defaults: { bits: 8 },
        controls: [
            control('bits', 'Bits', {
                min: 1,
                max: 16,
                scale: 'linear',
                low: 1,
                high: 16,
                decimals: 0,
                unit: '',
                integer: true,
                color: '--chain-violet',
                meaning: (value) => `steps of 1/${2 ** (value - 1)}`,
            }),
        ],
        readout: (params) => `${params.bits} bits`,
    },
    {
        kind: 'dropout',
        name: 'Dropouts',
        description: 'Cuts the sound to silence for short moments.',
        defaults: { probability: 0.1, min_ms: 25, max_ms: 100 },
        controls: [
            control('probability', 'Rate', {
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
        ],
        readout: (params) =>
            params.probability > 0
                ? `one every ${every(params.probability)} · ${params.min_ms}–${params.max_ms} ms`
                : 'no dropouts',
    },
];

export const kindOf = (effect) => kinds.find((entry) => entry.kind === effect.kind);

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
    return isNumber(value) ? `${roundTo(value, spec.decimals)}${spec.unit}` : '—';
}

export function limitText(spec) {
    return `params.${spec.key} · server limit ${spec.min}–${spec.max}`;
}

export function keepOrdered(params, spec, value) {
    if (spec.key === 'min_ms' && isNumber(params.max_ms)) {
        return Math.min(value, params.max_ms);
    }
    if (spec.key === 'max_ms' && isNumber(params.min_ms)) {
        return Math.max(value, params.min_ms);
    }
    return value;
}

export const slot = (index, key) => `${index}.${key}`;

function controlError(spec, value) {
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

function filterErrors(effects) {
    const lowpasses = effects
        .map((effect, index) => ({ effect, index }))
        .filter(({ effect }) => effect.kind === 'lowpass' && isNumber(effect.params.frequency));
    if (lowpasses.length === 0) {
        return [];
    }
    const lowest = lowpasses.reduce((low, entry) =>
        entry.effect.params.frequency < low.effect.params.frequency ? entry : low,
    );
    return effects
        .map((effect, index) => ({ effect, index }))
        .filter(
            ({ effect }) =>
                effect.kind === 'highpass' &&
                isNumber(effect.params.frequency) &&
                effect.params.frequency >= lowest.effect.params.frequency,
        )
        .map(({ effect, index }) => ({
            ids: [slot(index, 'frequency'), slot(lowest.index, 'frequency')],
            text: `${effect.name}: must cut below ${lowest.effect.name}.`,
        }));
}

export function formErrors(form) {
    const errors = form.effects.flatMap((effect, index) => {
        const { params } = effect;
        const found = kindOf(effect)
            .controls.map((spec) => ({ ids: [slot(index, spec.key)], text: controlError(spec, params[spec.key]) }))
            .filter(({ text }) => text !== null)
            .map((error) => ({ ...error, text: `${effect.name}: ${error.text}` }));
        if (effect.kind === 'dropout' && params.min_ms > params.max_ms) {
            found.push({
                ids: [slot(index, 'min_ms'), slot(index, 'max_ms')],
                text: `${effect.name}: Shortest must not be longer than Longest.`,
            });
        }
        return found;
    });
    return [...errors, ...filterErrors(form.effects)];
}

export function newEffect(kind, effects) {
    const taken = new Set(effects.map((effect) => effect.name));
    let name = kind;
    for (let count = 2; taken.has(name); count++) {
        name = `${kind}-${count}`;
    }
    return { name, kind, params: clone(kindOf({ kind }).defaults) };
}
