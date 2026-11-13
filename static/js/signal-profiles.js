import { apiRequest, emptyMessage } from './api.js';
import { blankRow, convertObject, fieldFromServer, fieldToServer } from './form-fields.js';
import { registryEditor } from './registry-editor.js';

export const signalProfileForms = () => ({
    signalProfileSetting: 'manual',
    signalProfileName: '',
    signalProfileFade: null,
    signalProfileDuration: null,
    signalProfileMessage: emptyMessage(),
});

const profileFields = [
    { key: 'bandpass_low', label: 'Bandpass Low (Hz)', type: 'number', step: 'any', min: 0, default: 20 },
    { key: 'bandpass_high', label: 'Bandpass High (Hz)', type: 'number', step: 'any', min: 0, default: 20000 },
    { key: 'white_noise_level', label: 'White Noise Level (0-2)', type: 'number', step: 'any', min: 0, max: 2 },
    { key: 'pink_noise_level', label: 'Pink Noise Level (0-2)', type: 'number', step: 'any', min: 0, max: 2 },
    { key: 'brown_noise_level', label: 'Brown Noise Level (0-2)', type: 'number', step: 'any', min: 0, max: 2 },
    { key: 'tremolo_depth', label: 'Tremolo Depth', type: 'number', step: 'any' },
    { key: 'tremolo_rate', label: 'Tremolo Rate (Hz)', type: 'number', step: 'any' },
    { key: 'tremolo_jitter', label: 'Tremolo Jitter', type: 'number', step: 'any' },
    { key: 'clip_pregain', label: 'Clip Pregain', type: 'number', step: 'any', default: 1 },
    { key: 'clip_threshold', label: 'Clip Threshold', type: 'number', step: 'any', default: 1 },
    { key: 'bitcrush_bits', label: 'Bitcrush Bits', type: 'number', step: 1, min: 1, max: 255, optional: true },
    { key: 'dropout_probability', label: 'Dropout Probability', type: 'number', step: 'any', min: 0 },
];

const dropoutFields = [
    { key: 0, label: 'Dropout Min Duration (ms)', type: 'number', step: 'any', min: 0 },
    { key: 1, label: 'Dropout Max Duration (ms)', type: 'number', step: 'any', min: 0 },
];

function blocks(form) {
    return [
        { row: form, fields: profileFields, path: [] },
        { row: form.dropout_duration_ms, fields: dropoutFields, path: ['dropout_duration_ms'] },
    ];
}

function toForm(name, profile) {
    return {
        name,
        ...convertObject(profileFields, profile, fieldFromServer),
        dropout_duration_ms: [...profile.dropout_duration_ms],
    };
}

function toBody(form) {
    return {
        ...convertObject(profileFields, form, fieldToServer),
        dropout_duration_ms: form.dropout_duration_ms,
    };
}

function blankForm() {
    return { name: '', ...blankRow(profileFields), dropout_duration_ms: [0, 0] };
}

const profileEditor = registryEditor({
    path: '/api/profiles',
    prefix: 'profile',
    list: 'profiles',
    kind: 'signal-profiles',
    label: 'signal profile',
    toForm,
    toBody,
    blankForm,
    blocks,
});

export function signalProfiles() {
    return {
        ...profileEditor.state(),
        ...profileEditor.methods,
        ...signalProfileForms(),
        signalProfileSaving: false,
        profileFieldRows() {
            return blocks(this.profileForm).flatMap(({ row, fields, path }) =>
                fields.map((field) => ({ row, field, path: [...path, field.key] })),
            );
        },

        async changeSignalProfile() {
            const body = {
                tier: this.signalProfileSetting,
                profile: this.signalProfileName || null,
            };

            if (body.profile && this.signalProfileFade !== null && this.signalProfileFade !== '') {
                body.fade_duration = Number.parseFloat(this.signalProfileFade);
            }
            if (body.profile && this.signalProfileDuration !== null && this.signalProfileDuration !== '') {
                body.duration = Number.parseFloat(this.signalProfileDuration);
            }

            await this.runAction({
                saving: 'signalProfileSaving',
                message: 'signalProfileMessage',
                label: 'changing signal profile',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/profile`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Signal profile changed successfully!',
                failure: 'Failed to change signal profile',
            });
        },
    };
}
