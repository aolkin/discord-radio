import { apiRequest } from './api.js';

export const logTypes = [
    { id: 'members', label: '👥 Members' },
    { id: 'dj', label: '🎧 DJ' },
];

const memberEmoji = { joined: '➕', left: '➖', moved: '↔️' };
const djEmoji = { playing_track: '🎵', playing_hex_message: '📡', playing_noise: '🔊', stopped: '⏹️' };

export function logs() {
    return {
        logTypes,
        logLimit: 50,
        logResult: null,
        logLoading: false,
        logError: '',

        async loadLogs(offset = null, direction = 'backward') {
            const { name: guildId, logType } = this.route;
            const limit = Math.min(500, Math.max(10, Number.parseInt(this.logLimit, 10) || 50));
            this.logLimit = limit;
            const params = new URLSearchParams({ limit, direction });
            if (offset !== null) {
                params.set('offset', offset);
            }
            this.logLoading = true;
            this.logError = '';
            try {
                const response = await apiRequest(`/api/guilds/${guildId}/logs/${logType}?${params}`, {
                    errorMessage: 'Failed to load logs',
                    includeStatus: true,
                });
                const result = await response.json();
                if (this.route.name === guildId && this.route.logType === logType) {
                    this.logResult = result;
                }
            } catch (e) {
                this.logResult = null;
                this.logError = `Error loading logs: ${e.message}`;
                console.error('Error loading logs:', e);
            } finally {
                this.logLoading = false;
            }
        },

        loadLatestLogs() {
            return this.loadLogs();
        },

        loadPreviousLogs() {
            if (this.logResult?.has_previous) {
                this.loadLogs(this.logResult.start_offset, 'backward');
            }
        },

        loadNextLogs() {
            if (this.logResult?.has_next) {
                this.loadLogs(this.logResult.end_offset, 'forward');
            }
        },

        logTime(content) {
            return new Date(content.timestamp).toLocaleString();
        },

        memberLogTitle(content) {
            const nickname = content.nickname ? ` (${content.nickname})` : '';
            return `${memberEmoji[content.action] || '•'} ${content.username}${nickname} ${content.action}`;
        },

        memberLogDetail(content) {
            const channel = content.channel_id ? ` | Channel ID: ${content.channel_id}` : '';
            return `User ID: ${content.user_id}${channel}`;
        },

        djLogTitle(content) {
            return `${djEmoji[content.event_type] || '•'} ${content.event_type.replace(/_/g, ' ').toUpperCase()}`;
        },

        djLogDetail(content) {
            const details = content.details;
            const profile = details.forced_profile ? ` | Profile: ${details.forced_profile}` : '';
            if (details.filename) {
                return `Track: ${details.filename} (${details.duration_secs}s)${profile}`;
            }
            if (details.message) {
                return `Message: "${details.message}" (${details.target_loops} loops)${profile}`;
            }
            if (details.noise_profile) {
                return `Profile: ${details.noise_profile} (${details.duration_secs}s)`;
            }
            if (details.duration_secs) {
                return `Duration: ${details.duration_secs}s`;
            }
            return '';
        },
    };
}
