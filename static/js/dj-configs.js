import { emptyMessage } from './api.js';
import {
    blankRow,
    blankSegmentDefaults,
    convertObject,
    fieldFromServer,
    fieldToServer,
    historyPenaltyMultiplier,
    noiseEntryFields,
    seconds,
    segmentDefaultsBlocks,
    segmentDefaultsFromServer,
    segmentDefaultsToServer,
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

const generalFields = [
    {
        ...historyPenaltyMultiplier,
        key: 'pool_history_penalty_multiplier',
        label: 'Pool selection history penalty multiplier',
    },
];

const poolReferenceFields = [
    { key: 'pool', label: 'Pool', type: 'text' },
    { ...weight, min: 1 },
];

const rotationFields = [historyPenaltyMultiplier];

const rotations = [
    {
        key: 'signal_profiles',
        label: 'Signal Profile Rotation',
        addLabel: '+ Add Signal Profile',
        entryFields: noiseEntryFields,
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
                ...convertObject(rotationFields, source[key], convert),
                entries: source[key].entries.map((row) => convertObject(entryFields, row, convert)),
            },
        ]),
    );
}

function toForm(name, config) {
    return {
        name,
        ...convertObject(generalFields, config, fieldFromServer),
        pools: Object.entries(config.pools).map(([pool, reference]) => ({
            ...convertObject(poolReferenceFields, { pool, ...reference }, fieldFromServer),
            segment_defaults: segmentDefaultsFromServer(reference.segment_defaults),
        })),
        ...convertRotations(config, fieldFromServer),
    };
}

function toBody(form) {
    return {
        ...convertObject(generalFields, form, fieldToServer),
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
        ...blankRow(generalFields),
        pools: [],
        ...Object.fromEntries(rotations.map(({ key }) => [key, { ...blankRow(rotationFields), entries: [] }])),
    };
}

const djConfigEditor = registryEditor({
    path: '/api/dj-configs',
    prefix: 'djConfig',
    list: 'djConfigs',
    listKey: 'configs',
    label: 'DJ config',
    toForm,
    toBody,
    blankForm,
});

export function djConfigs() {
    return {
        ...djConfigEditor.state(),
        ...djConfigEditor.methods,
        djConfigGeneralFields: generalFields,
        djConfigRotationFields: rotationFields,
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
