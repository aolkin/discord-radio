import { apiRequest, emptyMessage } from './api.js';

export const modals = {
    channelMessage: {
        flag: 'showChannelMessageModal',
        perGuild: false,
        reset: () => ({
            registeredChannels: [],
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
        registeredChannels: [],
        channelsLoading: false,
        selectedChannelId: '',
        channelMessageText: '',
        channelMessageTitle: '',
        channelMessageDescription: '',
        channelMessageSaving: false,
        channelMessage: { text: '', type: '' },

        // Channel Message Functions
        async openChannelMessageModal() {
            this.openModal('channelMessage');
            this.channelsLoading = true;

            try {
                const response = await apiRequest('/api/registered-channels', {
                    errorMessage: 'Failed to load registered channels',
                    includeStatus: true,
                });

                const data = await response.json();
                this.registeredChannels = data.channels;
            } catch (error) {
                console.error('Error loading registered channels:', error);
                this.channelMessage = {
                    type: 'error',
                    text: error.message || 'Failed to load registered channels. Please try again.',
                };
            } finally {
                this.channelsLoading = false;
            }
        },

        async sendChannelMessage() {
            this.channelMessageSaving = true;
            this.channelMessage = emptyMessage();

            try {
                // Validate at least one field is filled
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

                await apiRequest(`/api/registered-channels/${this.selectedChannelId}/send`, {
                    method: 'POST',
                    body,
                    errorMessage: 'Failed to send message',
                    includeStatus: true,
                });

                this.channelMessage = { type: 'success', text: 'Message sent successfully!' };

                // Clear form fields after successful send
                this.channelMessageText = '';
                this.channelMessageTitle = '';
                this.channelMessageDescription = '';

                setTimeout(() => {
                    if (this.channelMessage.type === 'success') this.channelMessage = emptyMessage();
                }, 3000);
            } catch (error) {
                console.error('Error sending message:', error);
                this.channelMessage = {
                    type: 'error',
                    text: error.message || 'Failed to send message. Please try again.',
                };
            } finally {
                this.channelMessageSaving = false;
            }
        },
    };
}
