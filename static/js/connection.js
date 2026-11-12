import { apiRequest, bodyOrStatus } from './api.js';

export function connection() {
    return {
        ws: null,
        connected: false,
        statusText: 'Connecting...',
        guilds: [],
        guildNames: {},
        guildList: [],
        guildListLoading: false,
        version: '',
        commitHash: '',
        lastCommitHash: null,
        reconnectTimeout: null,
        currentTime: Date.now(),

        init() {
            this.connect();
            // Load audio files and profiles
            this.loadGuildList();
            this.loadAudioFiles();
            this.loadProfiles();
            this.loadPools();
            this.loadDjConfigs();
            this.initRouter();
            // Update current time every 500ms for smooth progress bars
            setInterval(() => {
                this.currentTime = Date.now();
            }, 500);
        },

        async loadGuildList() {
            if (this.guildListLoading) {
                return;
            }
            this.guildListLoading = true;
            try {
                const response = await apiRequest('/api/guilds', { errorMessage: bodyOrStatus });
                const data = await response.json();
                this.guildList = data.guilds;
                this.guildNames = Object.fromEntries(data.guilds.map((guild) => [guild.guild_id, guild.name]));
            } catch (error) {
                console.error('Failed to load guild list:', error);
                setTimeout(() => this.loadGuildList(), 3000);
            } finally {
                this.guildListLoading = false;
            }
        },

        guildName(guildId) {
            return this.guildNames[guildId] || guildId;
        },

        guildsWithoutRadio() {
            const withRadio = new Set(this.guilds.map((guild) => guild.guild_id));
            return this.guildList.filter((guild) => !withRadio.has(guild.guild_id));
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
                    if (this.guilds.some((guild) => !(guild.guild_id in this.guildNames))) {
                        this.loadGuildList();
                    }

                    // Update current activity from websocket data
                    if (data.current_activity) {
                        this.currentActivity = {
                            activity_type: data.current_activity.activity_type,
                            status: data.current_activity.status,
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
    };
}
