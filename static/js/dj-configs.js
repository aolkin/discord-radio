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

const scalarGroups = [
    {
        title: 'General',
        key: null,
        fields: [
            { key: 'recent_history_size', label: 'Recent History Size', type: 'number', step: 1, min: 0 },
            { key: 'duplicate_penalty_multiplier', label: 'Duplicate Penalty Multiplier', type: 'number', step: 'any' },
            optionalText('channel_status', 'Channel Status'),
        ],
    },
    {
        title: 'State Weights',
        key: 'state_weights',
        fields: [
            { key: 'track', label: 'Track', type: 'number', step: 1, min: 0 },
            { key: 'hex_message', label: 'Hex Message', type: 'number', step: 1, min: 0 },
            { key: 'noise', label: 'Noise', type: 'number', step: 1, min: 0 },
        ],
    },
    {
        title: 'Hex Message Defaults',
        key: 'hex_message_defaults',
        fields: [
            { key: 'loop_min', label: 'Loop Min', type: 'number', step: 1, min: 0 },
            { key: 'loop_max', label: 'Loop Max', type: 'number', step: 1, min: 0 },
            profileText('signal_profile', 'Signal Profile', true),
        ],
    },
];

const lists = [
    {
        key: 'track_pool',
        title: 'Track Pool',
        itemLabel: 'Track',
        fields: [
            { key: 'filename', label: 'Filename', type: 'text' },
            weight,
            optionalSeconds('max_duration_seconds', 'Max Duration (s)'),
            { key: 'allow_subsection', label: 'Allow Subsection', type: 'bool', optional: true },
            profileText('signal_profile', 'Signal Profile', true),
            { key: 'volume', label: 'Volume (0-2)', type: 'number', step: 'any', min: 0, max: 2, optional: true },
            optionalText('channel_status', 'Channel Status'),
        ],
    },
    {
        key: 'hex_messages',
        title: 'Hex Messages',
        itemLabel: 'Hex Message',
        fields: [
            { key: 'text', label: 'Text', type: 'text' },
            weight,
            profileText('signal_profile', 'Signal Profile', true),
            { key: 'loop_min', label: 'Loop Min', type: 'number', step: 1, min: 0, optional: true },
            { key: 'loop_max', label: 'Loop Max', type: 'number', step: 1, min: 0, optional: true },
            optionalText('announcement', 'Announcement'),
        ],
    },
    {
        key: 'noise_periods',
        title: 'Noise Periods',
        itemLabel: 'Noise Period',
        fields: [
            profileText('noise_profile', 'Noise Profile'),
            seconds('min_duration_seconds', 'Min Duration (s)'),
            seconds('max_duration_seconds', 'Max Duration (s)'),
            weight,
        ],
    },
    {
        key: 'signal_profiles',
        title: 'Signal Profile Rotation',
        itemLabel: 'Signal Profile',
        fields: [
            profileText('profile_name', 'Profile'),
            weight,
            seconds('fade_duration_seconds', 'Fade Duration (s)'),
            seconds('min_time_seconds', 'Min Time (s)'),
            seconds('max_time_seconds', 'Max Time (s)'),
        ],
    },
];

function fieldFromServer(field, value) {
    if (value === null || value === undefined) {
        return '';
    }
    return field.type === 'bool' ? String(value) : value;
}

function fieldToServer(field, value) {
    if (value === '') {
        return null;
    }
    return field.type === 'bool' ? value === 'true' : value;
}

function convertObject(fields, source, convert) {
    return Object.fromEntries(fields.map((field) => [field.key, convert(field, source[field.key])]));
}

function toForm(config) {
    const form = { name: config.name, hex_message_announcements: config.hex_message_announcements ?? [] };
    for (const group of scalarGroups) {
        const converted = convertObject(group.fields, group.key ? config[group.key] : config, fieldFromServer);
        if (group.key) {
            form[group.key] = converted;
        } else {
            Object.assign(form, converted);
        }
    }
    for (const list of lists) {
        form[list.key] = config[list.key].map((row) => convertObject(list.fields, row, fieldFromServer));
    }
    return form;
}

function toBody(form) {
    const body = { hex_message_announcements: form.hex_message_announcements };
    for (const group of scalarGroups) {
        const converted = convertObject(group.fields, group.key ? form[group.key] : form, fieldToServer);
        if (group.key) {
            body[group.key] = converted;
        } else {
            Object.assign(body, converted);
        }
    }
    for (const list of lists) {
        body[list.key] = form[list.key].map((row) => convertObject(list.fields, row, fieldToServer));
    }
    return body;
}

function blankRow(list) {
    return Object.fromEntries(
        list.fields.map((field) => [field.key, field.type === 'number' && !field.optional ? 0 : '']),
    );
}

function blankForm() {
    return {
        name: '',
        recent_history_size: 5,
        duplicate_penalty_multiplier: 0.5,
        channel_status: '',
        state_weights: { track: 1, hex_message: 1, noise: 1 },
        hex_message_defaults: { loop_min: 1, loop_max: 1, signal_profile: '' },
        hex_message_announcements: [],
        track_pool: [],
        hex_messages: [],
        noise_periods: [],
        signal_profiles: [],
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
        djConfigScalarGroups: scalarGroups,
        djConfigLists: lists,

        async loadDjConfigs() {
            try {
                const response = await apiRequest('/api/dj-configs');
                const data = await response.json();
                this.djConfigs = data.configs.sort();
            } catch (error) {
                console.error('Failed to load DJ configs:', error);
            }
        },

        djConfigGroupValues(group) {
            return group.key ? this.djConfigForm[group.key] : this.djConfigForm;
        },

        addDjConfigRow(list) {
            this.djConfigForm[list.key].push(blankRow(list));
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
