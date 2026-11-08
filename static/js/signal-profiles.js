import { apiRequest, emptyMessage } from './api.js';
import { blankRow, convertObject, fieldFromServer, fieldToServer } from './form-fields.js';
import { registryEditor } from './registry-editor.js';

export const modals = {
    signalProfile: {
        flag: 'showSignalProfileModal',
        perGuild: true,
        reset: () => ({
            selectedProfile: '',
            profileSetting: 'manual',
            profileFadeDuration: null,
            profileDuration: null,
            profileMessage: emptyMessage(),
        }),
    },
    profileEditor: {
        flag: 'showProfileEditor',
        perGuild: false,
        reset: () => ({ profileEditorMessage: emptyMessage() }),
    },
};

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
    { key: 'frequency_warble_hz', label: 'Frequency Warble (Hz)', type: 'number', step: 'any', optional: true },
];

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
    label: 'signal profile',
    toForm,
    toBody,
    blankForm,
});

export function signalProfiles() {
    return {
        ...profileEditor.state(),
        ...profileEditor.methods,
        showSignalProfileModal: false,
        selectedProfile: '',
        profileSetting: 'manual',
        profileFadeDuration: null,
        profileDuration: null,
        profileSaving: false,
        profileMessage: emptyMessage(),
        profileFields,

        async changeSignalProfile() {
            const body = {
                tier: this.profileSetting,
                profile: this.selectedProfile || null,
            };

            if (body.profile && this.profileFadeDuration !== null && this.profileFadeDuration !== '') {
                body.fade_duration = Number.parseFloat(this.profileFadeDuration);
            }
            if (body.profile && this.profileDuration !== null && this.profileDuration !== '') {
                body.duration = Number.parseFloat(this.profileDuration);
            }

            await this.runAction({
                saving: 'profileSaving',
                message: 'profileMessage',
                label: 'changing signal profile',
                request: () =>
                    apiRequest(`/api/guilds/${this.currentGuildId}/dj/profile`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Signal profile changed successfully!',
                failure: 'Failed to change signal profile',
                afterFn: () => this.closeModal('signalProfile'),
                afterMs: 1500,
            });
        },
    };
}
