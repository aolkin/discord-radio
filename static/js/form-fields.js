export const weight = { key: 'weight', label: 'Weight', type: 'number', step: 1, min: 0 };
export const seconds = (key, label) => ({ key, label, type: 'number', step: 'any', min: 0 });
export const optionalSeconds = (key, label) => ({ ...seconds(key, label), optional: true });
export const profileText = (key, label, optional = false) => ({ key, label, type: 'text', optional, profile: true });
export const optionalText = (key, label) => ({ key, label, type: 'text', optional: true });

export const noisePeriodFields = [
    seconds('min_duration_seconds', 'Min Duration (s)'),
    seconds('max_duration_seconds', 'Max Duration (s)'),
    weight,
    optionalSeconds('fade_duration_seconds', 'Fade Duration (s)'),
];

export const signalProfileEntryFields = [profileText('signal_profile', 'Signal Profile'), ...noisePeriodFields];

export const entryPlaybackFields = [
    profileText('signal_profile', 'Signal Profile', true),
    optionalText('channel_status', 'Channel Status'),
    optionalText('announcement', 'Announcement'),
];

export const historyPenaltyMultiplier = {
    key: 'history_penalty_multiplier',
    label: 'History penalty multiplier',
    type: 'number',
    step: 'any',
    min: 0,
    max: 1,
    optional: true,
};

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
    profileText('signal_profile', 'Signal Profile', true),
    optionalText('channel_status', 'Channel Status'),
    { key: 'announcements', label: 'Announcements', type: 'strings' },
];

const hexConfigFields = [
    loops('loop_min', 'Loop Min'),
    loops('loop_max', 'Loop Max'),
    { key: 'hex_audio_prefix', label: 'Hex Audio Prefix', type: 'text', default: 'audio/hex/' },
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

export function segmentDefaultsBlocks(segmentDefaults) {
    return [
        { row: segmentDefaults, fields: segmentDefaultsFields, label: 'Segment Defaults' },
        ...(segmentDefaults.hex_config
            ? [{ row: segmentDefaults.hex_config, fields: hexConfigFields, label: 'Hex Config' }]
            : []),
    ];
}

export function toggleHexConfig(segmentDefaults) {
    segmentDefaults.hex_config = segmentDefaults.hex_config ? null : blankRow(hexConfigFields);
}
