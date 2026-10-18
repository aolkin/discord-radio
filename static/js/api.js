// The response body text, or `HTTP <status>` when the body is empty.
// Pass as `errorMessage` to show what the server said.
export async function bodyOrStatus(response) {
    return (await response.text()) || `HTTP ${response.status}`;
}

// Sends a request and returns the response. Non-2xx responses and
// network failures throw an Error carrying `status` (when a response
// arrived). `body` is sent as JSON. `errorMessage` is the Error
// message: a string, or a function of the response returning one.
// With `includeStatus`, the message ends with ` (<status>: <statusText>)`.
export async function apiRequest(url, { method = 'GET', body, errorMessage = 'Request failed', includeStatus = false } = {}) {
    const options = { method };
    if (body !== undefined) {
        options.headers = { 'Content-Type': 'application/json' };
        options.body = JSON.stringify(body);
    }
    const response = await fetch(url, options);
    if (!response.ok) {
        const message = typeof errorMessage === 'function' ? await errorMessage(response) : errorMessage;
        const detail = includeStatus
            ? ` (${response.status}: ${response.statusText || 'Unknown error'})`
            : '';
        const error = new Error(message + detail);
        error.status = response.status;
        throw error;
    }
    return response;
}

export const emptyMessage = () => ({ text: '', type: '' });
