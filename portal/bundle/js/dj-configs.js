import { apiRequest } from './api.js';
import {
    blankRow,
    blankSegmentDefaults,
    convertObject,
    fieldFromServer,
    fieldToServer,
    maxDuration,
    minDuration,
    segmentDefaultsBlocks,
    segmentDefaultsFromServer,
    segmentDefaultsToServer,
    signalPresetEntryFields,
    toggleTtsConfig,
    weight,
} from './form-fields.js';
import { poolToForm } from './pools.js';
import { newItem, registryEditor } from './registry-editor.js';
import { shareOf } from './weights.js';

const poolWeight = { ...weight, min: 1 };

const poolModeFields = [
    { key: 'playback', label: 'Playback', type: 'select', options: ['single', 'all'], default: 'single' },
    { key: 'order', label: 'Order', type: 'select', options: ['weighted', 'sequential'], default: 'weighted' },
];

const poolReferenceFields = [{ key: 'pool', label: 'Pool', type: 'text' }, poolWeight, ...poolModeFields];

const rotations = [
    {
        key: 'signal_presets',
        label: 'Signal Preset Rotation',
        addLabel: '+ Add Signal Preset',
        entryFields: signalPresetEntryFields,
    },
    {
        key: 'channel_statuses',
        label: 'Channel Status Rotation',
        addLabel: '+ Add Channel Status',
        entryFields: [{ key: 'status', label: 'Status', type: 'text' }, weight, minDuration, maxDuration],
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

function newReference(pool) {
    return { ...blankRow(poolReferenceFields), pool, segment_defaults: blankSegmentDefaults() };
}

function referenceBlocks(reference, index) {
    return segmentDefaultsBlocks(reference.segment_defaults, ['pools', index, 'segment_defaults']);
}

function blocks(form) {
    return [
        ...form.pools.flatMap((reference, index) => [
            { row: reference, fields: poolReferenceFields, title: `Pool ${reference.pool}`, path: ['pools', index] },
            ...referenceBlocks(reference, index).map((block) => ({
                ...block,
                title: `Pool ${reference.pool}: ${block.label}`,
            })),
        ]),
        ...rotations.flatMap(({ key, label, entryFields }) =>
            form[key].entries.map((row, index) => ({
                row,
                fields: entryFields,
                title: `${label} ${index + 1}: ${row[entryFields[0].key]}`,
                path: [key, 'entries', index],
            })),
        ),
    ];
}

function formErrors(form) {
    const names = form.pools.map((reference) => reference.pool).filter((name) => name !== '');
    return new Set(names).size === names.length ? [] : ['Each pool can appear only once'];
}

const djConfigEditor = registryEditor({
    path: '/api/dj-configs',
    prefix: 'djConfig',
    nameInput: 'dj-config-editor-name',
    list: 'djConfigs',
    kind: 'dj-configs',
    label: 'DJ config',
    toForm,
    toBody,
    blankForm,
    blocks,
    formErrors,
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
        djConfigModeFields: poolModeFields,
        djConfigPools: {},
        djConfigSavedPools: {},
        djConfigChancesOpen: false,
        poolCreateFor: null,

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

        async loadDjConfigChancePools() {
            const missing = [
                ...new Set(
                    this.djConfigForm.pools
                        .map((reference) => reference.pool)
                        .filter(
                            (name) => name !== '' && !this.drafts.pools[name] && !(name in this.djConfigSavedPools),
                        ),
                ),
            ];
            await Promise.all(
                missing.map(async (name) => {
                    try {
                        const response = await apiRequest(`/api/pools/${encodeURIComponent(name)}`);
                        this.djConfigSavedPools[name] = poolToForm(name, await response.json());
                    } catch (error) {
                        console.error(`Failed to load pool ${name}:`, error);
                    }
                }),
            );
        },

        djConfigChances() {
            return this.djConfigForm.pools
                .map((reference, index) => ({ reference, share: this.weightShare(this.djConfigForm.pools, index) }))
                .filter(({ reference }) => reference.pool !== '')
                .map(({ reference, share }) => {
                    const pool = this.drafts.pools[reference.pool]?.form ?? this.djConfigSavedPools[reference.pool];
                    return {
                        pool: reference.pool,
                        loaded: pool !== undefined,
                        entries: (pool?.entries ?? []).map((entry, entryIndex) => ({
                            name: this.poolEntryName(entry),
                            chance:
                                (share *
                                    shareOf(
                                        pool.entries.map((row) => row.weight),
                                        entryIndex,
                                    )) /
                                100,
                        })),
                    };
                });
        },

        removeDjConfigPool(index) {
            this.djConfigForm.pools.splice(index, 1);
        },

        djConfigUnusedPools() {
            const used = this.djConfigForm.pools.map((reference) => reference.pool);
            return this.pools.filter((name) => !used.includes(name));
        },

        addDjConfigPool(pool) {
            if (pool === newItem) {
                this.poolCreateFor = this.route.name;
                this.openItem('pools', newItem);
                return;
            }
            this.djConfigForm.pools.push(newReference(pool));
        },

        addCreatedPoolToDjConfig(pool) {
            const draft = this.drafts['dj-configs'][this.poolCreateFor];
            if (!draft) {
                return;
            }
            draft.form.pools.push(newReference(pool));
            this.openItem('dj-configs', this.poolCreateFor);
        },

        djConfigReferenceBlocks(reference, index) {
            return referenceBlocks(reference, index);
        },

        djConfigDefaultsInvalid() {
            return this.blocksInvalid(this.djConfigForm.pools.flatMap(referenceBlocks));
        },

        djConfigDefaultsChangeCount() {
            const savedCount = this.openDraft('dj-configs')?.saved.pools.length ?? 0;
            return this.djConfigForm.pools
                .slice(0, savedCount)
                .reduce(
                    (total, _reference, index) =>
                        total + this.sectionChangeCount('dj-configs', ['pools', index, 'segment_defaults']),
                    0,
                );
        },

        toggleDjConfigReferenceTtsConfig(reference) {
            toggleTtsConfig(reference.segment_defaults);
        },

        addDjConfigRotationEntry(rotation) {
            const { entries } = this.djConfigForm[rotation.key];
            const row = blankRow(rotation.entryFields);
            if ('signal_preset' in row) {
                const used = entries.map((entry) => entry.signal_preset);
                row.signal_preset = this.presets.find((preset) => !used.includes(preset)) ?? '';
            }
            entries.push(row);
            this.focusWhenShown(`[data-rotation="${rotation.key}"] .list-row:has(+ .empty-state) input`);
        },
    };
}
