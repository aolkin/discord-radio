import { emptyMessage } from './api.js';
import {
    blankRow,
    blankSegmentDefaults,
    convertObject,
    entryPlaybackFields,
    fieldFromServer,
    fieldToServer,
    historyPenaltyMultiplier,
    noisePeriodFields,
    optionalSeconds,
    segmentDefaultsBlocks,
    segmentDefaultsFromServer,
    segmentDefaultsToServer,
    toggleHexConfig,
    weight,
} from './form-fields.js';
import { registryEditor } from './registry-editor.js';

export const modals = {
    poolEditor: {
        flag: 'showPoolEditor',
        perGuild: false,
        reset: () => ({ poolEditorMessage: emptyMessage() }),
    },
};

const kinds = {
    track: {
        label: 'Track',
        identifier: 'filename',
        entryFields: [
            { key: 'filename', label: 'Filename', type: 'text' },
            weight,
            optionalSeconds('max_duration_seconds', 'Max Duration (s)'),
            { key: 'allow_subsection', label: 'Allow Subsection', type: 'bool', optional: true },
            { key: 'volume', label: 'Volume (0-2)', type: 'number', step: 'any', min: 0, max: 2, optional: true },
            ...entryPlaybackFields,
        ],
    },
    hex: {
        label: 'Hex Message',
        identifier: 'text',
        entryFields: [
            { key: 'text', label: 'Text', type: 'text' },
            weight,
            { key: 'loop_min', label: 'Loop Min', type: 'number', step: 1, min: 0, optional: true },
            { key: 'loop_max', label: 'Loop Max', type: 'number', step: 1, min: 0, optional: true },
            ...entryPlaybackFields,
        ],
    },
    noise: {
        label: 'Noise',
        identifier: 'min_duration_seconds',
        entryFields: [...noisePeriodFields, ...entryPlaybackFields],
    },
};

const poolFields = [historyPenaltyMultiplier];

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
        segment_defaults: blankSegmentDefaults(),
        entries: [],
    };
}

const poolEditor = registryEditor({
    path: '/api/pools',
    prefix: 'pool',
    list: 'pools',
    label: 'pool',
    toForm,
    toBody,
    blankForm,
});

export function pools() {
    return {
        ...poolEditor.state(),
        ...poolEditor.methods,
        poolNewEntryKind: 'track',
        poolKinds: kinds,

        poolBlocks() {
            const pool = this.poolForm;
            return [
                { row: pool, fields: poolFields },
                ...segmentDefaultsBlocks(pool.segment_defaults),
                ...pool.entries.map((entry, index) => ({
                    row: entry,
                    fields: kinds[entry.kind].entryFields,
                    label: `${kinds[entry.kind].label} Entry`,
                    index,
                })),
            ];
        },

        togglePoolHexConfig() {
            toggleHexConfig(this.poolForm.segment_defaults);
        },

        addPoolEntry() {
            const kind = this.poolNewEntryKind;
            this.poolForm.entries.push({ kind, ...blankRow(kinds[kind].entryFields) });
        },
    };
}
