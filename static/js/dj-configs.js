import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const modals = {
    djConfigEditor: {
        flag: 'showDjConfigEditor',
        perGuild: false,
        reset: () => ({ djConfigEditorMessage: emptyMessage() }),
    },
};

const weight = { key: 'weight', label: 'Weight', type: 'number', step: 1, min: 0 };
const seconds = (key, label) => ({ key, label, type: 'number', step: 'any', min: 0 });
const optionalSeconds = (key, label) => ({ ...seconds(key, label), optional: true });
const profileText = (key, label, optional = false) => ({ key, label, type: 'text', optional, profile: true });
const optionalText = (key, label) => ({ key, label, type: 'text', optional: true });

const generalFields = [optionalText('channel_status', 'Channel Status')];

const loops = (key, label) => ({ key, label, type: 'number', step: 1, min: 0, default: 1 });

const kinds = {
    track: {
        label: 'Track',
        identifier: 'filename',
        entryFields: [
            { key: 'filename', label: 'Filename', type: 'text' },
            weight,
            optionalSeconds('max_duration_seconds', 'Max Duration (s)'),
            { key: 'allow_subsection', label: 'Allow Subsection', type: 'bool', optional: true },
            profileText('signal_profile', 'Signal Profile', true),
            { key: 'volume', label: 'Volume (0-2)', type: 'number', step: 'any', min: 0, max: 2, optional: true },
            optionalText('channel_status', 'Channel Status'),
        ],
    },
    hex: {
        label: 'Hex Message',
        identifier: 'text',
        entryFields: [
            { key: 'text', label: 'Text', type: 'text' },
            weight,
            profileText('signal_profile', 'Signal Profile', true),
            { key: 'loop_min', label: 'Loop Min', type: 'number', step: 1, min: 0, optional: true },
            { key: 'loop_max', label: 'Loop Max', type: 'number', step: 1, min: 0, optional: true },
            optionalText('announcement', 'Announcement'),
        ],
    },
    noise: {
        label: 'Noise',
        identifier: 'noise_profile',
        entryFields: [
            profileText('noise_profile', 'Noise Profile'),
            seconds('min_duration_seconds', 'Min Duration (s)'),
            seconds('max_duration_seconds', 'Max Duration (s)'),
            weight,
        ],
    },
};

const historyPenaltyMultiplier = {
    key: 'history_penalty_multiplier',
    label: 'History penalty multiplier',
    type: 'number',
    step: 'any',
    min: 0,
    max: 1,
    optional: true,
};

const poolFields = [{ key: 'name', label: 'Name', type: 'text' }, weight, historyPenaltyMultiplier];

const signalProfilesFields = [historyPenaltyMultiplier];

const hexDefaultsFields = [
    loops('loop_min', 'Loop Min'),
    loops('loop_max', 'Loop Max'),
    profileText('signal_profile', 'Signal Profile', true),
    { key: 'announcements', label: 'Announcements', type: 'strings' },
];

const signalProfileFields = [
    profileText('profile_name', 'Profile'),
    weight,
    seconds('fade_duration_seconds', 'Fade Duration (s)'),
    seconds('min_time_seconds', 'Min Time (s)'),
    seconds('max_time_seconds', 'Max Time (s)'),
];

function fieldFromServer(field, value) {
    if (field.type === 'strings') {
        return [...(value ?? [])];
    }
    if (value === null || value === undefined) {
        return '';
    }
    return field.type === 'bool' ? String(value) : value;
}

function fieldToServer(field, value) {
    if (field.type === 'strings') {
        return value;
    }
    if (value === '') {
        return null;
    }
    return field.type === 'bool' ? value === 'true' : value;
}

function convertObject(fields, source, convert) {
    return Object.fromEntries(fields.map((field) => [field.key, convert(field, source[field.key])]));
}

function poolFromServer(pool) {
    return {
        ...convertObject(poolFields, pool, fieldFromServer),
        hex_defaults: pool.hex_defaults && convertObject(hexDefaultsFields, pool.hex_defaults, fieldFromServer),
        entries: pool.entries.map((entry) => {
            const kind = Object.keys(kinds).find((name) => kinds[name].identifier in entry);
            return { kind, ...convertObject(kinds[kind].entryFields, entry, fieldFromServer) };
        }),
    };
}

function poolToServer(pool) {
    return {
        ...convertObject(poolFields, pool, fieldToServer),
        hex_defaults: pool.hex_defaults && convertObject(hexDefaultsFields, pool.hex_defaults, fieldToServer),
        entries: pool.entries.map((entry) => convertObject(kinds[entry.kind].entryFields, entry, fieldToServer)),
    };
}

function toForm(config) {
    return {
        name: config.name,
        ...convertObject(generalFields, config, fieldFromServer),
        pools: config.pools.map(poolFromServer),
        signal_profiles: {
            ...convertObject(signalProfilesFields, config.signal_profiles, fieldFromServer),
            entries: config.signal_profiles.entries.map((row) =>
                convertObject(signalProfileFields, row, fieldFromServer),
            ),
        },
    };
}

