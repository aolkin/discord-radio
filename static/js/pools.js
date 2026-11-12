import {
    blankRow,
    blankSegmentDefaults,
    convertObject,
    entryPlaybackFields,
    fieldFromServer,
    fieldToServer,
    noisePeriodFields,
    optionalSeconds,
    segmentDefaultsBlocks,
    segmentDefaultsFromServer,
    segmentDefaultsToServer,
    toggleHexConfig,
    weight,
} from './form-fields.js';
import { registryEditor } from './registry-editor.js';

const kinds = {
    track: {
        label: 'Track',
        identifier: 'filename',
        what: ['filename'],
        length: ['max_duration_seconds'],
        expansion: ['allow_subsection', 'volume', 'channel_status', 'announcement'],
        entryFields: [
            { key: 'filename', label: 'Filename', type: 'text' },
            weight,
            { ...optionalSeconds('max_duration_seconds', 'Max Duration (s)'), placeholder: 'Whole file' },
            { key: 'allow_subsection', label: 'Allow Subsection', type: 'bool', optional: true },
            { key: 'volume', label: 'Volume (0-2)', type: 'number', step: 'any', min: 0, max: 2, optional: true },
            ...entryPlaybackFields,
        ],
    },
    hex: {
        label: 'Hex Message',
        identifier: 'text',
        what: ['text'],
        length: ['loop_min', 'loop_max'],
        expansion: ['channel_status', 'announcement'],
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
        what: [],
        length: ['min_duration_seconds', 'max_duration_seconds'],
        expansion: ['fade_duration_seconds', 'channel_status', 'announcement'],
        entryFields: [...noisePeriodFields, ...entryPlaybackFields],
    },
};

function toForm(name, pool) {
    return {
        name,
        segment_defaults: segmentDefaultsFromServer(pool.segment_defaults),
        entries: pool.entries.map((entry) => {
            const kind = Object.keys(kinds).find((kindName) => kinds[kindName].identifier in entry);
            return { kind, ...convertObject(kinds[kind].entryFields, entry, fieldFromServer) };
        }),
    };
}

function toBody(form) {
    return {
        segment_defaults: segmentDefaultsToServer(form.segment_defaults),
        entries: form.entries.map((entry) => convertObject(kinds[entry.kind].entryFields, entry, fieldToServer)),
    };
}

function blankForm() {
    return {
        name: '',
        segment_defaults: blankSegmentDefaults(),
        entries: [],
    };
}

function blocks(form) {
    return [
        ...segmentDefaultsBlocks(form.segment_defaults, ['segment_defaults']),
        ...form.entries.map((entry, index) => ({
            row: entry,
            fields: kinds[entry.kind].entryFields,
            label: `${kinds[entry.kind].label} Entry`,
            path: ['entries', index],
            index,
        })),
    ];
}

function formErrors(form) {
    return form.entries.some((entry) => typeof entry.weight === 'number' && entry.weight > 0)
        ? []
        : ['A pool needs at least one entry with weight above 0'];
}

const poolEditor = registryEditor({
    path: '/api/pools',
    prefix: 'pool',
    list: 'pools',
    kind: 'pools',
    label: 'pool',
    toForm,
    toBody,
    blankForm,
    blocks,
    formErrors,
});

export function pools() {
    return {
        ...poolEditor.state(),
        ...poolEditor.methods,
        poolMode: 'properties',
        poolExpanded: {},
        poolKinds: kinds,
        poolWeightField: weight,

        poolUsers(pool) {
            return this.djConfigs.filter((config) => this.djConfigPools[config]?.includes(pool));
        },

        togglePoolHexConfig() {
            toggleHexConfig(this.poolForm.segment_defaults);
        },

        poolDefaultBlocks() {
            return segmentDefaultsBlocks(this.poolForm.segment_defaults, ['segment_defaults']);
        },

        poolColumn(entry, column) {
            const { entryFields } = kinds[entry.kind];
            return kinds[entry.kind][column].map((key) => entryFields.find((field) => field.key === key));
        },

        poolProfileOptions(value) {
            return value === '' || this.profiles.includes(value) ? this.profiles : [...this.profiles, value];
        },

        poolTotalWeight() {
            return this.poolForm.entries.reduce(
                (total, entry) => total + (typeof entry.weight === 'number' ? entry.weight : 0),
                0,
            );
        },

        poolShare(entry) {
            const total = this.poolTotalWeight();
            return total > 0 && typeof entry.weight === 'number'
                ? `${((entry.weight / total) * 100).toFixed(1)}%`
                : '0%';
        },

        poolEntryName(entry) {
            if (entry.kind === 'noise') {
                return `Noise ${entry.min_duration_seconds}-${entry.max_duration_seconds}s`;
            }
            return entry[kinds[entry.kind].identifier];
        },

        pickPoolFile(entry) {
            this.openFileBrowser({
                mode: 'file',
                prefix: 'tracks/',
                onSelect: (key) => {
                    entry.filename = `s3://${key}`;
                },
            });
        },

        addPoolEntry(kind) {
            this.poolForm.entries.push({ kind, ...blankRow(kinds[kind].entryFields) });
        },

        removePoolEntry(index) {
            this.poolForm.entries.splice(index, 1);
            this.poolExpanded = {};
        },
    };
}
