import { emptyMessage } from './api.js';
import {
    blankRow,
    blankSegmentDefaults,
    convertObject,
    fieldFromServer,
    fieldToServer,
    seconds,
    segmentDefaultsBlocks,
    segmentDefaultsFromServer,
    segmentDefaultsToServer,
    signalProfileEntryFields,
    toggleHexConfig,
    weight,
} from './form-fields.js';
import { registryEditor } from './registry-editor.js';

export const modals = {
    djConfigEditor: {
        flag: 'showDjConfigEditor',
        perGuild: false,
        reset: () => ({ djConfigEditorMessage: emptyMessage() }),
    },
};

const poolReferenceFields = [
    { key: 'pool', label: 'Pool', type: 'text' },
    { ...weight, min: 1 },
];

const rotations = [
    {
        key: 'signal_profiles',
        label: 'Signal Profile Rotation',
        addLabel: '+ Add Signal Profile',
        entryFields: signalProfileEntryFields,
    },
    {
        key: 'channel_statuses',
        label: 'Channel Status Rotation',
        addLabel: '+ Add Channel Status',
        entryFields: [
            { key: 'status', label: 'Status', type: 'text' },
            weight,
            seconds('min_duration_seconds', 'Min Duration (s)'),
            seconds('max_duration_seconds', 'Max Duration (s)'),
        ],
    },
];

function convertRotations(source, convert) {
    return Object.fromEntries(
        rotations.map(({ key, entryFields }) => [
            key,
            {
                entries: source[key].entries.map((row) => convertObject(entryFields, row, convert)),
            },
        ]),
    );
}

function toForm(name, config) {
    return {
        name,
        pools: Object.entries(config.pools).map(([pool, reference]) => ({
            ...convertObject(poolReferenceFields, { pool, ...reference }, fieldFromServer),
            segment_defaults: segmentDefaultsFromServer(reference.segment_defaults),
        })),
        ...convertRotations(config, fieldFromServer),
    };
}

function toBody(form) {
    return {
        pools: Object.fromEntries(
            form.pools.map((reference) => {
                const { pool, ...settings } = convertObject(poolReferenceFields, reference, fieldToServer);
                return [pool, { ...settings, segment_defaults: segmentDefaultsToServer(reference.segment_defaults) }];
            }),
        ),
        ...convertRotations(form, fieldToServer),
    };
}

function blankForm() {
    return {
        name: '',
        pools: [],
        ...Object.fromEntries(rotations.map(({ key }) => [key, { entries: [] }])),
    };
}

const djConfigEditor = registryEditor({
    path: '/api/dj-configs',
    prefix: 'djConfig',
    list: 'djConfigs',
    label: 'DJ config',
    toForm,
    toBody,
    blankForm,
});

export function djConfigs() {
    return {
        ...djConfigEditor.state(),
        ...djConfigEditor.methods,
        djConfigRotations: rotations,

        djConfigPoolOptions(reference) {
            return this.pools.includes(reference.pool) || reference.pool === ''
                ? this.pools
                : [...this.pools, reference.pool];
        },

        addDjConfigPool() {
            this.djConfigForm.pools.push({
                ...blankRow(poolReferenceFields),
                pool: this.pools[0] ?? '',
                segment_defaults: blankSegmentDefaults(),
            });
        },

        djConfigReferenceBlocks(reference) {
            return segmentDefaultsBlocks(reference.segment_defaults);
        },

        toggleDjConfigReferenceHexConfig(reference) {
            toggleHexConfig(reference.segment_defaults);
        },

        addDjConfigRotationEntry(rotation) {
            this.djConfigForm[rotation.key].entries.push(blankRow(rotation.entryFields));
        },
    };
}
