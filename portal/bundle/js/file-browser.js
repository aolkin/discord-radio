import { apiRequest, bodyOrStatus } from './api.js';

export const modals = {
    fileBrowser: {
        flag: 'fileBrowser.open',
        reset: () => ({ fileBrowserOnSelect: null }),
    },
};

document.addEventListener('alpine:init', () => {
    Alpine.directive('include', (element, { expression }) => {
        const content = document.querySelector(expression).content.cloneNode(true);
        const children = [...content.children];
        element.append(content);
        for (const child of children) {
            Alpine.initTree(child);
        }
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
            newFolderName: '',
            creatingFolder: false,
            deleting: false,
            dragging: false,
            picked: [],
        },
        fileBrowserOnSelect: null,
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
            this.fileBrowser.picked = [];
            this.openModal('fileBrowser');
            this.fileBrowserOnSelect = onSelect;
            return this.loadFileBrowser(prefix);
        },

        openFileBrowserPage(path) {
            this.fileBrowser.newFolderName = '';
            return this.loadFileBrowser(path ? `${path}/` : '');
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
            return this.fileKinds[this.fileBrowser.prefix.split('/')[0]] ?? null;
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
                this.showToast(`Failed to load files: ${error.message}`, 'error');
            } finally {
                this.fileBrowser.loading = false;
            }
        },

        async createFileBrowserFolder() {
            this.fileBrowser.creatingFolder = true;
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
                this.showToast(`Failed to create folder: ${error.message}`, 'error');
            } finally {
                this.fileBrowser.creatingFolder = false;
            }
        },

        async deleteFileBrowserFile(key) {
            if (!confirm(`Delete ${key}?`)) {
                return;
            }
            this.fileBrowser.deleting = true;
            try {
                await apiRequest(`/api/files?key=${encodeURIComponent(key)}`, {
                    method: 'DELETE',
                    errorMessage: bodyOrStatus,
                });
                this.fileBrowser.picked = this.fileBrowser.picked.filter((item) => item !== key);
                await this.loadFileBrowser(this.fileBrowser.prefix);
            } catch (error) {
                console.error('Failed to delete file:', error);
                this.showToast(`Failed to delete file: ${error.message}`, 'error');
            } finally {
                this.fileBrowser.deleting = false;
            }
        },

        dropOnFileBrowser(event) {
            this.fileBrowser.dragging = false;
            const entries = Array.from(event.dataTransfer.items)
                .map((item) => item.webkitGetAsEntry())
                .filter(Boolean);
            return this.uploadEntries(entries, this.fileBrowser.prefix, this.fileBrowserKind());
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
