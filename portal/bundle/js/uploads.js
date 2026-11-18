import { apiRequest, bodyOrStatus } from './api.js';

const CONCURRENCY = 3;

const readEntries = async (directory) => {
    const reader = directory.createReader();
    const entries = [];
    for (;;) {
        const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
        if (batch.length === 0) {
            return entries;
        }
        entries.push(...batch);
    }
};

const isActive = (item) => item.status === 'queued' || item.status === 'uploading';

export function uploads() {
    return {
        uploadQueue: {
            items: [],
            nextId: 0,
            refreshPending: false,
        },

        uploadItem(id) {
            return this.uploadQueue.items.find((item) => item.id === id);
        },

        addUpload(fields) {
            const queue = this.uploadQueue;
            queue.items.push({
                id: queue.nextId++,
                type: 'file',
                after: null,
                progress: 0,
                status: 'queued',
                detail: '',
                retryable: true,
                blocked: false,
                ...fields,
            });
            return queue.items.at(-1);
        },

        queueFile(file, prefix, kind, parent) {
            const item = this.addUpload({
                path: `${prefix}${file.name}`,
                prefix,
                file,
                after: parent?.id ?? null,
            });
            if (!kind.pattern.test(file.name)) {
                item.status = 'error';
                item.detail = `Unsupported file type (only ${kind.extensions} are accepted)`;
                item.retryable = false;
            }
        },

        async queueEntry(entry, prefix, kind, parent) {
            if (entry.isFile) {
                const file = await new Promise((resolve, reject) => entry.file(resolve, reject));
                this.queueFile(file, prefix, kind, parent);
                return;
            }
            const folder = this.addUpload({
                type: 'folder',
                path: `${prefix}${entry.name}/`,
                prefix,
                name: entry.name,
                after: parent?.id ?? null,
            });
            for (const child of await readEntries(entry)) {
                await this.queueEntry(child, folder.path, kind, folder);
            }
        },

        async uploadEntries(entries, prefix, kind) {
            if (!kind) {
                for (const entry of entries) {
                    this.addUpload({
                        path: entry.name,
                        status: 'error',
                        detail: 'Uploads are only allowed inside the tracks and images folders',
                        retryable: false,
                    });
                }
                return;
            }
            for (const entry of entries) {
                await this.queueEntry(entry, prefix, kind, null);
            }
            this.runUploads();
        },

        uploadFiles(fileList) {
            const prefix = this.fileBrowser.prefix;
            const kind = this.fileBrowserKind();
            for (const file of Array.from(fileList)) {
                this.queueFile(file, prefix, kind, null);
            }
            this.runUploads();
        },

        runUploads() {
            const items = this.uploadQueue.items;
            let active = items.filter((item) => item.status === 'uploading').length;
            for (const item of items) {
                if (item.status !== 'queued') {
                    continue;
                }
                const parent = item.after === null ? undefined : this.uploadItem(item.after);
                if (parent?.status === 'error') {
                    item.status = 'error';
                    item.detail = 'The containing folder was not created';
                    item.blocked = true;
                    continue;
                }
                if (parent && parent.status !== 'done') {
                    continue;
                }
                if (active >= CONCURRENCY) {
                    break;
                }
                active++;
                this.startUpload(item);
            }
            if (active === 0 && !items.some((item) => item.status === 'queued')) {
                this.finishUploads();
            }
        },

        async startUpload(item) {
            item.status = 'uploading';
            item.progress = 0;
            try {
                if (item.type === 'folder') {
                    await apiRequest('/api/files/folders', {
                        method: 'POST',
                        body: { prefix: item.prefix, name: item.name },
                        errorMessage: bodyOrStatus,
                    });
                } else {
                    item.detail = await this.sendFile(item.file, item.prefix, (percent) => {
                        item.progress = percent;
                    });
                    if (this.fileBrowser.open && this.fileBrowser.mode === 'files') {
                        this.fileBrowser.picked = [...this.fileBrowser.picked, item.detail];
                    }
                }
                item.progress = 100;
                item.status = 'done';
                this.uploadQueue.refreshPending = true;
            } catch (error) {
                console.error('Failed to upload:', error);
                item.status = 'error';
                item.detail = error.message;
            }
            this.runUploads();
        },

        finishUploads() {
            const queue = this.uploadQueue;
            if (!queue.refreshPending) {
                return;
            }
            queue.refreshPending = false;
            if (this.fileBrowser.open || this.route.kind === 'files') {
                this.loadFileBrowser(this.fileBrowser.prefix);
            }
        },

        retryUpload(item) {
            item.status = 'queued';
            item.progress = 0;
            item.detail = '';
            item.blocked = false;
            for (const child of this.uploadQueue.items) {
                if (child.after === item.id && child.blocked) {
                    this.retryUpload(child);
                }
            }
            this.runUploads();
        },

        dismissUpload(item) {
            this.uploadQueue.items = this.uploadQueue.items.filter((other) => other.id !== item.id);
        },

        dismissFinishedUploads() {
            this.uploadQueue.items = this.uploadQueue.items.filter(isActive);
        },

        uploadsActive() {
            return this.uploadQueue.items.some(isActive);
        },

        uploadsFinished() {
            return this.uploadQueue.items.some((item) => !isActive(item));
        },

        uploadsOverallPercent() {
            const items = this.uploadQueue.items;
            if (items.length === 0) {
                return 0;
            }
            const total = items.reduce((sum, item) => sum + (isActive(item) ? item.progress : 100), 0);
            return Math.round(total / items.length);
        },

        uploadsSummary() {
            const items = this.uploadQueue.items;
            const done = items.filter((item) => item.status === 'done').length;
            const failed = items.filter((item) => item.status === 'error').length;
            const failedText = failed > 0 ? `, ${failed} failed` : '';
            if (this.uploadsActive()) {
                return `Uploading ${done + failed} of ${items.length}${failedText}`;
            }
            return `${done} uploaded${failedText}`;
        },
    };
}
