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

                async loadAudioFiles() {
                    try {
                        const response = await fetch('/api/files?prefix=tracks/');
                        if (response.ok) {
                            const data = await response.json();
                            this.audioFiles = data.files.map(file => 's3://' + file.key);
                        }
                    } catch (error) {
                        console.error('Failed to load audio files:', error);
                    }
                },

                async loadProfiles() {
                    try {
                        const response = await fetch('/api/profiles');
                        if (response.ok) {
                            const data = await response.json();
                            this.profiles = data.profiles.sort();
                        }
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
                        const response = await fetch(`/api/files?prefix=${encodeURIComponent(prefix)}`);
                        if (!response.ok) {
                            throw new Error((await response.text()) || `HTTP ${response.status}`);
                        }
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
                        const response = await fetch('/api/files/folders', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ prefix: this.fileBrowser.prefix, name: this.fileBrowser.newFolderName }),
                        });
                        if (!response.ok) {
                            throw new Error((await response.text()) || `HTTP ${response.status}`);
                        }
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
                        const response = await fetch(`/api/profiles/${encodeURIComponent(name)}`);
                        if (!response.ok) {
                            throw new Error(`HTTP ${response.status}`);
                        }
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
                        const response = await fetch(`/api/profiles/${encodeURIComponent(name)}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(body),
                        });
                        if (!response.ok) {
                            throw new Error((await response.text()) || `HTTP ${response.status}`);
                        }
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
                // DJ Modal Functions
                openAdvanceDJModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showAdvanceDJModal = true;
                    this.djStateType = '';
                    this.djMessage = { text: '', type: '' };
                },

                closeAdvanceDJModal() {
                    this.showAdvanceDJModal = false;
                    this.currentGuildId = null;
                    this.djStateType = '';
                    this.djMessage = { text: '', type: '' };
                },

                async advanceDJState() {
                    this.djSaving = true;
                    this.djMessage = { text: '', type: '' };

                    try {
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj/advance`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                state_type: this.djStateType || null,
                            }),
                        });

                        if (!response.ok) {
                            throw new Error('Failed to advance DJ state');
                        }

                        this.djMessage = {
                            text: 'DJ state advanced successfully!',
                            type: 'success'
                        };

                        setTimeout(() => this.closeAdvanceDJModal(), 1500);
                    } catch (error) {
                        console.error('Error advancing DJ state:', error);
                        this.djMessage = {
                            text: 'Failed to advance DJ state. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djSaving = false;
                    }
                },

                // Hex Message Modal Functions
                openPlayHexModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showPlayHexModal = true;
                    this.hexMessageText = '';
                    this.hexMessage = { text: '', type: '' };
                },

                closePlayHexModal() {
                    this.showPlayHexModal = false;
                    this.currentGuildId = null;
                    this.hexMessageText = '';
                    this.hexMessage = { text: '', type: '' };
                },

                async playHexMessage() {
                    this.hexSaving = true;
                    this.hexMessage = { text: '', type: '' };

                    try {
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/hex/play`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                message: this.hexMessageText,
                            }),
                        });

                        if (!response.ok) {
                            throw new Error('Failed to play hex message');
                        }

                        this.hexMessage = {
                            text: 'Hex message queued successfully!',
                            type: 'success'
                        };

                        setTimeout(() => this.closePlayHexModal(), 1500);
                    } catch (error) {
                        console.error('Error playing hex message:', error);
                        this.hexMessage = {
                            text: 'Failed to play hex message. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.hexSaving = false;
                    }
                },

                // Play Track Modal Functions
                openPlayTrackModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showPlayTrackModal = true;
                    this.resetPlayTrackForm();
                },

                closePlayTrackModal() {
                    this.showPlayTrackModal = false;
                    this.currentGuildId = null;
                    this.resetPlayTrackForm();
                },

                resetPlayTrackForm() {
                    this.playTrackFilename = '';
                    this.playTrackVolume = null;
                    this.playTrackStart = null;
                    this.playTrackDuration = null;
                    this.playTrackMessage = { text: '', type: '' };
                },

                async playTrack() {
                    this.playTrackSaving = true;
                    this.playTrackMessage = { text: '', type: '' };

                    try {
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

                        const response = await fetch(`/api/guilds/${this.currentGuildId}/track/play`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(body),
                        });

                        if (!response.ok) {
                            throw new Error('Failed to play track');
                        }

                        this.playTrackMessage = {
                            text: 'Track started successfully!',
                            type: 'success'
                        };

                        setTimeout(() => this.closePlayTrackModal(), 1500);
                    } catch (error) {
                        console.error('Error playing track:', error);
                        this.playTrackMessage = {
                            text: 'Failed to play track. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.playTrackSaving = false;
                    }
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
                    this.trackSaving = true;
                    this.trackMessage = { text: '', type: '' };

                    try {
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

                        const response = await fetch(`/api/guilds/${this.currentGuildId}/tracks`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(body),
                        });

                        if (!response.ok) {
                            throw new Error('Failed to change track state');
                        }

                        this.trackMessage = {
                            text: 'Track state updated successfully!',
                            type: 'success'
                        };

                        setTimeout(() => this.closeTrackStateModal(), 1500);
                    } catch (error) {
                        console.error('Error changing track state:', error);
                        this.trackMessage = {
                            text: 'Failed to change track state. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.trackSaving = false;
                    }
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
                    this.profileSaving = true;
                    this.profileMessage = { text: '', type: '' };

                    try {
                        const body = {
                            profile: this.selectedProfile || null,
                        };

                        if (body.profile
                            && this.profileFadeDuration !== null && this.profileFadeDuration !== '') {
                            body.fade_duration = parseFloat(this.profileFadeDuration);
                        }

                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj/${this.profileSetting}`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(body),
                        });

                        if (!response.ok) {
                            throw new Error('Failed to change signal profile');
                        }

                        this.profileMessage = {
                            text: 'Signal profile changed successfully!',
                            type: 'success'
                        };

                        setTimeout(() => this.closeSignalProfileModal(), 1500);
                    } catch (error) {
                        console.error('Error changing signal profile:', error);
                        this.profileMessage = {
                            text: 'Failed to change signal profile. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.profileSaving = false;
                    }
                },

                openBotProfileModal(guildId) {
                    this.currentGuildId = guildId;
                    this.showBotProfileModal = true;
                    this.botProfileMessage = { text: '', type: '' };
                    this.botProfileForm = { nick: '', avatar: '', banner: '' };
                    return this.loadBotProfile();
                },

                closeBotProfileModal() {
                    this.showBotProfileModal = false;
                    this.currentGuildId = null;
                },

                async loadBotProfile() {
                    this.botProfileLoading = true;
                    try {
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/bot-profile`);
                        if (!response.ok) {
                            throw new Error((await response.text()) || `HTTP ${response.status}`);
                        }
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
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/bot-profile`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(body),
                        });
                        if (!response.ok) {
                            throw new Error((await response.text()) || `HTTP ${response.status}`);
                        }
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
                        const response = await fetch(`/api/guilds/${guildId}/dj/channel-status`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ enabled }),
                        });
                        if (!response.ok) {
                            throw new Error('Failed to change channel status setting');
                        }
                    } catch (error) {
                        console.error('Error changing channel status setting:', error);
                        alert('Failed to change the voice channel status setting. Please try again.');
                    }
                },

                // Activity Management Functions
                openActivityModal() {
                    this.showActivityModal = true;
                    this.activityMessage = { text: '', type: '' };
                },

                closeActivityModal() {
                    this.showActivityModal = false;
                    this.newActivityStatus = '';
                    this.activityMessage = { text: '', type: '' };
                },

                async performActivityAction(endpoint, activityType, status, successMessage, statusField) {
                    this.activitySaving = true;
                    this.activityMessage = { text: '', type: '' };

                    try {
                        const response = await fetch(endpoint, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                activity_type: activityType,
                                status: status,
                            }),
                        });

                        if (!response.ok) {
                            throw new Error('Failed to perform activity action');
                        } else {
                            this.activityMessage = {
                                text: successMessage,
                                type: 'success'
                            };

                            // Clear the status field
                            if (statusField) {
                                this[statusField] = '';
                            }

                            setTimeout(() => {
                                this.activityMessage = { text: '', type: '' };
                            }, 2000);
                        }
                    } catch (error) {
                        console.error('Error performing activity action:', error);
                        this.activityMessage = {
                            text: 'Failed to perform action. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.activitySaving = false;
                    }
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
                    this.djConfigSaving = true;
                    this.djConfigMessage = { text: '', type: '' };
                    
                    try {
                        const enabled = this.djConfigData[category].enabled;
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides/toggle`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ category, enabled }),
                        });
                        
                        if (!response.ok) {
                            throw new Error('Failed to toggle category');
                        }
                        
                        this.djConfigMessage = {
                            text: `${category.replace('_', ' ')} ${enabled ? 'enabled' : 'disabled'} successfully!`,
                            type: 'success'
                        };
                        setTimeout(() => { this.djConfigMessage = { text: '', type: '' }; }, 2000);
                    } catch (error) {
                        console.error('Error toggling category:', error);
                        this.djConfigMessage = {
                            text: 'Failed to toggle category. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djConfigSaving = false;
                    }
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
                    this.djConfigSaving = true;
                    this.djConfigMessage = { text: '', type: '' };
                    
                    try {
                        const payload = {
                            index: this.hexMessageFormData.index,
                            text: this.hexMessageFormData.text,
                            weight: this.hexMessageFormData.weight,
                            signal_profile: this.hexMessageFormData.signal_profile || null,
                            loop_min: this.hexMessageFormData.loop_min || null,
                            loop_max: this.hexMessageFormData.loop_max || null,
                            announcement: this.hexMessageFormData.announcement || null
                        };
                        
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides/hex-messages`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(payload),
                        });
                        
                        if (!response.ok) {
                            throw new Error('Failed to save hex message');
                        }
                        
                        // Reload the config
                        await this.loadDJConfigOverrides();
                        this.closeHexMessageForm();
                        
                        this.djConfigMessage = {
                            text: 'Hex message saved successfully!',
                            type: 'success'
                        };
                        setTimeout(() => { this.djConfigMessage = { text: '', type: '' }; }, 2000);
                    } catch (error) {
                        console.error('Error saving hex message:', error);
                        this.djConfigMessage = {
                            text: 'Failed to save hex message. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djConfigSaving = false;
                    }
                },

                async deleteHexMessage(index) {
                    if (!confirm('Are you sure you want to delete this hex message?')) {
                        return;
                    }
                    
                    this.djConfigSaving = true;
                    this.djConfigMessage = { text: '', type: '' };
                    
                    try {
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides/hex-messages/${index}`, {
                            method: 'DELETE',
                        });
                        
                        if (!response.ok) {
                            throw new Error('Failed to delete hex message');
                        }
                        
                        // Reload the config
                        await this.loadDJConfigOverrides();
                        
                        this.djConfigMessage = {
                            text: 'Hex message deleted successfully!',
                            type: 'success'
                        };
                        setTimeout(() => { this.djConfigMessage = { text: '', type: '' }; }, 2000);
                    } catch (error) {
                        console.error('Error deleting hex message:', error);
                        this.djConfigMessage = {
                            text: 'Failed to delete hex message. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djConfigSaving = false;
                    }
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
                    this.djConfigSaving = true;
                    this.djConfigMessage = { text: '', type: '' };
                    
                    try {
                        const payload = {
                            index: this.announcementFormData.index,
                            text: this.announcementFormData.text
                        };
                        
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides/announcements`, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(payload),
                        });
                        
                        if (!response.ok) {
                            throw new Error('Failed to save announcement');
                        }
                        
                        // Reload the config
                        await this.loadDJConfigOverrides();
                        this.closeAnnouncementForm();
                        
                        this.djConfigMessage = {
                            text: 'Announcement saved successfully!',
                            type: 'success'
                        };
                        setTimeout(() => { this.djConfigMessage = { text: '', type: '' }; }, 2000);
                    } catch (error) {
                        console.error('Error saving announcement:', error);
                        this.djConfigMessage = {
                            text: 'Failed to save announcement. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djConfigSaving = false;
                    }
                },

                async deleteAnnouncement(index) {
                    if (!confirm('Are you sure you want to delete this announcement?')) {
                        return;
                    }
                    
                    this.djConfigSaving = true;
                    this.djConfigMessage = { text: '', type: '' };
                    
                    try {
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides/announcements/${index}`, {
                            method: 'DELETE',
                        });
                        
                        if (!response.ok) {
                            throw new Error('Failed to delete announcement');
                        }
                        
                        // Reload the config
                        await this.loadDJConfigOverrides();
                        
                        this.djConfigMessage = {
                            text: 'Announcement deleted successfully!',
                            type: 'success'
                        };
                        setTimeout(() => { this.djConfigMessage = { text: '', type: '' }; }, 2000);
                    } catch (error) {
                        console.error('Error deleting announcement:', error);
                        this.djConfigMessage = {
                            text: 'Failed to delete announcement. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djConfigSaving = false;
                    }
                },

                async saveStateWeights() {
                    this.djConfigSaving = true;
                    this.djConfigMessage = { text: '', type: '' };
                    
                    try {
                        const response = await fetch(`/api/guilds/${this.currentGuildId}/dj-config/overrides/state-weights`, {
                            method: 'POST',
                            headers: {
                                'Content-Type': 'application/json',
                            },
                            body: JSON.stringify({
                                track: this.djConfigData.state_weights.value.track,
                                hex_message: this.djConfigData.state_weights.value.hex_message,
                                noise: this.djConfigData.state_weights.value.noise,
                            }),
                        });
                        
                        if (!response.ok) {
                            throw new Error('Failed to save state weights');
                        }
                        
                        this.djConfigMessage = {
                            text: 'State weights saved successfully!',
                            type: 'success'
                        };
                        setTimeout(() => { this.djConfigMessage = { text: '', type: '' }; }, 2000);
                    } catch (error) {
                        console.error('Error saving state weights:', error);
                        this.djConfigMessage = {
                            text: 'Failed to save state weights. Please try again.',
                            type: 'error'
                        };
                    } finally {
                        this.djConfigSaving = false;
                    }
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
                        const response = await fetch('/api/registered-channels');
                        if (!response.ok) {
                            const statusText = response.statusText || 'Unknown error';
                            throw new Error(`Failed to load registered channels (${response.status}: ${statusText})`);
                        }
                        
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
                        
                        const response = await fetch(`/api/registered-channels/${this.selectedChannelId}/send`, {
                            method: 'POST',
                            headers: {
                                'Content-Type': 'application/json',
                            },
                            body: JSON.stringify(body),
                        });
                        
                        if (!response.ok) {
                            const statusText = response.statusText || 'Unknown error';
                            throw new Error(`Failed to send message (${response.status}: ${statusText})`);
                        }
                        
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
