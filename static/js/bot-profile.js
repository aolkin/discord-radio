import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const modals = {
    botProfile: {
        flag: 'showBotProfileModal',
        perGuild: true,
        reset: () => ({
            botProfileMessage: emptyMessage(),
            botProfileForm: { nick: '', avatar: '', banner: '' },
        }),
    },
};

export function botProfile() {
    return {
        // Bot Profile Modal state
        showBotProfileModal: false,
        botProfileLoading: false,
        botProfileSaving: false,
        botProfileMessage: { text: '', type: '' },
        botProfileCurrent: { nick: null, avatar_url: null, banner_url: null },
        botProfileForm: { nick: '', avatar: '', banner: '' },

        openBotProfileModal(guildId) {
            this.openModal('botProfile', guildId);
            return this.loadBotProfile();
        },

        async loadBotProfile() {
            this.botProfileLoading = true;
            try {
                const response = await apiRequest(`/api/guilds/${this.currentGuildId}/bot-profile`, {
                    errorMessage: bodyOrStatus,
                });
                this.botProfileCurrent = await response.json();
                this.botProfileForm.nick = this.botProfileCurrent.nick || '';
            } catch (error) {
                console.error('Failed to load bot profile:', error);
                this.botProfileMessage = { text: `Failed to load the current profile: ${error.message}`, type: 'error' };
            } finally {
                this.botProfileLoading = false;
            }
        },

        pickBotProfileImage(field) {
            this.openFileBrowser({
                mode: 'file',
                prefix: 'images/',
                onSelect: (key) => { this.botProfileForm[field] = 's3://' + key; },
            });
        },

        async saveBotProfile() {
            const body = {};
            if (this.botProfileForm.nick !== (this.botProfileCurrent.nick || '')) body.nick = this.botProfileForm.nick;
            if (this.botProfileForm.avatar) body.avatar = this.botProfileForm.avatar;
            if (this.botProfileForm.banner) body.banner = this.botProfileForm.banner;
            if (Object.keys(body).length === 0) {
                this.botProfileMessage = { text: 'Nothing changed', type: 'error' };
                return;
            }

            this.botProfileSaving = true;
            this.botProfileMessage = { text: '', type: '' };
            try {
                await apiRequest(`/api/guilds/${this.currentGuildId}/bot-profile`, {
                    method: 'PUT',
                    body,
                    errorMessage: bodyOrStatus,
                });
                this.botProfileMessage = { text: 'Profile saved', type: 'success' };
                this.botProfileForm.avatar = '';
                this.botProfileForm.banner = '';
                await this.loadBotProfile();
            } catch (error) {
                console.error('Failed to save bot profile:', error);
                this.botProfileMessage = { text: `Failed to save the profile: ${error.message}`, type: 'error' };
            } finally {
                this.botProfileSaving = false;
            }
        },
    };
}
