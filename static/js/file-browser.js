import { apiRequest, bodyOrStatus } from './api.js';

export const modals = {
    fileBrowser: {
        flag: 'fileBrowser.open',
        perGuild: false,
        reset: () => ({ fileBrowserOnSelect: null }),
    },
};

export function fileBrowser() {
    return {
        fileBrowser: {
            open: false,
            mode: 'browse',
            prefix: 'tracks/',
            folders: [],
            files: [],
            loading: false,
            message: { text: '', type: '' },
            newFolderName: '',
            creatingFolder: false,
            dragging: false,
            uploads: [],
        },
        fileBrowserOnSelect: null,
        nextUploadId: 0,
        fileKinds: {
            tracks: { noun: 'Audio File', extensions: '.mp3, .ogg and .wav', hint: '.mp3, .ogg, .wav at 48kHz', accept: '.mp3,.ogg,.wav', pattern: /\.(mp3|ogg|wav)$/i },
            images: { noun: 'Image', extensions: '.png, .jpg and .gif', hint: '.png, .jpg, .gif', accept: '.png,.jpg,.jpeg,.gif', pattern: /\.(png|jpe?g|gif)$/i },
        },

        // mode is 'browse' (no selection), 'file' or 'folder'. A picking
        // mode calls onSelect with the chosen object key or folder prefix.
        openFileBrowser({ mode = 'browse', prefix = 'tracks/', onSelect = null } = {}) {
            this.fileBrowser.mode = mode;
            this.fileBrowser.newFolderName = '';
            this.fileBrowser.uploads = [];
            this.openModal('fileBrowser');
            this.fileBrowserOnSelect = onSelect;
            return this.loadFileBrowser(prefix);
        },

        selectFileBrowserEntry(value) {
            const onSelect = this.fileBrowserOnSelect;
            this.closeModal('fileBrowser');
            if (onSelect) onSelect(value);
        },

        fileBrowserKind() {
            return this.fileKinds[this.fileBrowser.prefix.split('/')[0]] || this.fileKinds.tracks;
        },

        fileBrowserCrumbs() {
            const names = this.fileBrowser.prefix.split('/').filter(Boolean);
            return names.map((name, index) => ({
                name,
                prefix: names.slice(0, index + 1).join('/') + '/',
            }));
        },

        async loadFileBrowser(prefix) {
            this.fileBrowser.loading = true;
            this.fileBrowser.message = { text: '', type: '' };
            try {
                const response = await apiRequest(`/api/files?prefix=${encodeURIComponent(prefix)}`, {
                    errorMessage: bodyOrStatus,
                });
                const listing = await response.json();
                this.fileBrowser.prefix = listing.prefix;
                this.fileBrowser.folders = listing.folders;
                this.fileBrowser.files = listing.files;
            } catch (error) {
                console.error('Failed to load files:', error);
                this.fileBrowser.message = { text: `Failed to load files: ${error.message}`, type: 'error' };
            } finally {
                this.fileBrowser.loading = false;
            }
        },

        async createFileBrowserFolder() {
            this.fileBrowser.creatingFolder = true;
            this.fileBrowser.message = { text: '', type: '' };
            try {
                await apiRequest('/api/files/folders', {
                    method: 'POST',
                    body: { prefix: this.fileBrowser.prefix, name: this.fileBrowser.newFolderName },
                    errorMessage: bodyOrStatus,
                });
                this.fileBrowser.newFolderName = '';
                await this.loadFileBrowser(this.fileBrowser.prefix);
            } catch (error) {
                console.error('Failed to create folder:', error);
                this.fileBrowser.message = { text: `Failed to create folder: ${error.message}`, type: 'error' };
            } finally {
                this.fileBrowser.creatingFolder = false;
            }
        },

        async uploadFiles(fileList) {
            const prefix = this.fileBrowser.prefix;
            const kind = this.fileBrowserKind();
            let uploaded = false;
            for (const file of Array.from(fileList)) {
                this.fileBrowser.uploads.push({
                    id: this.nextUploadId++, name: file.name, progress: 0, status: 'uploading', detail: '',
                });
                const upload = this.fileBrowser.uploads.at(-1);
                if (!kind.pattern.test(file.name)) {
                    upload.status = 'error';
                    upload.detail = `Only ${kind.extensions} files are accepted`;
                    continue;
                }
                try {
                    upload.detail = await this.sendFile(file, prefix, (percent) => { upload.progress = percent; });
                    upload.status = 'done';
                    uploaded = true;
                } catch (error) {
                    console.error('Failed to upload file:', error);
                    upload.status = 'error';
                    upload.detail = error.message;
                }
            }
            if (uploaded && this.fileBrowser.prefix === prefix) {
                await this.loadFileBrowser(prefix);
            }
        },

        // Resolves with the uploaded key. XMLHttpRequest rather than fetch because fetch reports no upload progress.
        sendFile(file, prefix, onProgress) {
            return new Promise((resolve, reject) => {
                const xhr = new XMLHttpRequest();
                xhr.open('POST', `/api/files/upload?prefix=${encodeURIComponent(prefix)}`);
                xhr.upload.onprogress = (event) => {
                    if (event.lengthComputable) onProgress(Math.round((event.loaded / event.total) * 100));
                };
                xhr.onload = () => {
                    if (xhr.status >= 200 && xhr.status < 300) {
                        resolve(JSON.parse(xhr.responseText).key);
                    } else {
                        reject(new Error(xhr.responseText || `HTTP ${xhr.status}`));
                    }
                };
                xhr.onerror = () => reject(new Error('Network error'));
                const form = new FormData();
                form.append('file', file);
                xhr.send(form);
            });
        },

        formatBytes(bytes) {
            const units = ['B', 'KB', 'MB', 'GB'];
            let value = bytes;
            let unit = 0;
            while (value >= 1024 && unit < units.length - 1) {
                value /= 1024;
                unit++;
            }
            return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`;
        },

        formatDate(value) {
            const date = new Date(value);
            return isNaN(date) ? value : date.toLocaleString();
        },
    };
}
