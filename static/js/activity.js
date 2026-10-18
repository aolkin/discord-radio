import { apiRequest, emptyMessage } from './api.js';

export const modals = {
    activity: {
        flag: 'showActivityModal',
        perGuild: false,
        reset: () => ({ newActivityStatus: '', activityMessage: emptyMessage() }),
    },
};

export function activity() {
    return {
        // Activity Modal state
        showActivityModal: false,
        currentActivity: { activity_type: null, status: null },
        newActivityType: 'playing',
        newActivityStatus: '',
        activitySaving: false,
        activityMessage: { text: '', type: '' },

        getActivityIcon(activityType) {
            const icons = {
                playing: '🎮',
                listening: '🎵',
                streaming: '📡',
                custom: '✨',
            };
            return icons[activityType?.toLowerCase()] || '✨';
        },

        // Activity Management Functions
        async performActivityAction(endpoint, activityType, status, successMessage, statusField) {
            await this.runAction({
                saving: 'activitySaving',
                message: 'activityMessage',
                label: 'performing activity action',
                request: () =>
                    apiRequest(endpoint, {
                        method: 'POST',
                        body: {
                            activity_type: activityType,
                            status: status,
                        },
                    }),
                onSuccess: () => {
                    if (statusField) {
                        this[statusField] = '';
                    }
                },
                success: successMessage,
                failure: 'Failed to perform action. Please try again.',
                afterFn: () => {
                    this.activityMessage = { text: '', type: '' };
                },
                afterMs: 2000,
            });
        },

        async setActivity() {
            await this.performActivityAction(
                '/api/activity/set',
                this.newActivityType,
                this.newActivityStatus,
                'Activity set successfully!',
                'newActivityStatus',
            );
        },
    };
}
