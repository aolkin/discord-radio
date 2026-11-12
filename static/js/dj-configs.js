import { apiRequest } from './api.js';
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

const poolWeight = { ...weight, min: 1 };

const poolReferenceFields = [{ key: 'pool', label: 'Pool', type: 'text' }, poolWeight];

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

function referenceBlocks(reference, index) {
    return segmentDefaultsBlocks(reference.segment_defaults, ['pools', index, 'segment_defaults']);
}

function blocks(form) {
    return [
        ...form.pools.flatMap((reference, index) => [
            { row: reference, fields: poolReferenceFields, path: ['pools', index] },
            ...referenceBlocks(reference, index),
        ]),
        ...rotations.flatMap(({ key, entryFields }) =>
            form[key].entries.map((row, index) => ({ row, fields: entryFields, path: [key, 'entries', index] })),
        ),
    ];
}

const djConfigEditor = registryEditor({
    path: '/api/dj-configs',
    prefix: 'djConfig',
    list: 'djConfigs',
    kind: 'dj-configs',
    label: 'DJ config',
    toForm,
    toBody,
    blankForm,
    blocks,
    afterSave() {
        return this.loadDjConfigPools();
    },
});

export function djConfigs() {
    return {
        ...djConfigEditor.state(),
        ...djConfigEditor.methods,
        djConfigRotations: rotations,
        djConfigWeightField: poolWeight,
        djConfigPools: {},

        async loadDjConfigPools() {
            await this.loadDjConfigs();
            const entries = await Promise.all(
                this.djConfigs.map(async (name) => {
                    try {
                        const response = await apiRequest(`/api/dj-configs/${encodeURIComponent(name)}`);
                        return [name, Object.keys((await response.json()).pools)];
                    } catch (error) {
                        console.error(`Failed to load DJ config ${name}:`, error);
                        return [name, []];
                    }
                }),
            );
            this.djConfigPools = Object.fromEntries(entries);
        },

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

        djConfigReferenceBlocks(reference, index) {
            return referenceBlocks(reference, index);
        },

        toggleDjConfigReferenceHexConfig(reference) {
            toggleHexConfig(reference.segment_defaults);
        },

        addDjConfigRotationEntry(rotation) {
            this.djConfigForm[rotation.key].entries.push(blankRow(rotation.entryFields));
        },
    };
}
