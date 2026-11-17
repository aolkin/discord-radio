import { apiRequest } from './api.js';

export const modals = {
    activity: {
        flag: 'showActivityModal',
        reset: () => ({ newActivityStatus: '' }),
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
                label: 'performing activity action',
                request: () =>
                    apiRequest(endpoint, {
                        method: 'POST',
                        body: {
                            activity_type: activityType,
                            status,
                        },
                    }),
                onSuccess: () => {
                    if (statusField) {
                        this[statusField] = '';
                    }
                },
                success: successMessage,
                failure: 'Failed to perform action',
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
