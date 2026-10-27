import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';
import {
    blankRow,
    convertObject,
    fieldFromServer,
    fieldToServer,
    historyPenaltyMultiplier,
    profileText,
    seconds,
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

const signalProfilesFields = [historyPenaltyMultiplier];

const signalProfileFields = [
    profileText('noise_profile', 'Noise Profile'),
    weight,
    seconds('fade_duration_seconds', 'Fade Duration (s)'),
    seconds('min_duration_seconds', 'Min Duration (s)'),
    seconds('max_duration_seconds', 'Max Duration (s)'),
];

function toForm(config) {
    return {
        name: config.name,
        ...convertObject(generalFields, config, fieldFromServer),
        pools: Object.entries(config.pools).map(([pool, reference]) =>
            convertObject(poolReferenceFields, { pool, ...reference }, fieldFromServer),
        ),
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
        pools: Object.fromEntries(
            form.pools.map((reference) => {
                const { pool, ...settings } = convertObject(poolReferenceFields, reference, fieldToServer);
                return [pool, settings];
            }),
        ),
        signal_profiles: {
            ...convertObject(signalProfilesFields, form.signal_profiles, fieldToServer),
            entries: form.signal_profiles.entries.map((row) => convertObject(signalProfileFields, row, fieldToServer)),
        },
    };
}

function blankForm() {
    return {
        name: '',
        ...blankRow(generalFields),
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
        djConfigGeneralFields: generalFields,
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

        djConfigPoolOptions(reference) {
            return this.pools.includes(reference.pool) || reference.pool === ''
                ? this.pools
                : [...this.pools, reference.pool];
        },

        addDjConfigPool() {
            this.djConfigForm.pools.push({ ...blankRow(poolReferenceFields), pool: this.pools[0] ?? '' });
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
