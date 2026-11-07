import { apiRequest, emptyMessage } from './api.js';

export const modals = {
    advanceDJ: {
        flag: 'showAdvanceDJModal',
        perGuild: true,
        reset: () => ({ djStateType: '', djMessage: emptyMessage() }),
    },
    playHex: {
        flag: 'showPlayHexModal',
        perGuild: true,
        reset: () => ({ hexMessageText: '', hexMessage: emptyMessage() }),
    },
    channelStatus: {
        flag: 'showChannelStatusModal',
        perGuild: true,
        reset: () => ({
            channelStatusSetting: 'background',
            channelStatusText: '',
            channelStatusMessage: emptyMessage(),
        }),
    },
};

export function dj() {
    return {
        // DJ Modal state
        showAdvanceDJModal: false,
        djStateType: '',
        djSaving: false,
        djMessage: { text: '', type: '' },
        // Hex Message Modal state
        showPlayHexModal: false,
        hexMessageText: '',
        hexSaving: false,
        hexMessage: { text: '', type: '' },
        // Channel Status Modal state
        showChannelStatusModal: false,
        channelStatusSetting: 'background',
        channelStatusText: '',
        channelStatusSaving: false,
        channelStatusMessage: { text: '', type: '' },

        async advanceDJState() {
            await this.runAction({
                saving: 'djSaving',
                message: 'djMessage',
                label: 'advancing DJ state',
                request: () =>
                    apiRequest(`/api/guilds/${this.currentGuildId}/dj/advance`, {
                        method: 'POST',
                        body: { state_type: this.djStateType || null },
                    }),
                success: 'DJ state advanced successfully!',
                failure: 'Failed to advance DJ state. Please try again.',
                afterFn: () => this.closeModal('advanceDJ'),
                afterMs: 1500,
            });
        },

        // Hex Message Modal Functions
        async playHexMessage() {
            await this.runAction({
                saving: 'hexSaving',
                message: 'hexMessage',
                label: 'playing hex message',
                request: () =>
                    apiRequest(`/api/guilds/${this.currentGuildId}/hex/play`, {
                        method: 'POST',
                        body: { message: this.hexMessageText },
                    }),
                success: 'Hex message queued successfully!',
                failure: 'Failed to play hex message. Please try again.',
                afterFn: () => this.closeModal('playHex'),
                afterMs: 1500,
            });
        },

        async changeChannelStatus() {
            const status = this.channelStatusText || null;
            const [path, body] = {
                background: ['channel-status', { status }],
                override: ['channel-status-override', { status: status && { fixed: status } }],
                unmanaged: ['channel-status-override', { status: 'unmanaged' }],
            }[this.channelStatusSetting];

            await this.runAction({
                saving: 'channelStatusSaving',
                message: 'channelStatusMessage',
                label: 'changing channel status',
                request: () =>
                    apiRequest(`/api/guilds/${this.currentGuildId}/dj/${path}`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Channel status changed successfully!',
                failure: 'Failed to change channel status. Please try again.',
                afterFn: () => this.closeModal('channelStatus'),
                afterMs: 1500,
            });
        },
    };
}
