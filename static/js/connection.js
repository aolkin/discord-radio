export function connection() {
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

        init() {
            this.connect();
            // Load audio files and profiles
            this.loadAudioFiles();
            this.loadProfiles();
            this.loadPools();
            this.loadDjConfigs();
            // Update current time every 500ms for smooth progress bars
            setInterval(() => {
                this.currentTime = Date.now();
            }, 500);
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
