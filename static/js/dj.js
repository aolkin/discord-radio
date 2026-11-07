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
        reset: () => ({ hexMessageText: '', hexMessagePool: '', hexMessage: emptyMessage() }),
    },
    channelStatus: {
        flag: 'showChannelStatusModal',
        perGuild: true,
        reset: () => ({
            channelStatusSetting: 'background',
            channelStatusText: '',
            channelStatusDuration: null,
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
        djMessage: emptyMessage(),
        // Hex Message Modal state
        showPlayHexModal: false,
        hexMessageText: '',
        hexMessagePool: '',
        hexSaving: false,
        hexMessage: emptyMessage(),
        // Channel Status Modal state
        showChannelStatusModal: false,
        channelStatusSetting: 'background',
        channelStatusText: '',
        channelStatusDuration: null,
        channelStatusSaving: false,
        channelStatusMessage: emptyMessage(),

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
                failure: 'Failed to advance DJ state',
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
                        body: {
                            message: this.hexMessageText,
                            ...(this.hexMessagePool && { pool: this.hexMessagePool }),
                        },
                    }),
                success: 'Hex message queued successfully!',
                failure: 'Failed to play hex message',
                afterFn: () => this.closeModal('playHex'),
                afterMs: 1500,
            });
        },

        async changeChannelStatus() {
            const status = this.channelStatusText || null;
            const body = {
                background: { tier: 'manual', status: status && { fixed: status } },
                override: { tier: 'override', status: status && { fixed: status } },
                unmanaged: { tier: 'override', status: 'unmanaged' },
            }[this.channelStatusSetting];
            if (body.status && this.channelStatusDuration !== null && this.channelStatusDuration !== '') {
                body.duration = Number.parseFloat(this.channelStatusDuration);
            }

            await this.runAction({
                saving: 'channelStatusSaving',
                message: 'channelStatusMessage',
                label: 'changing channel status',
                request: () =>
                    apiRequest(`/api/guilds/${this.currentGuildId}/dj/channel-status`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Channel status changed successfully!',
                failure: 'Failed to change channel status',
                afterFn: () => this.closeModal('channelStatus'),
                afterMs: 1500,
            });
        },
    };
}
