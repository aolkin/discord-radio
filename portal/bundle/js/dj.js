import { apiRequest, bodyOrStatus } from './api.js';

export const playForms = () => ({
    djAdvancePool: '',
    djConfigSelection: '',
    idleSignalPreset: '',
    idleDuration: null,
    ttsMessageText: '',
    ttsMessagePool: '',
    ttsMessageVolume: null,
    ttsMessageLoops: null,
    channelStatusSetting: 'background',
    channelStatusText: '',
    channelStatusDuration: null,
    announcementSelection: '',
    voiceChannelSelection: '',
});

export function dj() {
    return {
        ...playForms(),
        djSaving: false,
        djConfigSaving: false,
        idleSaving: false,
        ttsSaving: false,
        channelStatusSaving: false,
        announcementOptions: [],
        announcementLoading: false,
        announcementSaving: false,
        voiceChannelOptions: [],
        voiceChannelLoading: false,
        voiceChannelSaving: false,
        voicePickerOpen: false,

        async loadVoiceChannelOptions() {
            const guildId = this.route.name;
            this.voiceChannelOptions = [];
            this.voiceChannelLoading = true;
            try {
                const response = await apiRequest(`/api/guilds/${guildId}/voice-channels`, {
                    errorMessage: bodyOrStatus,
                });
                const data = await response.json();
                if (guildId === this.route.name) {
                    this.voiceChannelOptions = data.channels;
                }
            } catch (error) {
                console.error('Error loading voice channels:', error);
                this.showToast(`Failed to load voice channels: ${error.message}`, 'error');
            } finally {
                this.voiceChannelLoading = false;
            }
        },

        async joinVoiceChannel() {
            await this.runAction({
                saving: 'voiceChannelSaving',
                label: 'joining voice channel',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/voice-channels/${this.voiceChannelSelection}/join`, {
                        method: 'POST',
                        errorMessage: bodyOrStatus,
                    }),
                success: 'Joined the voice channel.',
                failure: 'Failed to join the voice channel',
                onSuccess: () => {
                    this.voiceChannelSelection = '';
                    this.voicePickerOpen = false;
                },
            });
        },

        async leaveVoiceChannel() {
            await this.runAction({
                saving: 'voiceChannelSaving',
                label: 'leaving voice channel',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/voice-channels/leave`, {
                        method: 'POST',
                        errorMessage: bodyOrStatus,
                    }),
                success: 'Left the voice channel.',
                failure: 'Failed to leave the voice channel',
            });
        },

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
                this.showToast(`Failed to load channels: ${error.message}`, 'error');
            } finally {
                this.announcementLoading = false;
            }
        },

        async addAnnouncementChannel() {
            await this.runAction({
                saving: 'announcementSaving',
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
                label: 'advancing DJ state',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/advance`, {
                        method: 'POST',
                        body: { pool: this.djAdvancePool || null },
                        errorMessage: bodyOrStatus,
                    }),
                success: 'DJ state advanced successfully!',
                failure: 'Failed to advance DJ state',
            });
        },

        async idleDJ() {
            const body = { signal_preset: this.idleSignalPreset || null };
            if (this.idleDuration !== null && this.idleDuration !== '') {
                body.duration = Number.parseFloat(this.idleDuration);
            }

            await this.runAction({
                saving: 'idleSaving',
                label: 'making the DJ idle',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/idle`, {
                        method: 'POST',
                        body,
                        errorMessage: bodyOrStatus,
                    }),
                success: 'DJ is idle.',
                failure: 'Failed to make the DJ idle',
            });
        },

        async configureDJ() {
            const config = this.djConfigSelection;
            if (!confirm(`Switch the DJ to config "${config}"?`)) {
                return;
            }
            await this.runAction({
                saving: 'djConfigSaving',
                label: 'configuring DJ',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/dj/config`, {
                        method: 'POST',
                        body: { config },
                        errorMessage: bodyOrStatus,
                    }),
                success: `DJ switched to config "${config}".`,
                failure: 'Failed to configure DJ',
            });
        },

        async playTtsMessage() {
            const body = { message: this.ttsMessageText };
            if (this.ttsMessagePool) {
                body.pool = this.ttsMessagePool;
            }
            if (this.ttsMessageVolume !== null && this.ttsMessageVolume !== '') {
                body.volume = Number.parseFloat(this.ttsMessageVolume);
            }
            if (this.ttsMessageLoops !== null && this.ttsMessageLoops !== '') {
                body.loops = Number.parseInt(this.ttsMessageLoops, 10);
            }

            await this.runAction({
                saving: 'ttsSaving',
                label: 'playing TTS message',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/tts/play`, {
                        method: 'POST',
                        body,
                        errorMessage: bodyOrStatus,
                    }),
                success: 'TTS message queued successfully!',
                failure: 'Failed to play TTS message',
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
