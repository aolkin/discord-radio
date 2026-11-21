import { apiRequest, bodyOrStatus } from './api.js';

export const trackForms = () => ({
    playTrackFilename: '',
    playTrackVolume: null,
    playTrackStart: null,
    playTrackDuration: null,
    trackName: '',
    trackMode: 'start',
    trackFilename: '',
    trackVolume: null,
    trackLoops: '',
    trackFadeTime: null,
});

export function tracks() {
    return {
        ...trackForms(),
        playTrackSaving: false,
        trackSaving: false,

        getTrackDurationSecs(track) {
            return track.duration ? track.duration.secs + track.duration.nanos / 1e9 : null;
        },

        getTrackProgress(track) {
            const durationSecs = this.getTrackDurationSecs(track);
            if (!durationSecs || track.loops) {
                return 0;
            }
            const elapsedMs =
                this.currentTime -
                (track.start_time.secs_since_epoch * 1000 + track.start_time.nanos_since_epoch / 1000000);
            const elapsedSecs = elapsedMs / 1000;
            const progress = Math.min((elapsedSecs / durationSecs) * 100, 100);
            return Math.max(0, progress);
        },

        getTrackTimeDisplay(track) {
            const durationSecs = this.getTrackDurationSecs(track);
            if (!durationSecs) {
                return '';
            }
            const elapsedMs =
                this.currentTime -
                (track.start_time.secs_since_epoch * 1000 + track.start_time.nanos_since_epoch / 1000000);
            const elapsedSecs = Math.max(0, Math.min(elapsedMs / 1000, durationSecs));
            return `${this.formatTime(elapsedSecs)} / ${this.formatTime(durationSecs)}`;
        },

        formatTime(seconds) {
            const mins = Math.floor(seconds / 60);
            const secs = Math.floor(seconds % 60);
            return `${mins}:${secs.toString().padStart(2, '0')}`;
        },

        pickTrackFile(field) {
            this.openFileBrowser({
                mode: 'file',
                prefix: 'tracks/',
                onSelect: (key) => {
                    this[field] = `s3://${key}`;
                },
            });
        },

        followPlayingTracks(playing) {
            if (playing.length === 0) {
                this.trackMode = 'start';
            } else if (!playing.some((track) => track.name === this.trackName)) {
                this.trackName = playing[0].name;
            }
        },

        trackFileLabel(filename) {
            return filename.replace(/^s3:\/\/(tracks\/)?/, '');
        },

        async playTrack() {
            const body = { filename: this.playTrackFilename };

            if (this.playTrackVolume !== null && this.playTrackVolume !== '') {
                body.volume = Number.parseFloat(this.playTrackVolume);
            }

            if (this.playTrackStart !== null && this.playTrackStart !== '') {
                body.start_position = Number.parseFloat(this.playTrackStart);
            }

            if (this.playTrackDuration !== null && this.playTrackDuration !== '') {
                body.duration = Number.parseFloat(this.playTrackDuration);
            }

            await this.runAction({
                saving: 'playTrackSaving',
                label: 'playing track',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/track/play`, {
                        method: 'POST',
                        body,
                        errorMessage: bodyOrStatus,
                    }),
                success: 'Track started successfully!',
                failure: 'Failed to play track',
            });
        },

        fileStem(filename) {
            return filename
                .split('/')
                .pop()
                .replace(/\.[^.]*$/, '');
        },

        async changeTrackState() {
            const start = this.trackMode === 'start';
            const body = {
                name: start ? this.trackName || this.fileStem(this.trackFilename) : this.trackName,
                state: this.trackMode === 'update' ? null : this.trackMode,
            };

            if (start) {
                body.filename = this.trackFilename;
            }

            if (this.trackMode !== 'stop' && this.trackVolume !== null && this.trackVolume !== '') {
                body.volume = Number.parseFloat(this.trackVolume);
            }

            if (this.trackMode !== 'stop' && this.trackLoops !== '') {
                body.loops = this.trackLoops === 'true';
            }

            if (this.trackFadeTime !== null && this.trackFadeTime !== '') {
                body.fade_time = Number.parseFloat(this.trackFadeTime);
            }

            await this.runAction({
                saving: 'trackSaving',
                label: 'changing track state',
                request: () =>
                    apiRequest(`/api/guilds/${this.route.name}/tracks`, {
                        method: 'POST',
                        body,
                        errorMessage: bodyOrStatus,
                    }),
                success: 'Track state updated successfully!',
                failure: 'Failed to change track state',
            });
        },
    };
}
