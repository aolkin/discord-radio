import { apiRequest, bodyOrStatus } from './api.js';

export const logTypes = [
    { id: 'members', icon: '👥', label: 'Members' },
    { id: 'dj', icon: '🎧', label: 'DJ' },
];

const memberEmoji = { joined: '➕', left: '➖', moved: '↔️' };
const djEmoji = { track: '🎵', hex_message: '📡', noise: '🔊', stopped: '⏹️' };

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
                    errorMessage: bodyOrStatus,
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

        logIcon(content) {
            const emoji = this.route.logType === 'dj' ? djEmoji[content.event_type] : memberEmoji[content.action];
            return emoji || '•';
        },

        memberLogTitle(content) {
            const nickname = content.nickname ? ` (${content.nickname})` : '';
            return `${content.username}${nickname} ${content.action}`;
        },

        memberLogDetail(content) {
            const channel = content.channel_id ? ` | Channel ID: ${content.channel_id}` : '';
            return `User ID: ${content.user_id}${channel}`;
        },

        djLogTitle(content) {
            const label = content.event_type.replace(/_/g, ' ');
            return `${label.charAt(0).toUpperCase()}${label.slice(1)}`;
        },

        djLogDetail(content) {
            const details = content.details;
            const preset = details.signal_preset ? ` | Preset: ${details.signal_preset}` : '';
            if (details.filename) {
                return `Track: ${details.filename} (${details.duration_secs}s)${preset}`;
            }
            if (details.message) {
                return `Message: "${details.message}" (${details.target_loops} loops)${preset}`;
            }
            if (details.signal_preset) {
                return `Preset: ${details.signal_preset} (${details.duration_secs}s)`;
            }
            if (details.duration_secs) {
                return `Duration: ${details.duration_secs}s`;
            }
            return '';
        },
    };
}
