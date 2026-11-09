import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const modals = {
    channelMessage: {
        flag: 'showChannelMessageModal',
        perGuild: false,
        reset: () => ({
            messageGuilds: [],
            selectedGuildId: '',
            sendableChannels: [],
            selectedChannelId: '',
            channelMessageText: '',
            channelMessageTitle: '',
            channelMessageDescription: '',
            channelMessage: emptyMessage(),
        }),
    },
};

export function channels() {
    return {
        // Channel Message state
        showChannelMessageModal: false,
        messageGuilds: [],
        selectedGuildId: '',
        sendableChannels: [],
        channelsLoading: false,
        channelListLoading: false,
        selectedChannelId: '',
        channelMessageText: '',
        channelMessageTitle: '',
        channelMessageDescription: '',
        channelMessageSaving: false,
        channelMessage: emptyMessage(),

        // Channel Message Functions
        async openChannelMessageModal() {
            this.openModal('channelMessage');
            this.channelsLoading = true;

            try {
                const response = await apiRequest('/api/guilds', {
                    errorMessage: bodyOrStatus,
                });

                const data = await response.json();
                this.messageGuilds = data.guilds;
            } catch (error) {
                console.error('Error loading servers:', error);
                this.channelMessage = {
                    type: 'error',
                    text: error.message || 'Failed to load servers. Please try again.',
                };
            } finally {
                this.channelsLoading = false;
            }
        },

        async loadSendableChannels() {
            this.sendableChannels = [];
            this.selectedChannelId = '';
            this.channelMessage = emptyMessage();
            if (!this.selectedGuildId) {
                return;
            }
            const guildId = this.selectedGuildId;
            this.channelListLoading = true;

            try {
                const response = await apiRequest(`/api/guilds/${guildId}/channels`, {
                    errorMessage: bodyOrStatus,
                });

                const data = await response.json();
                if (guildId === this.selectedGuildId) {
                    this.sendableChannels = data.channels;
                }
            } catch (error) {
                console.error('Error loading channels:', error);
                this.channelMessage = {
                    type: 'error',
                    text: error.message || 'Failed to load channels. Please try again.',
                };
            } finally {
                this.channelListLoading = false;
            }
        },

        async sendChannelMessage() {
            if (!this.channelMessageText && !this.channelMessageTitle && !this.channelMessageDescription) {
                this.channelMessage = {
                    type: 'error',
                    text: 'Please provide at least a message, title, or description.',
                };
                return;
            }

            const body = {};
            if (this.channelMessageText) {
                body.message = this.channelMessageText;
            }
            if (this.channelMessageTitle) {
                body.title = this.channelMessageTitle;
            }
            if (this.channelMessageDescription) {
                body.description = this.channelMessageDescription;
            }

            await this.runAction({
                saving: 'channelMessageSaving',
                message: 'channelMessage',
                label: 'sending message',
                request: () =>
                    apiRequest(`/api/guilds/${this.selectedGuildId}/channels/${this.selectedChannelId}/send`, {
                        method: 'POST',
                        body,
                        errorMessage: bodyOrStatus,
                    }),
                onSuccess: () => {
                    this.channelMessageText = '';
                    this.channelMessageTitle = '';
                    this.channelMessageDescription = '';
                },
                success: 'Message sent successfully!',
                failure: 'Failed to send message',
                afterFn: () => {
                    if (this.channelMessage.type === 'success') {
                        this.channelMessage = emptyMessage();
                    }
                },
                afterMs: 3000,
            });
        },
    };
}
