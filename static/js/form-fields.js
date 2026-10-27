export const weight = { key: 'weight', label: 'Weight', type: 'number', step: 1, min: 0 };
export const seconds = (key, label) => ({ key, label, type: 'number', step: 'any', min: 0 });
export const optionalSeconds = (key, label) => ({ ...seconds(key, label), optional: true });
export const profileText = (key, label, optional = false) => ({ key, label, type: 'text', optional, profile: true });
export const optionalText = (key, label) => ({ key, label, type: 'text', optional: true });

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
    return field.type === 'number' && !field.optional ? (field.default ?? 0) : '';
}

export function blankRow(fields) {
    return Object.fromEntries(fields.map((field) => [field.key, blankValue(field)]));
}
