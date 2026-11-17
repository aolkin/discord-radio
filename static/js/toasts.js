const MAX_TOASTS = 5;
const SUCCESS_MS = 5000;

export function toasts() {
    return {
        toasts: [],
        nextToastId: 1,

        showToast(text, type) {
            const id = this.nextToastId++;
            this.toasts.push({ id, text, type });
            this.toasts.splice(0, this.toasts.length - MAX_TOASTS);
            if (type === 'success') {
                setTimeout(() => this.dismissToast(id), SUCCESS_MS);
            }
        },

        dismissToast(id) {
            this.toasts = this.toasts.filter((toast) => toast.id !== id);
        },
    };
}
