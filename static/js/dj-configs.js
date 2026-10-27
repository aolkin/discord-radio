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

const historyPenaltyMultiplier = {
    key: 'history_penalty_multiplier',
    label: 'History penalty multiplier',
    type: 'number',
    step: 'any',
    min: 0,
    max: 1,
    optional: true,
};

const playbackFields = (signalProfileLabel) => [
    historyPenaltyMultiplier,
    profileText('signal_profile', signalProfileLabel, true),
    optionalText('channel_status', 'Channel Status'),
];

const configPlaybackFields = playbackFields('Rotation Fallback Signal Profile');

const poolPlaybackFields = playbackFields('Segment Signal Profile');

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
            optionalSeconds('fade_duration_seconds', 'Fade Duration (s)'),
        ],
    },
};

const poolFields = [{ key: 'name', label: 'Name', type: 'text' }, weight];

const signalProfilesFields = [historyPenaltyMultiplier];

const hexDefaultsFields = [
    loops('loop_min', 'Loop Min'),
    loops('loop_max', 'Loop Max'),
    { key: 'announcements', label: 'Announcements', type: 'strings' },
];

const signalProfileFields = [
    profileText('noise_profile', 'Noise Profile'),
    weight,
    seconds('fade_duration_seconds', 'Fade Duration (s)'),
    seconds('min_duration_seconds', 'Min Duration (s)'),
    seconds('max_duration_seconds', 'Max Duration (s)'),
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

function playbackFromServer(fields, playback) {
    const source = playback ?? {};
    return {
        ...convertObject(fields, source, fieldFromServer),
        hex_defaults: source.hex_defaults
            ? convertObject(hexDefaultsFields, source.hex_defaults, fieldFromServer)
            : null,
    };
}

function playbackToServer(fields, form) {
    const playback = {
        ...convertObject(fields, form, fieldToServer),
        hex_defaults: form.hex_defaults && convertObject(hexDefaultsFields, form.hex_defaults, fieldToServer),
    };
    return Object.values(playback).some((value) => value !== null) ? playback : null;
}

function blankPlayback(fields) {
    return { ...blankRow(fields), hex_defaults: null };
}

function playbackBlocks(fields, playback, label) {
    return [
        { row: playback, fields, label },
        ...(playback.hex_defaults
            ? [{ row: playback.hex_defaults, fields: hexDefaultsFields, label: 'Hex Defaults' }]
            : []),
    ];
}

function poolFromServer(pool) {
    return {
        ...convertObject(poolFields, pool, fieldFromServer),
        playback_config: playbackFromServer(poolPlaybackFields, pool.playback_config),
        entries: pool.entries.map((entry) => {
            const kind = Object.keys(kinds).find((name) => kinds[name].identifier in entry);
            return { kind, ...convertObject(kinds[kind].entryFields, entry, fieldFromServer) };
        }),
    };
}

function poolToServer(pool) {
    return {
        ...convertObject(poolFields, pool, fieldToServer),
        playback_config: playbackToServer(poolPlaybackFields, pool.playback_config),
        entries: pool.entries.map((entry) => convertObject(kinds[entry.kind].entryFields, entry, fieldToServer)),
    };
}

function toForm(config) {
    const signalProfiles = config.signal_profiles ?? { entries: [] };
    return {
        name: config.name,
        playback_config: playbackFromServer(configPlaybackFields, config.playback_config),
        pools: config.pools.map(poolFromServer),
        signal_profiles: {
            ...convertObject(signalProfilesFields, signalProfiles, fieldFromServer),
            entries: signalProfiles.entries.map((row) => convertObject(signalProfileFields, row, fieldFromServer)),
        },
    };
}

function toBody(form) {
    return {
        playback_config: playbackToServer(configPlaybackFields, form.playback_config),
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
        playback_config: blankPlayback(configPlaybackFields),
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
            this.djConfigForm.pools.push({
                ...blankRow(poolFields),
                name,
                playback_config: blankPlayback(poolPlaybackFields),
                entries: [],
            });
        },

        djConfigGeneralBlocks() {
            return playbackBlocks(configPlaybackFields, this.djConfigForm.playback_config);
        },

        djConfigPoolBlocks(pool) {
            return [
                { row: pool, fields: poolFields },
                ...playbackBlocks(poolPlaybackFields, pool.playback_config, 'Playback'),
                ...pool.entries.map((entry, index) => ({
                    row: entry,
                    fields: kinds[entry.kind].entryFields,
                    label: `${kinds[entry.kind].label} Entry`,
                    index,
                })),
            ];
        },

        toggleDjConfigHexDefaults(playback) {
            playback.hex_defaults = playback.hex_defaults ? null : blankRow(hexDefaultsFields);
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
