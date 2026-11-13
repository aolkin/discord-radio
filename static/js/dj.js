import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const playForms = () => ({
    djStateType: '',
    djMessage: emptyMessage(),
    hexMessageText: '',
    hexMessagePool: '',
    hexMessage: emptyMessage(),
    channelStatusSetting: 'background',
    channelStatusText: '',
    channelStatusDuration: null,
    channelStatusMessage: emptyMessage(),
    announcementSelection: '',
    announcementMessage: emptyMessage(),
});

export function dj() {
    return {
        ...playForms(),
        djSaving: false,
        hexSaving: false,
        channelStatusSaving: false,
        announcementOptions: [],
        announcementLoading: false,
        announcementSaving: false,

        announcementChannelsOf(guildId) {
            return this.guilds.find((guild) => guild.guild_id === guildId)?.announcement_channels ?? [];
        },

        availableAnnouncementOptions() {
            const listed = this.announcementChannelsOf(this.route.name).map((channel) => channel.channel_id);
            return this.announcementOptions.filter((channel) => !listed.includes(channel.channel_id));
        },

        async loadAnnouncementOptions() {
            const guildId = this.route.name;
            this.announcementOptions = [];
            this.announcementLoading = true;
            try {
                const response = await apiRequest(`/api/guilds/${guildId}/channels`, {
                    errorMessage: bodyOrStatus,
                });
                const data = await response.json();
                if (guildId === this.route.name) {
                    this.announcementOptions = data.channels;
                }
            } catch (error) {
                console.error('Error loading channels:', error);
                this.announcementMessage = {
                    text: `Failed to load channels: ${error.message}`,
                    type: 'error',
                };
            } finally {
                this.announcementLoading = false;
            }
        },

        async addAnnouncementChannel() {
            await this.runAction({
                saving: 'announcementSaving',
                message: 'announcementMessage',
                label: 'adding announcement channel',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/announcement-channels`, {
                        method: 'POST',
                        body: { channel_id: this.announcementSelection },
                        errorMessage: bodyOrStatus,
                    }),
                success: 'Announcement channel added.',
                failure: 'Failed to add announcement channel',
                onSuccess: () => {
                    this.announcementSelection = '';
                },
            });
        },

        async removeAnnouncementChannel(channelId) {
            await this.runAction({
                saving: 'announcementSaving',
                message: 'announcementMessage',
                label: 'removing announcement channel',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/announcement-channels/${channelId}`, {
                        method: 'DELETE',
                        errorMessage: bodyOrStatus,
                    }),
                success: 'Announcement channel removed.',
                failure: 'Failed to remove announcement channel',
            });
        },

        async advanceDJState() {
            await this.runAction({
                saving: 'djSaving',
                message: 'djMessage',
                label: 'advancing DJ state',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/advance`, {
                        method: 'POST',
                        body: { state_type: this.djStateType || null },
                    }),
                success: 'DJ state advanced successfully!',
                failure: 'Failed to advance DJ state',
            });
        },

        async playHexMessage() {
            await this.runAction({
                saving: 'hexSaving',
                message: 'hexMessage',
                label: 'playing hex message',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/hex/play`, {
                        method: 'POST',
                        body: {
                            message: this.hexMessageText,
                            ...(this.hexMessagePool && { pool: this.hexMessagePool }),
                        },
                    }),
                success: 'Hex message queued successfully!',
                failure: 'Failed to play hex message',
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
                    apiRequest(`/api/guilds/${this.route.name}/dj/channel-status`, {
                        method: 'POST',
                        body,
                    }),
                success: 'Channel status changed successfully!',
                failure: 'Failed to change channel status',
            });
        },
    };
}
