export function actionMethods() {
    return {
        // Runs a save request with the shared flow: set `saving`, send,
        // then show a success or error toast.
        async runAction({ saving, request, success, failure, label, onSuccess }) {
            this[saving] = true;

            try {
                await request();
                if (onSuccess) {
                    await onSuccess();
                }
                this.showToast(success, 'success');
            } catch (error) {
                console.error(`Error ${label}:`, error);
                this.showToast(`${failure}: ${error.message}`, 'error');
            } finally {
                this[saving] = false;
            }
        },
    };
}
