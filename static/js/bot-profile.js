import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const modals = {
    botProfile: {
        flag: 'showBotProfileModal',
        reset: () => botProfileForms(),
    },
};

export const botProfileForms = () => ({
    botProfileMessage: emptyMessage(),
    botProfileForm: { nick: '', avatar: '', banner: '' },
});

export function botProfile() {
    return {
        showBotProfileModal: false,
        botProfileGuildId: null,
        botProfileLoading: false,
        botProfileSaving: false,
        botProfileCurrent: { nick: null, avatar_url: null, banner_url: null },
        ...botProfileForms(),

        openBotProfileModal(guildId) {
            this.openModal('botProfile');
            return this.loadBotProfile(guildId);
        },

        async loadBotProfile(guildId) {
            this.botProfileGuildId = guildId;
            this.botProfileLoading = true;
            try {
                const response = await apiRequest(`/api/guilds/${guildId}/bot-profile`, {
                    errorMessage: bodyOrStatus,
                });
                const current = await response.json();
                if (guildId === this.botProfileGuildId) {
                    this.botProfileCurrent = current;
                    this.botProfileForm.nick = current.nick || '';
                }
            } catch (error) {
                console.error('Failed to load bot profile:', error);
                this.botProfileMessage = {
                    text: `Failed to load the current profile: ${error.message}`,
                    type: 'error',
                };
            } finally {
                this.botProfileLoading = false;
            }
        },

        pickBotProfileImage(field) {
            this.openFileBrowser({
                mode: 'file',
                prefix: 'images/',
                onSelect: (key) => {
                    this.botProfileForm[field] = `s3://${key}`;
                },
            });
        },

        async saveBotProfile() {
            const body = {};
            if (this.botProfileForm.nick !== (this.botProfileCurrent.nick || '')) {
                body.nick = this.botProfileForm.nick;
            }
            if (this.botProfileForm.avatar) {
                body.avatar = this.botProfileForm.avatar;
            }
            if (this.botProfileForm.banner) {
                body.banner = this.botProfileForm.banner;
            }
            if (Object.keys(body).length === 0) {
                this.botProfileMessage = { text: 'Nothing changed', type: 'error' };
                return;
            }

            await this.runAction({
                saving: 'botProfileSaving',
                message: 'botProfileMessage',
                label: 'saving bot profile',
                request: () =>
                    apiRequest(`/api/guilds/${this.botProfileGuildId}/bot-profile`, {
                        method: 'PUT',
                        body,
                        errorMessage: bodyOrStatus,
                    }),
                onSuccess: async () => {
                    this.botProfileForm.avatar = '';
                    this.botProfileForm.banner = '';
                    await this.loadBotProfile(this.botProfileGuildId);
                },
                success: 'Profile saved',
                failure: 'Failed to save the profile',
            });
        },
    };
}
