import { emptyMessage } from './api.js';

export function actionMethods() {
    return {
        // Runs a save request with the shared flow: set `saving`, clear
        // `message`, send, then show a success or error message. After a
        // success, `afterFn` runs once `afterMs` has elapsed.
        async runAction({ saving, message, request, success, failure, label, onSuccess, afterFn, afterMs }) {
            this[saving] = true;
            this[message] = emptyMessage();

            try {
                await request();
                if (onSuccess) {
                    await onSuccess();
                }
                this[message] = { text: success, type: 'success' };
                if (afterFn) {
                    setTimeout(afterFn, afterMs);
                }
            } catch (error) {
                console.error(`Error ${label}:`, error);
                this[message] = { text: `${failure}: ${error.message}`, type: 'error' };
            } finally {
                this[saving] = false;
            }
        },
    };
}
