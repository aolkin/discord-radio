        // The response body text, or `HTTP <status>` when the body is empty.
        // Pass as `errorMessage` to show what the server said.
        async function bodyOrStatus(response) {
            return (await response.text()) || `HTTP ${response.status}`;
        }

        // Sends a request and returns the response. Non-2xx responses and
        // network failures throw an Error carrying `status` (when a response
        // arrived). `body` is sent as JSON. `errorMessage` is the Error
        // message: a string, or a function of the response returning one.
        // With `includeStatus`, the message ends with ` (<status>: <statusText>)`.
        async function apiRequest(url, { method = 'GET', body, errorMessage = 'Request failed', includeStatus = false } = {}) {
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

        const emptyMessage = () => ({ text: '', type: '' });

        // State behind each `<template x-modal data-modal="name">` in
        // index.html: `flag` is the visibility property, `perGuild` makes
        // openModal/closeModal set and clear `currentGuildId`, and `reset`
        // returns the field values restored on every open and close.
        const modals = {
            advanceDJ: {
                flag: 'showAdvanceDJModal',
                perGuild: true,
                reset: () => ({ djStateType: '', djMessage: emptyMessage() }),
            },
            playHex: {
                flag: 'showPlayHexModal',
                perGuild: true,
                reset: () => ({ hexMessageText: '', hexMessage: emptyMessage() }),
            },
            playTrack: {
                flag: 'showPlayTrackModal',
                perGuild: true,
                reset: () => ({
                    playTrackFilename: '',
                    playTrackVolume: null,
                    playTrackStart: null,
                    playTrackDuration: null,
                    playTrackMessage: emptyMessage(),
                }),
            },
            botProfile: {
                flag: 'showBotProfileModal',
                perGuild: true,
                reset: () => ({
                    botProfileMessage: emptyMessage(),
                    botProfileForm: { nick: '', avatar: '', banner: '' },
                }),
            },
            activity: {
                flag: 'showActivityModal',
                perGuild: false,
                reset: () => ({ newActivityStatus: '', activityMessage: emptyMessage() }),
            },
        };

        // `<template x-modal data-modal="name" ...>` renders the shared modal
        // shell (overlay, header, close button, message area, and for modals
        // with data-submit a form with a Cancel/Submit row) and puts the
        // template's content in the body, or in the form when there is one.
        // Open state comes from `modals[name]`; the other settings are
        // data attributes:
        //   data-title         header text
        //   data-message       property holding the { text, type } message
        //   data-submit        expression run on form submit
        //   data-saving        property that is true while submitting
        //   data-submit-label  Submit button text
        //   data-busy-label    Submit button text while saving
        //   data-cancel-label  Cancel button text (default "Cancel")
        //   data-loading       property; while true, shows "Loading..." and
        //                      hides the form
        //   data-escape-unless expression; Escape does not close while true
        document.addEventListener('alpine:init', () => {
            Alpine.directive('modal', (template) => {
                const config = template.dataset;
                const { flag } = modals[config.modal];
                const close = `closeModal(${JSON.stringify(config.modal)})`;

                const element = (tag, attributes = {}, className = '') => {
                    const node = document.createElement(tag);
                    if (className) node.className = className;
                    for (const [name, value] of Object.entries(attributes)) {
                        node.setAttribute(name, value);
                    }
                    return node;
                };

                const escapeHandler = config.escapeUnless ? `!(${config.escapeUnless}) && ${close}` : close;
                const overlay = element('div', {
                    'x-show': flag,
                    'x-transition.opacity': '',
                    'x-on:click.self': close,
                    'x-on:keydown.escape.window': escapeHandler,
                }, 'modal-overlay');
                const content = element('div', { 'x-show': flag, 'x-transition': '' }, 'modal-content');
                overlay.append(content);

                const header = element('div', {}, 'modal-header');
                const title = element('div', {}, 'modal-title');
                title.textContent = config.title;
                const closeButton = element('button', { 'x-on:click': close }, 'close');
                closeButton.innerHTML = '&times;';
                header.append(title, closeButton);

                const body = element('div', {}, 'modal-body');
                body.append(element('div', {
                    'x-show': `${config.message}.text`,
                    ':class': `${config.message}.type === 'error' ? 'error-message' : 'success-message'`,
                    'x-text': `${config.message}.text`,
                }));
                content.append(header, body);

                if (config.loading) {
                    const loading = element('div', { 'x-show': config.loading }, 'loading');
                    loading.textContent = 'Loading...';
                    body.append(loading);
                }
                if (config.submit) {
                    const form = element('form', { 'x-on:submit.prevent': config.submit });
                    if (config.loading) form.setAttribute('x-show', `!${config.loading}`);
                    const cancel = element('button', { type: 'button', 'x-on:click': close }, 'btn btn-secondary');
                    cancel.textContent = config.cancelLabel || 'Cancel';
                    const submit = element('button', { type: 'submit', ':disabled': config.saving }, 'btn btn-primary');
                    submit.append(element('span', {
                        'x-text': `${config.saving} ? ${JSON.stringify(config.busyLabel)} : ${JSON.stringify(config.submitLabel)}`,
                    }));
                    const buttons = element('div', {}, 'button-group');
                    buttons.append(cancel, submit);
                    form.append(template.content.cloneNode(true), buttons);
                    body.append(form);
                } else {
                    body.append(template.content.cloneNode(true));
                }

                template.after(overlay);
                Alpine.initTree(overlay);
            });
        });

        function botPortal() {
            return {
                ws: null,
                connected: false,
                statusText: 'Connecting...',
                guilds: [],
                version: '',
                commitHash: '',
                lastCommitHash: null,
                reconnectTimeout: null,
                currentTime: Date.now(),
                // DJ Modal state
                showAdvanceDJModal: false,
                djStateType: '',
                djSaving: false,
                djMessage: { text: '', type: '' },
                // Hex Message Modal state
                showPlayHexModal: false,
                hexMessageText: '',
                hexSaving: false,
                hexMessage: { text: '', type: '' },
                // Play Track Modal state
                showPlayTrackModal: false,
                playTrackFilename: '',
                playTrackVolume: null,
                playTrackStart: null,
                playTrackDuration: null,
                playTrackSaving: false,
                playTrackMessage: { text: '', type: '' },
                // Track State Modal state
                showTrackStateModal: false,
                trackName: '',
                trackState: '',
                trackFilename: '',
                trackVolume: null,
                trackLoops: '',
                trackFadeTime: null,
                trackSaving: false,
                trackMessage: { text: '', type: '' },
                audioFiles: [],
                // Signal Profile Modal state
                showSignalProfileModal: false,
                selectedProfile: '',
                profileSetting: 'profile',
                profileFadeDuration: null,
                profileSaving: false,
                profileMessage: { text: '', type: '' },
                profiles: [],
                fileBrowser: {
                    open: false,
                    mode: 'browse',
                    onSelect: null,
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
                nextUploadId: 0,
                fileKinds: {
                    tracks: { noun: 'Audio File', extensions: '.mp3, .ogg and .wav', hint: '.mp3, .ogg, .wav at 48kHz', accept: '.mp3,.ogg,.wav', pattern: /\.(mp3|ogg|wav)$/i },
                    images: { noun: 'Image', extensions: '.png, .jpg and .gif', hint: '.png, .jpg, .gif', accept: '.png,.jpg,.jpeg,.gif', pattern: /\.(png|jpe?g|gif)$/i },
                },
                // Bot Profile Modal state
                showBotProfileModal: false,
                botProfileLoading: false,
                botProfileSaving: false,
                botProfileMessage: { text: '', type: '' },
                botProfileCurrent: { nick: null, avatar_url: null, banner_url: null },
                botProfileForm: { nick: '', avatar: '', banner: '' },
                // Signal Profile Editor state
                showProfileEditor: false,
                profileEditorIsNew: false,
                profileEditorLoading: false,
                profileEditorSaving: false,
                profileEditorMessage: { text: '', type: '' },
                profileForm: { dropout_duration_ms: [0, 0] },
                profileFields: [
                    { key: 'bandpass_low', label: 'Bandpass Low (Hz)', step: 'any', min: 0 },
                    { key: 'bandpass_high', label: 'Bandpass High (Hz)', step: 'any', min: 0 },
                    { key: 'white_noise_level', label: 'White Noise Level (0-2)', step: 'any', min: 0, max: 2 },
                    { key: 'pink_noise_level', label: 'Pink Noise Level (0-2)', step: 'any', min: 0, max: 2 },
                    { key: 'brown_noise_level', label: 'Brown Noise Level (0-2)', step: 'any', min: 0, max: 2 },
                    { key: 'tremolo_depth', label: 'Tremolo Depth', step: 'any' },
                    { key: 'tremolo_rate', label: 'Tremolo Rate (Hz)', step: 'any' },
                    { key: 'tremolo_jitter', label: 'Tremolo Jitter', step: 'any' },
                    { key: 'clip_pregain', label: 'Clip Pregain', step: 'any' },
                    { key: 'clip_threshold', label: 'Clip Threshold', step: 'any' },
                    { key: 'bitcrush_bits', label: 'Bitcrush Bits', step: 1, min: 1, max: 255, optional: true },
                    { key: 'dropout_probability', label: 'Dropout Probability', step: 'any', min: 0 },
                    { key: 'frequency_warble_hz', label: 'Frequency Warble (Hz)', step: 'any', optional: true },
                ],
                // Activity Modal state
                showActivityModal: false,
                currentActivity: { activity_type: null, status: null },
                newActivityType: 'playing',
                newActivityStatus: '',
                activitySaving: false,
                activityMessage: { text: '', type: '' },
                // DJ Config state
                showDJConfigModal: false,
                djConfigLoading: false,
                djConfigSaving: false,
                djConfigError: null,
                djConfigMessage: { text: '', type: '' },
                djConfigData: { 
                    hex_messages: { enabled: false, items: [] }, 
                    hex_message_announcements: { enabled: false, items: [] },
                    state_weights: { enabled: false, value: { track: 100, hex_message: 2, noise: 50 } }
                },
                defaultStateWeights: { track: 100, hex_message: 2, noise: 50 },
                showHexMessageForm: false,
                hexMessageFormData: { index: null, text: '', weight: 1, signal_profile: '', loop_min: null, loop_max: null, announcement: '' },
                showAnnouncementForm: false,
                announcementFormData: { index: null, text: '' },
                // Channel Message state
                showChannelMessageModal: false,
                registeredChannels: [],
                channelsLoading: false,
                selectedChannelId: '',
                channelMessageText: '',
                channelMessageTitle: '',
                channelMessageDescription: '',
                channelMessageSaving: false,
                channelMessageError: '',
                channelMessageSuccess: '',

                getTrackProgress(track) {
                    if (!track.duration_secs || track.loops) return 0;
                    const elapsedMs = this.currentTime - (track.start_time.secs_since_epoch * 1000 + track.start_time.nanos_since_epoch / 1000000);
                    const elapsedSecs = elapsedMs / 1000;
                    const progress = Math.min((elapsedSecs / track.duration_secs) * 100, 100);
                    return Math.max(0, progress);
                },

                getTrackTimeDisplay(track) {
                    if (!track.duration_secs) return '';
                    const elapsedMs = this.currentTime - (track.start_time.secs_since_epoch * 1000 + track.start_time.nanos_since_epoch / 1000000);
                    const elapsedSecs = Math.max(0, Math.min(elapsedMs / 1000, track.duration_secs));
                    return this.formatTime(elapsedSecs) + ' / ' + this.formatTime(track.duration_secs);
                },

                formatTime(seconds) {
                    const mins = Math.floor(seconds / 60);
                    const secs = Math.floor(seconds % 60);
                    return mins + ':' + secs.toString().padStart(2, '0');
                },

                getActivityIcon(activityType) {
                    const icons = {
                        'playing': '🎮',
                        'listening': '🎵',
                        'streaming': '📡',
                        'custom': '✨'
                    };
                    return icons[activityType?.toLowerCase()] || '✨';
                },

                init() {
                    this.connect();
                    // Load audio files and profiles
                    this.loadAudioFiles();
                    this.loadProfiles();
                    // Update current time every 500ms for smooth progress bars
                    setInterval(() => {
                        this.currentTime = Date.now();
                    }, 500);
                },

                // Runs a save request with the shared flow: set `saving`, clear
                // `message`, send, then show a success or error message. After a
                // success, `afterFn` runs once `afterMs` has elapsed.
                async runAction({ saving, message, request, success, failure, label, onSuccess, afterFn, afterMs }) {
                    this[saving] = true;
                    this[message] = { text: '', type: '' };

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
                        this[message] = { text: failure, type: 'error' };
                    } finally {
                        this[saving] = false;
                    }
                },

                async loadAudioFiles() {
                    try {
                        const response = await apiRequest('/api/files?prefix=tracks/');
                        const data = await response.json();
                        this.audioFiles = data.files.map(file => 's3://' + file.key);
                    } catch (error) {
                        console.error('Failed to load audio files:', error);
                    }
                },

                async loadProfiles() {
                    try {
                        const response = await apiRequest('/api/profiles');
                        const data = await response.json();
                        this.profiles = data.profiles.sort();
                    } catch (error) {
                        console.error('Failed to load profiles:', error);
                    }
                },

                // mode is 'browse' (no selection), 'file' or 'folder'. A picking
                // mode calls onSelect with the chosen object key or folder prefix.
                openFileBrowser({ mode = 'browse', prefix = 'tracks/', onSelect = null } = {}) {
                    this.fileBrowser.mode = mode;
                    this.fileBrowser.onSelect = onSelect;
                    this.fileBrowser.newFolderName = '';
                    this.fileBrowser.uploads = [];
                    this.fileBrowser.open = true;
                    return this.loadFileBrowser(prefix);
                },

                closeFileBrowser() {
                    this.fileBrowser.open = false;
                    this.fileBrowser.onSelect = null;
                },

                selectFileBrowserEntry(value) {
                    const onSelect = this.fileBrowser.onSelect;
                    this.closeFileBrowser();
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

                async openProfileEditor(name) {
                    this.profileEditorIsNew = name === null;
                    this.profileEditorMessage = { text: '', type: '' };
                    this.showProfileEditor = true;

                    if (this.profileEditorIsNew) {
                        this.profileForm = {
                            name: '',
                            bandpass_low: 20, bandpass_high: 20000,
                            white_noise_level: 0, pink_noise_level: 0, brown_noise_level: 0,
                            tremolo_depth: 0, tremolo_rate: 0, tremolo_jitter: 0,
                            clip_pregain: 1, clip_threshold: 1, bitcrush_bits: '',
                            dropout_probability: 0, dropout_duration_ms: [0, 0],
                            frequency_warble_hz: '',
                        };
                        return;
                    }

                    this.profileEditorLoading = true;
                    try {
                        const response = await apiRequest(`/api/profiles/${encodeURIComponent(name)}`, {
                            errorMessage: (response) => `HTTP ${response.status}`,
                        });
                        const profile = await response.json();
                        for (const field of this.profileFields) {
                            if (profile[field.key] === null) profile[field.key] = '';
                        }
                        this.profileForm = profile;
                    } catch (error) {
                        console.error('Failed to load signal profile:', error);
                        this.profileEditorMessage = { text: `Failed to load profile: ${error.message}`, type: 'error' };
                    } finally {
                        this.profileEditorLoading = false;
                    }
                },

                closeProfileEditor() {
                    this.showProfileEditor = false;
                    this.profileEditorMessage = { text: '', type: '' };
                },

                async saveProfile() {
                    const name = this.profileForm.name;
                    if (this.profileEditorIsNew && this.profiles.includes(name)) {
                        this.profileEditorMessage = { text: `A profile named "${name}" already exists`, type: 'error' };
                        return;
                    }

                    const { name: _, ...body } = this.profileForm;
                    for (const field of this.profileFields) {
                        if (field.optional && body[field.key] === '') body[field.key] = null;
                    }

                    this.profileEditorSaving = true;
                    this.profileEditorMessage = { text: '', type: '' };
                    try {
                        await apiRequest(`/api/profiles/${encodeURIComponent(name)}`, {
                            method: 'PUT',
                            body,
                            errorMessage: bodyOrStatus,
                        });
                        this.profileEditorIsNew = false;
                        this.profileEditorMessage = { text: 'Signal profile saved', type: 'success' };
                        await this.loadProfiles();
                    } catch (error) {
                        console.error('Failed to save signal profile:', error);
                        this.profileEditorMessage = { text: `Failed to save profile: ${error.message}`, type: 'error' };
                    } finally {
                        this.profileEditorSaving = false;
                    }
                },

                connect() {
                    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
                    const wsUrl = `${protocol}//${window.location.host}/ws`;

                    this.ws = new WebSocket(wsUrl);

                    this.ws.onopen = () => {
                        this.connected = true;
                        this.statusText = 'Connected';
                        if (this.reconnectTimeout) {
                            clearTimeout(this.reconnectTimeout);
                            this.reconnectTimeout = null;
                        }
                    };

                    this.ws.onmessage = (event) => {
                        try {
                            const data = JSON.parse(event.data);
                            this.guilds = data.guilds || [];
                            
                            // Update current activity from websocket data
                            if (data.current_activity) {
                                this.currentActivity = {
                                    activity_type: data.current_activity.activity_type,
                                    status: data.current_activity.status
                                };
                            } else {
                                this.currentActivity = { activity_type: null, status: null };
                            }
                            
                            // Update version info
                            if (data.version) {
                                this.version = data.version;
                            }
                            if (data.commit_hash) {
                                // Check if commit hash changed (page needs reload)
                                if (this.lastCommitHash && this.lastCommitHash !== data.commit_hash) {
                                    console.log('Commit hash changed, reloading page...');
                                    window.location.reload();
                                }
                                this.commitHash = data.commit_hash;
                                this.lastCommitHash = data.commit_hash;
                            }
                        } catch (e) {
                            console.error('Failed to parse message:', e);
                        }
                    };

                    this.ws.onerror = (error) => {
                        console.error('WebSocket error:', error);
                    };

                    this.ws.onclose = () => {
                        this.connected = false;
                        this.statusText = 'Disconnected - Reconnecting...';
                        this.reconnectTimeout = setTimeout(() => this.connect(), 3000);
                    };
                },
                // Modals named in `modals` (see above) open and close here.
                // Both reset the modal's fields.
                openModal(name, guildId) {
                    const modal = modals[name];
                    if (modal.perGuild) this.currentGuildId = guildId;
                    Object.assign(this, modal.reset());
                    this[modal.flag] = true;
                },

                closeModal(name) {
                    const modal = modals[name];
                    this[modal.flag] = false;
                    if (modal.perGuild) this.currentGuildId = null;
                    Object.assign(this, modal.reset());
                },

                openBotProfileModal(guildId) {
                    this.openModal('botProfile', guildId);
                    return this.loadBotProfile();
                },

                async advanceDJState() {
                    await this.runAction({
                        saving: 'djSaving',
                        message: 'djMessage',
                        label: 'advancing DJ state',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj/advance`, {
                            method: 'POST',
                            body: { state_type: this.djStateType || null },
                        }),
                        success: 'DJ state advanced successfully!',
                        failure: 'Failed to advance DJ state. Please try again.',
                        afterFn: () => this.closeModal('advanceDJ'),
                        afterMs: 1500,
                    });
                },

                // Hex Message Modal Functions
                async playHexMessage() {
                    await this.runAction({
                        saving: 'hexSaving',
                        message: 'hexMessage',
                        label: 'playing hex message',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/hex/play`, {
                            method: 'POST',
                            body: { message: this.hexMessageText },
                        }),
                        success: 'Hex message queued successfully!',
                        failure: 'Failed to play hex message. Please try again.',
                        afterFn: () => this.closeModal('playHex'),
                        afterMs: 1500,
                    });
                },

                // Play Track Modal Functions
                async playTrack() {
                    const body = { filename: this.playTrackFilename };

                    if (this.playTrackVolume !== null && this.playTrackVolume !== '') {
                        body.volume = parseFloat(this.playTrackVolume);
                    }

                    if (this.playTrackStart !== null && this.playTrackStart !== '') {
                        body.start_position = parseFloat(this.playTrackStart);
                    }

                    if (this.playTrackDuration !== null && this.playTrackDuration !== '') {
                        body.duration = parseFloat(this.playTrackDuration);
                    }

                    await this.runAction({
                        saving: 'playTrackSaving',
                        message: 'playTrackMessage',
                        label: 'playing track',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/track/play`, {
                            method: 'POST',
                            body,
                        }),
                        success: 'Track started successfully!',
                        failure: 'Failed to play track. Please try again.',
                        afterFn: () => this.closeModal('playTrack'),
                        afterMs: 1500,
                    });
                },

                // Track State Modal Functions
                openTrackStateModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showTrackStateModal = true;
                    this.trackName = '';
                    this.trackState = '';
                    this.trackFilename = '';
                    this.trackVolume = null;
                    this.trackLoops = '';
                    this.trackFadeTime = null;
                    this.trackMessage = { text: '', type: '' };
                },

                closeTrackStateModal() {
                    this.showTrackStateModal = false;
                    this.currentGuildId = null;
                    this.trackName = '';
                    this.trackState = '';
                    this.trackFilename = '';
                    this.trackVolume = null;
                    this.trackLoops = '';
                    this.trackFadeTime = null;
                    this.trackMessage = { text: '', type: '' };
                },

                trackStateChanged() {
                    // Clear filename when not starting
                    if (this.trackState !== 'start') {
                        this.trackFilename = '';
                    }
                },

                async changeTrackState() {
                    const body = {
                        name: this.trackName,
                        state: this.trackState || null,
                    };

                    if (this.trackState === 'start' && this.trackFilename) {
                        body.filename = this.trackFilename;
                    }

                    if (this.trackVolume !== null && this.trackVolume !== '') {
                        body.volume = parseFloat(this.trackVolume);
                    }

                    if (this.trackLoops !== '') {
                        body.loops = this.trackLoops === 'true';
                    }

                    if (this.trackFadeTime !== null && this.trackFadeTime !== '') {
                        body.fade_time = parseFloat(this.trackFadeTime);
                    }

                    await this.runAction({
                        saving: 'trackSaving',
                        message: 'trackMessage',
                        label: 'changing track state',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/tracks`, {
                            method: 'POST',
                            body,
                        }),
                        success: 'Track state updated successfully!',
                        failure: 'Failed to change track state. Please try again.',
                        afterFn: () => this.closeTrackStateModal(),
                        afterMs: 1500,
                    });
                },

                // Signal Profile Modal Functions
                openSignalProfileModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showSignalProfileModal = true;
                    this.selectedProfile = '';
                    this.profileSetting = 'profile';
                    this.profileFadeDuration = null;
                    this.profileMessage = { text: '', type: '' };
                },

                closeSignalProfileModal() {
                    this.showSignalProfileModal = false;
                    this.currentGuildId = null;
                    this.selectedProfile = '';
                    this.profileSetting = 'profile';
                    this.profileFadeDuration = null;
                    this.profileMessage = { text: '', type: '' };
                },

                async changeSignalProfile() {
                    const body = {
                        profile: this.selectedProfile || null,
                    };

                    if (body.profile
                        && this.profileFadeDuration !== null && this.profileFadeDuration !== '') {
                        body.fade_duration = parseFloat(this.profileFadeDuration);
                    }

                    await this.runAction({
                        saving: 'profileSaving',
                        message: 'profileMessage',
                        label: 'changing signal profile',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj/${this.profileSetting}`, {
                            method: 'POST',
                            body,
                        }),
                        success: 'Signal profile changed successfully!',
                        failure: 'Failed to change signal profile. Please try again.',
                        afterFn: () => this.closeSignalProfileModal(),
                        afterMs: 1500,
                    });
                },

                async loadBotProfile() {
                    this.botProfileLoading = true;
                    try {
                        const response = await apiRequest(`/api/guilds/${this.currentGuildId}/bot-profile`, {
                            errorMessage: bodyOrStatus,
                        });
                        this.botProfileCurrent = await response.json();
                        this.botProfileForm.nick = this.botProfileCurrent.nick || '';
                    } catch (error) {
                        console.error('Failed to load bot profile:', error);
                        this.botProfileMessage = { text: `Failed to load the current profile: ${error.message}`, type: 'error' };
                    } finally {
                        this.botProfileLoading = false;
                    }
                },

                pickBotProfileImage(field) {
                    this.openFileBrowser({
                        mode: 'file',
                        prefix: 'images/',
                        onSelect: (key) => { this.botProfileForm[field] = 's3://' + key; },
                    });
                },

                async saveBotProfile() {
                    const body = {};
                    if (this.botProfileForm.nick !== (this.botProfileCurrent.nick || '')) body.nick = this.botProfileForm.nick;
                    if (this.botProfileForm.avatar) body.avatar = this.botProfileForm.avatar;
                    if (this.botProfileForm.banner) body.banner = this.botProfileForm.banner;
                    if (Object.keys(body).length === 0) {
                        this.botProfileMessage = { text: 'Nothing changed', type: 'error' };
                        return;
                    }

                    this.botProfileSaving = true;
                    this.botProfileMessage = { text: '', type: '' };
                    try {
                        await apiRequest(`/api/guilds/${this.currentGuildId}/bot-profile`, {
                            method: 'PUT',
                            body,
                            errorMessage: bodyOrStatus,
                        });
                        this.botProfileMessage = { text: 'Profile saved', type: 'success' };
                        this.botProfileForm.avatar = '';
                        this.botProfileForm.banner = '';
                        await this.loadBotProfile();
                    } catch (error) {
                        console.error('Failed to save bot profile:', error);
                        this.botProfileMessage = { text: `Failed to save the profile: ${error.message}`, type: 'error' };
                    } finally {
                        this.botProfileSaving = false;
                    }
                },

                async setChannelStatusEnabled(guildId, enabled) {
                    try {
                        await apiRequest(`/api/guilds/${guildId}/dj/channel-status`, {
                            method: 'POST',
                            body: { enabled },
                        });
                    } catch (error) {
                        console.error('Error changing channel status setting:', error);
                        alert('Failed to change the voice channel status setting. Please try again.');
                    }
                },

                // Activity Management Functions
                async performActivityAction(endpoint, activityType, status, successMessage, statusField) {
                    await this.runAction({
                        saving: 'activitySaving',
                        message: 'activityMessage',
                        label: 'performing activity action',
                        request: () => apiRequest(endpoint, {
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
                        afterFn: () => { this.activityMessage = { text: '', type: '' }; },
                        afterMs: 2000,
                    });
                },

                async setActivity() {
                    await this.performActivityAction(
                        '/api/activity/set',
                        this.newActivityType,
                        this.newActivityStatus,
                        'Activity set successfully!',
                        'newActivityStatus'
                    );
                },



                // DJ Config Override Functions
                async openDJConfigModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showDJConfigModal = true;
                    await this.loadDJConfigOverrides();
                },

                async loadDJConfigOverrides() {
                    this.djConfigLoading = true;
                    this.djConfigError = null;
                    this.djConfigMessage = { text: '', type: '' };

                    try {
                        // Load both overrides and default state weights in parallel
                        const [overridesResponse, defaultWeightsResponse] = await Promise.all([
                            fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides`),
                            fetch('/api/dj-config/default-state-weights')
                        ]);
                        
                        if (!overridesResponse.ok) {
                            throw new Error('Failed to load DJ config overrides');
                        }
                        
                        this.djConfigData = await overridesResponse.json();
                        
                        // Load default state weights
                        if (defaultWeightsResponse.ok) {
                            this.defaultStateWeights = await defaultWeightsResponse.json();
                            
                            // If state weights override has no value, initialize with defaults
                            if (!this.djConfigData.state_weights.value) {
                                this.djConfigData.state_weights.value = { ...this.defaultStateWeights };
                            }
                        }
                    } catch (error) {
                        console.error('Error loading DJ config:', error);
                        this.djConfigError = 'Failed to load DJ config overrides. Please try again.';
                    } finally {
                        this.djConfigLoading = false;
                    }
                },

                closeDJConfigModal() {
                    this.showDJConfigModal = false;
                    this.currentGuildId = null;
                    this.djConfigError = null;
                    this.djConfigMessage = { text: '', type: '' };
                },

                async toggleDJConfigCategory(category) {
                    const enabled = this.djConfigData[category].enabled;
                    await this.runDJConfigAction({
                        label: 'toggling category',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/toggle`, {
                            method: 'POST',
                            body: { category, enabled },
                        }),
                        success: `${category.replace('_', ' ')} ${enabled ? 'enabled' : 'disabled'} successfully!`,
                        failure: 'Failed to toggle category. Please try again.',
                    });
                },

                runDJConfigAction(options) {
                    return this.runAction({
                        saving: 'djConfigSaving',
                        message: 'djConfigMessage',
                        afterFn: () => { this.djConfigMessage = { text: '', type: '' }; },
                        afterMs: 2000,
                        ...options,
                    });
                },

                openHexMessageForm(index = null) {
                    if (index !== null) {
                        const msg = this.djConfigData.hex_messages.items[index];
                        this.hexMessageFormData = {
                            index,
                            text: msg.text,
                            weight: msg.weight,
                            signal_profile: msg.signal_profile || '',
                            loop_min: msg.loop_min || null,
                            loop_max: msg.loop_max || null,
                            announcement: msg.announcement || ''
                        };
                    } else {
                        this.hexMessageFormData = { index: null, text: '', weight: 1, signal_profile: '', loop_min: null, loop_max: null, announcement: '' };
                    }
                    this.showHexMessageForm = true;
                },

                editHexMessage(index) {
                    this.openHexMessageForm(index);
                },

                closeHexMessageForm() {
                    this.showHexMessageForm = false;
                    this.hexMessageFormData = { index: null, text: '', weight: 1, signal_profile: '', loop_min: null, loop_max: null, announcement: '' };
                },

                async saveHexMessage() {
                    const payload = {
                        index: this.hexMessageFormData.index,
                        text: this.hexMessageFormData.text,
                        weight: this.hexMessageFormData.weight,
                        signal_profile: this.hexMessageFormData.signal_profile || null,
                        loop_min: this.hexMessageFormData.loop_min || null,
                        loop_max: this.hexMessageFormData.loop_max || null,
                        announcement: this.hexMessageFormData.announcement || null
                    };

                    await this.runDJConfigAction({
                        label: 'saving hex message',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/hex-messages`, {
                            method: 'POST',
                            body: payload,
                        }),
                        onSuccess: async () => {
                            await this.loadDJConfigOverrides();
                            this.closeHexMessageForm();
                        },
                        success: 'Hex message saved successfully!',
                        failure: 'Failed to save hex message. Please try again.',
                    });
                },

                async deleteHexMessage(index) {
                    if (!confirm('Are you sure you want to delete this hex message?')) {
                        return;
                    }
                    
                    await this.runDJConfigAction({
                        label: 'deleting hex message',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/hex-messages/${index}`, {
                            method: 'DELETE',
                        }),
                        onSuccess: () => this.loadDJConfigOverrides(),
                        success: 'Hex message deleted successfully!',
                        failure: 'Failed to delete hex message. Please try again.',
                    });
                },

                openAnnouncementForm(index = null) {
                    if (index !== null) {
                        this.announcementFormData = {
                            index,
                            text: this.djConfigData.hex_message_announcements.items[index]
                        };
                    } else {
                        this.announcementFormData = { index: null, text: '' };
                    }
                    this.showAnnouncementForm = true;
                },

                editAnnouncement(index) {
                    this.openAnnouncementForm(index);
                },

                closeAnnouncementForm() {
                    this.showAnnouncementForm = false;
                    this.announcementFormData = { index: null, text: '' };
                },

                async saveAnnouncement() {
                    const payload = {
                        index: this.announcementFormData.index,
                        text: this.announcementFormData.text
                    };

                    await this.runDJConfigAction({
                        label: 'saving announcement',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/announcements`, {
                            method: 'POST',
                            body: payload,
                        }),
                        onSuccess: async () => {
                            await this.loadDJConfigOverrides();
                            this.closeAnnouncementForm();
                        },
                        success: 'Announcement saved successfully!',
                        failure: 'Failed to save announcement. Please try again.',
                    });
                },

                async deleteAnnouncement(index) {
                    if (!confirm('Are you sure you want to delete this announcement?')) {
                        return;
                    }
                    
                    await this.runDJConfigAction({
                        label: 'deleting announcement',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/announcements/${index}`, {
                            method: 'DELETE',
                        }),
                        onSuccess: () => this.loadDJConfigOverrides(),
                        success: 'Announcement deleted successfully!',
                        failure: 'Failed to delete announcement. Please try again.',
                    });
                },

                async saveStateWeights() {
                    const weights = this.djConfigData.state_weights.value;
                    await this.runDJConfigAction({
                        label: 'saving state weights',
                        request: () => apiRequest(`/api/guilds/${this.currentGuildId}/dj-config/overrides/state-weights`, {
                            method: 'POST',
                            body: {
                                track: weights.track,
                                hex_message: weights.hex_message,
                                noise: weights.noise,
                            },
                        }),
                        success: 'State weights saved successfully!',
                        failure: 'Failed to save state weights. Please try again.',
                    });
                },

                // Channel Message Functions
                async openChannelMessageModal() {
                    this.showChannelMessageModal = true;
                    this.channelsLoading = true;
                    this.channelMessageError = '';
                    this.channelMessageSuccess = '';
                    this.selectedChannelId = '';
                    this.channelMessageText = '';
                    this.channelMessageTitle = '';
                    this.channelMessageDescription = '';
                    
                    try {
                        const response = await apiRequest('/api/registered-channels', {
                            errorMessage: 'Failed to load registered channels',
                            includeStatus: true,
                        });

                        const data = await response.json();
                        this.registeredChannels = data.channels;
                    } catch (error) {
                        console.error('Error loading registered channels:', error);
                        this.channelMessageError = error.message || 'Failed to load registered channels. Please try again.';
                    } finally {
                        this.channelsLoading = false;
                    }
                },

                closeChannelMessageModal() {
                    this.showChannelMessageModal = false;
                    this.registeredChannels = [];
                    this.selectedChannelId = '';
                    this.channelMessageText = '';
                    this.channelMessageTitle = '';
                    this.channelMessageDescription = '';
                    this.channelMessageError = '';
                    this.channelMessageSuccess = '';
                },

                async sendChannelMessage() {
                    this.channelMessageSaving = true;
                    this.channelMessageError = '';
                    this.channelMessageSuccess = '';
                    
                    try {
                        // Validate at least one field is filled
                        if (!this.channelMessageText && !this.channelMessageTitle && !this.channelMessageDescription) {
                            this.channelMessageError = 'Please provide at least a message, title, or description.';
                            return;
                        }
                        
                        const body = {};
                        if (this.channelMessageText) {
                            body.message = this.channelMessageText;
                        }
                        if (this.channelMessageTitle) {
                            body.title = this.channelMessageTitle;
                        }
                        if (this.channelMessageDescription) {
                            body.description = this.channelMessageDescription;
                        }
                        
                        await apiRequest(`/api/registered-channels/${this.selectedChannelId}/send`, {
                            method: 'POST',
                            body,
                            errorMessage: 'Failed to send message',
                            includeStatus: true,
                        });

                        this.channelMessageSuccess = 'Message sent successfully!';
                        
                        // Clear form fields after successful send
                        this.channelMessageText = '';
                        this.channelMessageTitle = '';
                        this.channelMessageDescription = '';
                        
                        setTimeout(() => {
                            this.channelMessageSuccess = '';
                        }, 3000);
                    } catch (error) {
                        console.error('Error sending message:', error);
                        this.channelMessageError = error.message || 'Failed to send message. Please try again.';
                    } finally {
                        this.channelMessageSaving = false;
                    }
                }
            };
        }
