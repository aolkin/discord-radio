export const weight = { key: 'weight', label: 'Weight', type: 'number', step: 1, min: 0, default: 1 };
const seconds = (key, label, value) => ({ key, label, type: 'number', step: 'any', min: 0, default: value });
export const optionalSeconds = (key, label) => ({ ...seconds(key, label), optional: true });
export const presetText = (key, label, optional = false) => ({ key, label, type: 'text', optional, preset: true });
export const optionalText = (key, label) => ({ key, label, type: 'text', optional: true });

export const minDuration = seconds('min_duration_seconds', 'Min Duration (s)', 60);
export const maxDuration = seconds('max_duration_seconds', 'Max Duration (s)', 120);

export const noisePeriodFields = [
    minDuration,
    maxDuration,
    weight,
    optionalSeconds('fade_duration_seconds', 'Fade Duration (s)'),
];

export const signalPresetEntryFields = [presetText('signal_preset', 'Signal Preset'), ...noisePeriodFields];

export const entryPlaybackFields = [
    presetText('signal_preset', 'Signal Preset', true),
    optionalText('channel_status', 'Channel Status'),
    optionalText('announcement', 'Announcement'),
];

export function fieldFromServer(field, value) {
    if (field.type === 'strings') {
        return [...(value ?? [])];
    }
    if (value === null || value === undefined) {
        return '';
    }
    return field.type === 'bool' ? String(value) : value;
}

export function fieldToServer(field, value) {
    if (field.type === 'strings') {
        return value;
    }
    if (value === '') {
        return null;
    }
    return field.type === 'bool' ? value === 'true' : value;
}

export function fieldInvalid(field, value) {
    if (field.type === 'strings') {
        return value.some((item) => item === '');
    }
    if (value === '') {
        return !field.optional;
    }
    if (field.type !== 'number') {
        return false;
    }
    return (
        typeof value !== 'number' ||
        Number.isNaN(value) ||
        (field.step === 1 && !Number.isInteger(value)) ||
        (field.min !== undefined && value < field.min) ||
        (field.max !== undefined && value > field.max)
    );
}

export function convertObject(fields, source, convert) {
    return Object.fromEntries(fields.map((field) => [field.key, convert(field, source[field.key])]));
}

function blankValue(field) {
    if (field.type === 'strings') {
        return [];
    }
    if (field.optional) {
        return '';
    }
    return field.default ?? (field.type === 'number' ? 0 : '');
}

export function blankRow(fields) {
    return Object.fromEntries(fields.map((field) => [field.key, blankValue(field)]));
}

const loops = (key, label) => ({ key, label, type: 'number', step: 1, min: 0, default: 1 });

const segmentDefaultsFields = [
    presetText('signal_preset', 'Signal Preset', true),
    optionalText('channel_status', 'Channel Status'),
    { key: 'announcements', label: 'Announcements', type: 'strings' },
];

const hexConfigFields = [
    loops('loop_min', 'Loop Min'),
    loops('loop_max', 'Loop Max'),
    optionalText('hex_audio_folder', 'Hex Audio Folder'),
];

export function segmentDefaultsFromServer(segmentDefaults) {
    const source = segmentDefaults ?? {};
    return {
        ...convertObject(segmentDefaultsFields, source, fieldFromServer),
        hex_config: source.hex_config ? convertObject(hexConfigFields, source.hex_config, fieldFromServer) : null,
    };
}

export function segmentDefaultsToServer(form) {
    return {
        ...convertObject(segmentDefaultsFields, form, fieldToServer),
        hex_config: form.hex_config && convertObject(hexConfigFields, form.hex_config, fieldToServer),
    };
}

export function blankSegmentDefaults() {
    return { ...blankRow(segmentDefaultsFields), hex_config: null };
}

export function segmentDefaultsBlocks(segmentDefaults, path) {
    return [
        { row: segmentDefaults, fields: segmentDefaultsFields, label: 'Segment Defaults', path },
        ...(segmentDefaults.hex_config
            ? [
                  {
                      row: segmentDefaults.hex_config,
                      fields: hexConfigFields,
                      label: 'Hex Config',
                      path: [...path, 'hex_config'],
                  },
              ]
            : []),
    ];
}

export function toggleHexConfig(segmentDefaults) {
    segmentDefaults.hex_config = segmentDefaults.hex_config ? null : blankRow(hexConfigFields);
}

export const sliderSteps = 1000;

const decimalsOf = (step) => (String(step).split('.')[1] ?? '').length;
const clamp01 = (t) => Math.min(Math.max(t, 0), 1);
const allowsZero = (field) => field.min <= 0;

export function sliderPosition(field, value) {
    const { scale, min, max } = field.slider;
    if (typeof value !== 'number' || Number.isNaN(value) || (scale === 'log' && value <= 0)) {
        return 0;
    }
    if (scale === 'linear') {
        return Math.round(clamp01((value - min) / (max - min)) * sliderSteps);
    }
    const t = clamp01(Math.log(value / min) / Math.log(max / min));
    return allowsZero(field) ? 1 + Math.round(t * (sliderSteps - 1)) : Math.round(t * sliderSteps);
}

export function sliderValue(field, position) {
    const { scale, min, max, step } = field.slider;
    if (scale === 'log' && allowsZero(field) && position === 0) {
        return 0;
    }
    let t = position / sliderSteps;
    if (scale === 'log' && allowsZero(field)) {
        t = (position - 1) / (sliderSteps - 1);
    }
    const value = scale === 'linear' ? min + t * (max - min) : min * (max / min) ** t;
    return Number(value.toFixed(decimalsOf(step)));
}
