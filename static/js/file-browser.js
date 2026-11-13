import { apiRequest, bodyOrStatus, emptyMessage } from './api.js';

export const modals = {
    fileBrowser: {
        flag: 'fileBrowser.open',
        reset: () => ({ fileBrowserOnSelect: null }),
    },
};

document.addEventListener('alpine:init', () => {
    Alpine.directive('include', (element, { expression }) => {
        element.append(document.querySelector(expression).content.cloneNode(true));
    });
});

export function fileBrowser() {
    return {
        fileBrowser: {
            open: false,
            mode: null,
            prefix: 'tracks/',
            folders: [],
            files: [],
            loading: false,
            message: emptyMessage(),
            newFolderName: '',
            creatingFolder: false,
            dragging: false,
            uploads: [],
            picked: [],
        },
        fileBrowserOnSelect: null,
        nextUploadId: 0,
        fileKinds: {
            tracks: {
                noun: 'Audio File',
                extensions: '.mp3, .ogg and .wav',
                hint: '.mp3, .ogg, .wav',
                accept: '.mp3,.ogg,.wav',
                pattern: /\.(mp3|ogg|wav)$/i,
            },
            images: {
                noun: 'Image',
                extensions: '.png, .jpg and .gif',
                hint: '.png, .jpg, .gif',
                accept: '.png,.jpg,.jpeg,.gif',
                pattern: /\.(png|jpe?g|gif)$/i,
            },
        },

        openFileBrowser({ mode, prefix, onSelect }) {
            this.fileBrowser.mode = mode;
            this.fileBrowser.newFolderName = '';
            this.fileBrowser.uploads = [];
            this.fileBrowser.picked = [];
            this.openModal('fileBrowser');
            this.fileBrowserOnSelect = onSelect;
            return this.loadFileBrowser(prefix);
        },

        openFileBrowserPage(path) {
            this.fileBrowser.newFolderName = '';
            this.fileBrowser.uploads = [];
            return this.loadFileBrowser(`${path}/`);
        },

        browseFileBrowser(prefix) {
            if (this.fileBrowser.open) {
                return this.loadFileBrowser(prefix);
            }
            return this.openItem('files', prefix.replace(/\/$/, ''));
        },

        selectFileBrowserEntry(value) {
            const onSelect = this.fileBrowserOnSelect;
            this.closeModal('fileBrowser');
            if (onSelect) {
                onSelect(value);
            }
        },

        toggleFileBrowserPicked(key) {
            const picked = this.fileBrowser.picked;
            this.fileBrowser.picked = picked.includes(key) ? picked.filter((item) => item !== key) : [...picked, key];
        },

        fileBrowserKind() {
            return this.fileKinds[this.fileBrowser.prefix.split('/')[0]] || this.fileKinds.tracks;
        },

        fileBrowserCrumbs() {
            const names = this.fileBrowser.prefix.split('/').filter(Boolean);
            return names.map((name, index) => ({
                name,
                prefix: `${names.slice(0, index + 1).join('/')}/`,
            }));
        },

        async loadFileBrowser(prefix) {
            this.fileBrowser.loading = true;
            this.fileBrowser.message = emptyMessage();
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
            this.fileBrowser.message = emptyMessage();
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
                    id: this.nextUploadId++,
                    name: file.name,
                    progress: 0,
                    status: 'uploading',
                    detail: '',
                });
                const upload = this.fileBrowser.uploads.at(-1);
                if (!kind.pattern.test(file.name)) {
                    upload.status = 'error';
                    upload.detail = `Only ${kind.extensions} files are accepted`;
                    continue;
                }
                try {
                    upload.detail = await this.sendFile(file, prefix, (percent) => {
                        upload.progress = percent;
                    });
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
                    if (event.lengthComputable) {
                        onProgress(Math.round((event.loaded / event.total) * 100));
                    }
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
            return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
        },
    };
}
