import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';
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

function toForm(config) {
    return {
        name: config.name,
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

export function djConfigs() {
    return {
        djConfigs: [],
        showDjConfigEditor: false,
        djConfigEditorIsNew: false,
        djConfigEditorLoading: false,
        djConfigEditorSaving: false,
        djConfigEditorMessage: emptyMessage(),
        djConfigForm: blankForm(),
        djConfigGeneralFields: generalFields,
        djConfigRotationFields: rotationFields,
        djConfigRotations: rotations,

        async loadDjConfigs() {
            try {
                const response = await apiRequest('/api/dj-configs');
                const data = await response.json();
                this.djConfigs = data.configs.sort();
            } catch (error) {
                console.error('Failed to load DJ configs:', error);
            }
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

        djConfigReferenceBlocks(reference) {
            return segmentDefaultsBlocks(reference.segment_defaults);
        },

        toggleDjConfigReferenceHexConfig(reference) {
            toggleHexConfig(reference.segment_defaults);
        },

        addDjConfigRotationEntry(rotation) {
            this.djConfigForm[rotation.key].entries.push(blankRow(rotation.entryFields));
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

            await this.runAction({
                saving: 'djConfigEditorSaving',
                message: 'djConfigEditorMessage',
                label: 'saving DJ config',
                request: () =>
                    apiRequest(`/api/dj-configs/${encodeURIComponent(name)}`, {
                        method: 'PUT',
                        body: toBody(this.djConfigForm),
                        errorMessage: bodyOrStatus,
                    }),
                onSuccess: async () => {
                    this.djConfigEditorIsNew = false;
                    await this.loadDjConfigs();
                },
                success: 'DJ config saved',
                failure: 'Failed to save DJ config',
            });
        },
    };
}