function toBody(form) {
    return {
        ...convertObject(generalFields, form, fieldToServer),
        pools: form.pools.map(poolToServer),
        signal_profiles: {
            ...convertObject(signalProfilesFields, form.signal_profiles, fieldToServer),
            entries: form.signal_profiles.entries.map((row) => convertObject(signalProfileFields, row, fieldToServer)),
        },
    };
}

function blankValue(field) {
    if (field.type === 'strings') {
        return [];
    }
    return field.type === 'number' && !field.optional ? (field.default ?? 0) : '';
}

function blankRow(fields) {
    return Object.fromEntries(fields.map((field) => [field.key, blankValue(field)]));
}

function blankForm() {
    return {
        name: '',
        channel_status: '',
        pools: [],
        signal_profiles: { ...blankRow(signalProfilesFields), entries: [] },
    };
}

export function djConfigs() {
    return {
        djConfigs: [],
        showDjConfigEditor: false,
        djConfigEditorIsNew: false,
        djConfigEditorLoading: false,
        djConfigEditorSaving: false,
        djConfigEditorMessage: { text: '', type: '' },
        djConfigForm: blankForm(),
        djConfigNewEntryKind: 'track',
        djConfigGeneralFields: generalFields,
        djConfigKinds: kinds,
        djConfigSignalProfilesFields: signalProfilesFields,
        djConfigSignalProfileFields: signalProfileFields,

        async loadDjConfigs() {
            try {
                const response = await apiRequest('/api/dj-configs');
                const data = await response.json();
                this.djConfigs = data.configs.sort();
            } catch (error) {
                console.error('Failed to load DJ configs:', error);
            }
        },

        addDjConfigPool() {
            const names = this.djConfigForm.pools.map((pool) => pool.name);
            let name = 'pool';
            for (let n = 2; names.includes(name); n++) {
                name = `pool-${n}`;
            }
            this.djConfigForm.pools.push({ ...blankRow(poolFields), name, hex_defaults: null, entries: [] });
        },

        djConfigPoolBlocks(pool) {
            return [
                { row: pool, fields: poolFields },
                ...(pool.hex_defaults
                    ? [{ row: pool.hex_defaults, fields: hexDefaultsFields, label: 'Hex Defaults' }]
                    : []),
                ...pool.entries.map((entry, index) => ({
                    row: entry,
                    fields: kinds[entry.kind].entryFields,
                    label: `${kinds[entry.kind].label} Entry`,
                    index,
                })),
            ];
        },

        toggleDjConfigHexDefaults(pool) {
            pool.hex_defaults = pool.hex_defaults ? null : blankRow(hexDefaultsFields);
        },

        addDjConfigEntry(pool, kind) {
            pool.entries.push({ kind, ...blankRow(kinds[kind].entryFields) });
        },

        addDjConfigSignalProfile() {
            this.djConfigForm.signal_profiles.entries.push(blankRow(signalProfileFields));
        },

        async openDjConfigEditor(name) {
            this.djConfigEditorIsNew = name === null;
            this.openModal('djConfigEditor');

            if (this.djConfigEditorIsNew) {
                this.djConfigForm = blankForm();
                return;
            }

            this.djConfigEditorLoading = true;
            try {
                const response = await apiRequest(`/api/dj-configs/${encodeURIComponent(name)}`, {
                    errorMessage: bodyOrStatus,
                });
                this.djConfigForm = toForm(await response.json());
            } catch (error) {
                console.error('Failed to load DJ config:', error);
                this.djConfigEditorMessage = { text: `Failed to load DJ config: ${error.message}`, type: 'error' };
            } finally {
                this.djConfigEditorLoading = false;
            }
        },

        async saveDjConfig() {
            const name = this.djConfigForm.name;
            if (this.djConfigEditorIsNew && this.djConfigs.includes(name)) {
                this.djConfigEditorMessage = { text: `A DJ config named "${name}" already exists`, type: 'error' };
                return;
            }

            const poolNames = this.djConfigForm.pools.map((pool) => pool.name);
            if (new Set(poolNames).size !== poolNames.length) {
                this.djConfigEditorMessage = { text: 'Pool names must be unique', type: 'error' };
                return;
            }

            this.djConfigEditorSaving = true;
            this.djConfigEditorMessage = emptyMessage();
            try {
                await apiRequest(`/api/dj-configs/${encodeURIComponent(name)}`, {
                    method: 'PUT',
                    body: toBody(this.djConfigForm),
                    errorMessage: bodyOrStatus,
                });
                this.djConfigEditorIsNew = false;
                this.djConfigEditorMessage = { text: 'DJ config saved', type: 'success' };
                await this.loadDjConfigs();
            } catch (error) {
                console.error('Failed to save DJ config:', error);
                this.djConfigEditorMessage = { text: `Failed to save DJ config: ${error.message}`, type: 'error' };
            } finally {
                this.djConfigEditorSaving = false;
            }
        },
    };
}
