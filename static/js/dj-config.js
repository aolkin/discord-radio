import { apiRequest, emptyMessage } from './api.js';

const emptyHexMessageForm = () => ({ index: null, text: '', weight: 1, signal_profile: '', loop_min: null, loop_max: null, announcement: '' });

export const modals = {
    djConfig: {
        flag: 'showDJConfigModal',
        perGuild: true,
        reset: () => ({ djConfigError: null, djConfigMessage: emptyMessage() }),
    },
    hexMessageForm: {
        flag: 'showHexMessageForm',
        perGuild: false,
        reset: () => ({ hexMessageFormData: emptyHexMessageForm() }),
    },
    announcementForm: {
        flag: 'showAnnouncementForm',
        perGuild: false,
        reset: () => ({ announcementFormData: { index: null, text: '' } }),
    },
};

export function djConfig() {
    return {
        // DJ Config state
        showDJConfigModal: false,
        djConfigLoading: false,
        djConfigSaving: false,
        djConfigError: null,
        djConfigMessage: { text: '', type: '' },
        djConfigData: { 
            hex_messages: { enabled: false, items: [] }, 
            hex_message_announcements: { enabled: false, items: [] },
            state_weights: { enabled: false, value: { track: 100, hex_message: 2, noise: 50 } }
        },
        defaultStateWeights: { track: 100, hex_message: 2, noise: 50 },
        showHexMessageForm: false,
        hexMessageFormData: { index: null, text: '', weight: 1, signal_profile: '', loop_min: null, loop_max: null, announcement: '' },
        showAnnouncementForm: false,
        announcementFormData: { index: null, text: '' },

        // DJ Config Override Functions
        async openDJConfigModal(guildId) {
            this.openModal('djConfig', guildId);
            await this.loadDJConfigOverrides();
        },

        async loadDJConfigOverrides() {
            this.djConfigLoading = true;
            this.djConfigError = null;
            this.djConfigMessage = { text: '', type: '' };

            try {
                // Load both overrides and default state weights in parallel
                const [overridesResponse, defaultWeightsResponse] = await Promise.all([
                    fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides`),
                    fetch('/api/dj-config/default-state-weights')
                ]);
                
                if (!overridesResponse.ok) {
                    throw new Error('Failed to load DJ config overrides');
                }
                
                this.djConfigData = await overridesResponse.json();
                
                // Load default state weights
                if (defaultWeightsResponse.ok) {
                    this.defaultStateWeights = await defaultWeightsResponse.json();
                    
                    // If state weights override has no value, initialize with defaults
                    if (!this.djConfigData.state_weights.value) {
                        this.djConfigData.state_weights.value = { ...this.defaultStateWeights };
                    }
                }
            } catch (error) {
                console.error('Error loading DJ config:', error);
                this.djConfigError = 'Failed to load DJ config overrides. Please try again.';
            } finally {
                this.djConfigLoading = false;
            }
        },

        async toggleDJConfigCategory(category) {
            const enabled = this.djConfigData[category].enabled;
            await this.runDJConfigAction({
                label: 'toggling category',
                request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/toggle`, {
                    method: 'POST',
                    body: { category, enabled },
                }),
                success: `${category.replace('_', ' ')} ${enabled ? 'enabled' : 'disabled'} successfully!`,
                failure: 'Failed to toggle category. Please try again.',
            });
        },

        runDJConfigAction(options) {
            return this.runAction({
                saving: 'djConfigSaving',
                message: 'djConfigMessage',
                afterFn: () => { this.djConfigMessage = { text: '', type: '' }; },
                afterMs: 2000,
                ...options,
            });
        },

        openHexMessageForm(index = null) {
            this.openModal('hexMessageForm');
            if (index !== null) {
                const msg = this.djConfigData.hex_messages.items[index];
                this.hexMessageFormData = {
                    index,
                    text: msg.text,
                    weight: msg.weight,
                    signal_profile: msg.signal_profile || '',
                    loop_min: msg.loop_min || null,
                    loop_max: msg.loop_max || null,
                    announcement: msg.announcement || ''
                };
            }
        },

        editHexMessage(index) {
            this.openHexMessageForm(index);
        },

        async saveHexMessage() {
            const payload = {
                index: this.hexMessageFormData.index,
                text: this.hexMessageFormData.text,
                weight: this.hexMessageFormData.weight,
                signal_profile: this.hexMessageFormData.signal_profile || null,
                loop_min: this.hexMessageFormData.loop_min || null,
                loop_max: this.hexMessageFormData.loop_max || null,
                announcement: this.hexMessageFormData.announcement || null
            };

            await this.runDJConfigAction({
                label: 'saving hex message',
                request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/hex-messages`, {
                    method: 'POST',
                    body: payload,
                }),
                onSuccess: async () => {
                    await this.loadDJConfigOverrides();
                    this.closeModal('hexMessageForm');
                },
                success: 'Hex message saved successfully!',
                failure: 'Failed to save hex message. Please try again.',
            });
        },

        async deleteHexMessage(index) {
            if (!confirm('Are you sure you want to delete this hex message?')) {
                return;
            }
            
            await this.runDJConfigAction({
                label: 'deleting hex message',
                request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/hex-messages/${index}`, {
                    method: 'DELETE',
                }),
                onSuccess: () => this.loadDJConfigOverrides(),
                success: 'Hex message deleted successfully!',
                failure: 'Failed to delete hex message. Please try again.',
            });
        },

        openAnnouncementForm(index = null) {
            this.openModal('announcementForm');
            if (index !== null) {
                this.announcementFormData = {
                    index,
                    text: this.djConfigData.hex_message_announcements.items[index]
                };
            }
        },

        editAnnouncement(index) {
            this.openAnnouncementForm(index);
        },

        async saveAnnouncement() {
            const payload = {
                index: this.announcementFormData.index,
                text: this.announcementFormData.text
            };

            await this.runDJConfigAction({
                label: 'saving announcement',
                request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/announcements`, {
                    method: 'POST',
                    body: payload,
                }),
                onSuccess: async () => {
                    await this.loadDJConfigOverrides();
                    this.closeModal('announcementForm');
                },
                success: 'Announcement saved successfully!',
                failure: 'Failed to save announcement. Please try again.',
            });
        },

        async deleteAnnouncement(index) {
            if (!confirm('Are you sure you want to delete this announcement?')) {
                return;
            }
            
            await this.runDJConfigAction({
                label: 'deleting announcement',
                request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/announcements/${index}`, {
                    method: 'DELETE',
                }),
                onSuccess: () => this.loadDJConfigOverrides(),
                success: 'Announcement deleted successfully!',
                failure: 'Failed to delete announcement. Please try again.',
            });
        },

        async saveStateWeights() {
            const weights = this.djConfigData.state_weights.value;
            await this.runDJConfigAction({
                label: 'saving state weights',
                request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/state-weights`, {
                    method: 'POST',
                    body: {
                        track: weights.track,
                        hex_message: weights.hex_message,
                        noise: weights.noise,
                    },
                }),
                success: 'State weights saved successfully!',
                failure: 'Failed to save state weights. Please try again.',
            });
        },
    };
}
