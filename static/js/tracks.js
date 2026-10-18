import { apiRequest, emptyMessage } from './api.js';

export const modals = {
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
    trackState: {
        flag: 'showTrackStateModal',
        perGuild: true,
        reset: () => ({
            trackName: '',
            trackState: '',
            trackFilename: '',
            trackVolume: null,
            trackLoops: '',
            trackFadeTime: null,
            trackMessage: emptyMessage(),
        }),
    },
};

export function tracks() {
    return {
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

        async loadAudioFiles() {
            try {
                const response = await apiRequest('/api/files?prefix=tracks/');
                const data = await response.json();
                this.audioFiles = data.files.map(file => 's3://' + file.key);
            } catch (error) {
                console.error('Failed to load audio files:', error);
            }
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
                afterFn: () => this.closeModal('trackState'),
                afterMs: 1500,
            });
        },
    };
}
