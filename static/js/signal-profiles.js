import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const modals = {
    signalProfile: {
        flag: 'showSignalProfileModal',
        perGuild: true,
        reset: () => ({
            selectedProfile: '',
            profileSetting: 'profile',
            profileFadeDuration: null,
            profileMessage: emptyMessage(),
        }),
    },
    profileEditor: {
        flag: 'showProfileEditor',
        perGuild: false,
        reset: () => ({ profileEditorMessage: emptyMessage() }),
    },
};

export function signalProfiles() {
    return {
        // Signal Profile Modal state
        showSignalProfileModal: false,
        selectedProfile: '',
        profileSetting: 'profile',
        profileFadeDuration: null,
        profileSaving: false,
        profileMessage: { text: '', type: '' },
        profiles: [],
        // Signal Profile Editor state
        showProfileEditor: false,
        profileEditorIsNew: false,
        profileEditorLoading: false,
        profileEditorSaving: false,
        profileEditorMessage: { text: '', type: '' },
        profileForm: { dropout_duration_ms: [0, 0] },
        profileFields: [
            { key: 'bandpass_low', label: 'Bandpass Low (Hz)', step: 'any', min: 0 },
            { key: 'bandpass_high', label: 'Bandpass High (Hz)', step: 'any', min: 0 },
            { key: 'white_noise_level', label: 'White Noise Level (0-2)', step: 'any', min: 0, max: 2 },
            { key: 'pink_noise_level', label: 'Pink Noise Level (0-2)', step: 'any', min: 0, max: 2 },
            { key: 'brown_noise_level', label: 'Brown Noise Level (0-2)', step: 'any', min: 0, max: 2 },
            { key: 'tremolo_depth', label: 'Tremolo Depth', step: 'any' },
            { key: 'tremolo_rate', label: 'Tremolo Rate (Hz)', step: 'any' },
            { key: 'tremolo_jitter', label: 'Tremolo Jitter', step: 'any' },
            { key: 'clip_pregain', label: 'Clip Pregain', step: 'any' },
            { key: 'clip_threshold', label: 'Clip Threshold', step: 'any' },
            { key: 'bitcrush_bits', label: 'Bitcrush Bits', step: 1, min: 1, max: 255, optional: true },
            { key: 'dropout_probability', label: 'Dropout Probability', step: 'any', min: 0 },
            { key: 'frequency_warble_hz', label: 'Frequency Warble (Hz)', step: 'any', optional: true },
        ],

        async loadProfiles() {
            try {
                const response = await apiRequest('/api/profiles');
                const data = await response.json();
                this.profiles = data.profiles.sort();
            } catch (error) {
                console.error('Failed to load profiles:', error);
            }
        },

        async openProfileEditor(name) {
            this.profileEditorIsNew = name === null;
            this.openModal('profileEditor');

            if (this.profileEditorIsNew) {
                this.profileForm = {
                    name: '',
                    bandpass_low: 20,
                    bandpass_high: 20000,
                    white_noise_level: 0,
                    pink_noise_level: 0,
                    brown_noise_level: 0,
                    tremolo_depth: 0,
                    tremolo_rate: 0,
                    tremolo_jitter: 0,
                    clip_pregain: 1,
                    clip_threshold: 1,
                    bitcrush_bits: '',
                    dropout_probability: 0,
                    dropout_duration_ms: [0, 0],
                    frequency_warble_hz: '',
                };
                return;
            }

            this.profileEditorLoading = true;
            try {
                const response = await apiRequest(`/api/profiles/${encodeURIComponent(name)}`, {
                    errorMessage: (failed) => `HTTP ${failed.status}`,
                });
                const profile = await response.json();
                for (const field of this.profileFields) {
                    if (profile[field.key] === null) {
                        profile[field.key] = '';
                    }
                }
                this.profileForm = profile;
            } catch (error) {
                console.error('Failed to load signal profile:', error);
                this.profileEditorMessage = { text: `Failed to load profile: ${error.message}`, type: 'error' };
            } finally {
                this.profileEditorLoading = false;
            }
        },

        async saveProfile() {
            const name = this.profileForm.name;
            if (this.profileEditorIsNew && this.profiles.includes(name)) {
                this.profileEditorMessage = { text: `A profile named "${name}" already exists`, type: 'error' };
                return;
            }

            const { name: _, ...body } = this.profileForm;
            for (const field of this.profileFields) {
                if (field.optional && body[field.key] === '') {
                    body[field.key] = null;
                }
            }

            this.profileEditorSaving = true;
            this.profileEditorMessage = { text: '', type: '' };
            try {
                await apiRequest(`/api/profiles/${encodeURIComponent(name)}`, {
                    method: 'PUT',
                    body,
                    errorMessage: bodyOrStatus,
                });
                this.profileEditorIsNew = false;
                this.profileEditorMessage = { text: 'Signal profile saved', type: 'success' };
                await this.loadProfiles();
            } catch (error) {
                console.error('Failed to save signal profile:', error);
                this.profileEditorMessage = { text: `Failed to save profile: ${error.message}`, type: 'error' };
            } finally {
                this.profileEditorSaving = false;
            }
        },

        // Signal Profile Modal Functions
        async changeSignalProfile() {
            const body = {
                profile: this.selectedProfile || null,
            };

            if (body.profile && this.profileFadeDuration !== null && this.profileFadeDuration !== '') {
                body.fade_duration = Number.parseFloat(this.profileFadeDuration);
            }

            await this.runAction({
                saving: 'profileSaving',
                message: 'profileMessage',
                label: 'changing signal profile',
                request: () =>
                    apiRequest(`/api/guilds/${this.currentGuildId}/dj/${this.profileSetting}`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Signal profile changed successfully!',
                failure: 'Failed to change signal profile. Please try again.',
                afterFn: () => this.closeModal('signalProfile'),
                afterMs: 1500,
            });
        },
    };
}
