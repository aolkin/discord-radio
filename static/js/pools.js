import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';
import {
    blankRow,
    convertObject,
    fieldFromServer,
    fieldToServer,
    historyPenaltyMultiplier,
    optionalSeconds,
    optionalText,
    profileText,
    seconds,
    weight,
} from './form-fields.js';

export const modals = {
    poolEditor: {
        flag: 'showPoolEditor',
        perGuild: false,
        reset: () => ({ poolEditorMessage: emptyMessage() }),
    },
};

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

const poolFields = [historyPenaltyMultiplier];

const segmentDefaultsFields = [
    profileText('signal_profile', 'Signal Profile', true),
    optionalText('channel_status', 'Channel Status'),
];

const hexDefaultsFields = [
    loops('loop_min', 'Loop Min'),
    loops('loop_max', 'Loop Max'),
    { key: 'announcements', label: 'Announcements', type: 'strings' },
];

function segmentDefaultsFromServer(segmentDefaults) {
    const source = segmentDefaults ?? {};
    return {
        ...convertObject(segmentDefaultsFields, source, fieldFromServer),
        hex_config: source.hex_config ? convertObject(hexDefaultsFields, source.hex_config, fieldFromServer) : null,
    };
}

function segmentDefaultsToServer(form) {
    return {
        ...convertObject(segmentDefaultsFields, form, fieldToServer),
        hex_config: form.hex_config && convertObject(hexDefaultsFields, form.hex_config, fieldToServer),
    };
}

function toForm(name, pool) {
    return {
        name,
        ...convertObject(poolFields, pool, fieldFromServer),
        segment_defaults: segmentDefaultsFromServer(pool.segment_defaults),
        entries: pool.entries.map((entry) => {
            const kind = Object.keys(kinds).find((kindName) => kinds[kindName].identifier in entry);
            return { kind, ...convertObject(kinds[kind].entryFields, entry, fieldFromServer) };
        }),
    };
}

function toBody(form) {
    return {
        ...convertObject(poolFields, form, fieldToServer),
        segment_defaults: segmentDefaultsToServer(form.segment_defaults),
        entries: form.entries.map((entry) => convertObject(kinds[entry.kind].entryFields, entry, fieldToServer)),
    };
}

function blankForm() {
    return {
        name: '',
        ...blankRow(poolFields),
        segment_defaults: { ...blankRow(segmentDefaultsFields), hex_config: null },
        entries: [],
    };
}

export function pools() {
    return {
        pools: [],
        showPoolEditor: false,
        poolEditorIsNew: false,
        poolEditorLoading: false,
        poolEditorSaving: false,
        poolEditorMessage: { text: '', type: '' },
        poolForm: blankForm(),
        poolNewEntryKind: 'track',
        poolKinds: kinds,

        async loadPools() {
            try {
                const response = await apiRequest('/api/pools');
                const data = await response.json();
                this.pools = data.pools.sort();
            } catch (error) {
                console.error('Failed to load pools:', error);
            }
        },

        poolBlocks() {
            const pool = this.poolForm;
            const segmentDefaults = pool.segment_defaults;
            return [
                { row: pool, fields: poolFields },
                { row: segmentDefaults, fields: segmentDefaultsFields, label: 'Segment Defaults' },
                ...(segmentDefaults.hex_config
                    ? [{ row: segmentDefaults.hex_config, fields: hexDefaultsFields, label: 'Hex Config' }]
                    : []),
                ...pool.entries.map((entry, index) => ({
                    row: entry,
                    fields: kinds[entry.kind].entryFields,
                    label: `${kinds[entry.kind].label} Entry`,
                    index,
                })),
            ];
        },

        togglePoolHexConfig() {
            const segmentDefaults = this.poolForm.segment_defaults;
            segmentDefaults.hex_config = segmentDefaults.hex_config ? null : blankRow(hexDefaultsFields);
        },

        addPoolEntry() {
            const kind = this.poolNewEntryKind;
            this.poolForm.entries.push({ kind, ...blankRow(kinds[kind].entryFields) });
        },

        async openPoolEditor(name) {
            this.poolEditorIsNew = name === null;
            this.openModal('poolEditor');

            if (this.poolEditorIsNew) {
                this.poolForm = blankForm();
                return;
            }

            this.poolEditorLoading = true;
            try {
                const response = await apiRequest(`/api/pools/${encodeURIComponent(name)}`, {
                    errorMessage: bodyOrStatus,
                });
                this.poolForm = toForm(name, await response.json());
            } catch (error) {
                console.error('Failed to load pool:', error);
                this.poolEditorMessage = { text: `Failed to load pool: ${error.message}`, type: 'error' };
            } finally {
                this.poolEditorLoading = false;
            }
        },

        async savePool() {
            const name = this.poolForm.name;
            if (this.poolEditorIsNew && this.pools.includes(name)) {
                this.poolEditorMessage = { text: `A pool named "${name}" already exists`, type: 'error' };
                return;
            }

            this.poolEditorSaving = true;
            this.poolEditorMessage = emptyMessage();
            try {
                await apiRequest(`/api/pools/${encodeURIComponent(name)}`, {
                    method: 'PUT',
                    body: toBody(this.poolForm),
                    errorMessage: bodyOrStatus,
                });
                this.poolEditorIsNew = false;
                this.poolEditorMessage = { text: 'Pool saved', type: 'success' };
                await this.loadPools();
            } catch (error) {
                console.error('Failed to save pool:', error);
                this.poolEditorMessage = { text: `Failed to save pool: ${error.message}`, type: 'error' };
            } finally {
                this.poolEditorSaving = false;
            }
        },
    };
}
