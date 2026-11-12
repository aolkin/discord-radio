(() => {
    const realFetch = window.fetch.bind(window);
    const fixtureBase = '/preview/fixtures';
    const registries = {
        profiles: ['clear', 'weak_signal', 'tuning'],
        pools: ['noise', 'hex', 'track'],
        'dj-configs': ['default'],
    };
    const modes = ['track', 'hex', 'noise'];

    const store = {
        snapshots: {},
        registries: {},
        files: new Map(),
        folders: new Set(),
        botProfiles: {},
        radio: null,
    };

    const forcedMode = new URLSearchParams(window.location.search).get('dj');

    const loadJson = async (path) => (await realFetch(`${fixtureBase}/${path}`)).json();

    const ready = (async () => {
        const [guilds, channels, botProfile, listing, ...snapshots] = await Promise.all([
            loadJson('guilds.json'),
            loadJson('channels.json'),
            loadJson('bot-profile.json'),
            loadJson('files/tracks.json'),
            ...modes.map((mode) => loadJson(`snapshots/playing-${mode}.json`)),
            loadJson('snapshots/no-radio.json'),
        ]);
        store.guilds = guilds;
        store.channels = channels;
        for (const guild of guilds.guilds) {
            store.botProfiles[guild.guild_id] = { ...botProfile };
        }
        for (const file of listing.files) {
            store.files.set(file.key, file);
        }
        for (const folder of listing.folders) {
            store.folders.add(folder);
        }
        modes.forEach((mode, index) => {
            store.snapshots[mode] = snapshots[index];
        });
        store.snapshots.none = snapshots[modes.length];
        for (const [kind, names] of Object.entries(registries)) {
            const items = await Promise.all(names.map((name) => loadJson(`${kind}/${name}.json`)));
            store.registries[kind] = Object.fromEntries(items.map((item) => [item.name, item]));
        }
        const live = store.snapshots.track.guilds[0];
        store.radio = {
            mode: modes.includes(forcedMode) || forcedMode === 'none' ? forcedMode : 'track',
            startedAt: Date.now(),
            text: null,
            profile: null,
            channelStatus: null,
            announcements: structuredClone(live.announcement_channels),
            activity: structuredClone(store.snapshots.track.current_activity),
        };
    })();

    const nextMode = (mode) => modes[(modes.indexOf(mode) + 1) % modes.length];

    const setMode = (mode) => {
        store.radio.mode = mode;
        store.radio.startedAt = Date.now();
        store.radio.text = null;
    };

    const durationOf = (guild) => {
        const track = guild.tracks[0];
        if (track) {
            return track.duration.secs + track.duration.nanos / 1e9;
        }
        return Number.parseFloat(/\/ ([\d.]+)s\)/.exec(guild.dj_state.details)?.[1] ?? '0');
    };

    const buildSnapshot = () => {
        const { radio } = store;
        if (radio.mode !== 'none') {
            const total = durationOf(store.snapshots[radio.mode].guilds[0]);
            if (total > 0 && (Date.now() - radio.startedAt) / 1000 >= total) {
                setMode(forcedMode ? radio.mode : nextMode(radio.mode));
            }
        }
        const snapshot = structuredClone(store.snapshots[radio.mode]);
        snapshot.current_activity = radio.activity;
        const guild = snapshot.guilds[0];
        if (!guild) {
            return snapshot;
        }
        const elapsed = Math.max(0, (Date.now() - radio.startedAt) / 1000);
        const total = durationOf(guild);
        const track = guild.tracks[0];
        if (track) {
            const startedAt = radio.startedAt;
            track.start_time = {
                secs_since_epoch: Math.floor(startedAt / 1000),
                nanos_since_epoch: (startedAt % 1000) * 1e6,
            };
        }
        const timing = `(${Math.min(elapsed, total).toFixed(1)}s / ${total.toFixed(1)}s)`;
        const label = radio.text && radio.mode === 'hex' ? radio.text : guild.dj_state.details.replace(/ \(.*$/, '');
        guild.dj_state.details = `${label} ${timing}`;
        if (radio.profile) {
            guild.audio_profile = {
                ...guild.audio_profile,
                name: radio.profile,
                expected_name: radio.profile,
                transitioning: false,
            };
        }
        if (radio.channelStatus !== null) {
            guild.channel_status = { ...guild.channel_status, status: radio.channelStatus };
        }
        guild.announcement_channels = radio.announcements;
        return snapshot;
    };

    const folderOf = (key) => key.slice(0, key.lastIndexOf('/') + 1);

    const listFiles = (prefix) => {
        const folders = new Set();
        const files = [];
        for (const key of store.files.keys()) {
            if (key.startsWith(prefix)) {
                const rest = key.slice(prefix.length);
                const slash = rest.indexOf('/');
                if (slash === -1) {
                    files.push(store.files.get(key));
                } else {
                    folders.add(prefix + rest.slice(0, slash + 1));
                }
            }
        }
        for (const folder of store.folders) {
            const rest = folder.slice(prefix.length);
            if (folder.startsWith(prefix) && rest) {
                folders.add(prefix + rest.slice(0, rest.indexOf('/') + 1));
            }
        }
        return { prefix, folders: [...folders].sort(), files };
    };

    const addFile = (key, size) => {
        store.files.set(key, { key, size, last_modified: new Date().toISOString() });
        store.folders.add(folderOf(key));
    };

    const respond = (data, status = 200) =>
        new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json' } });
    const fail = (status, message) => new Response(message, { status });

    const registryRoutes = Object.keys(registries).flatMap((kind) => [
        ['GET', new RegExp(`^/api/${kind}$`), () => respond(Object.keys(store.registries[kind]))],
        [
            'GET',
            new RegExp(`^/api/${kind}/([^/]+)$`),
            ([, name]) => {
                const item = store.registries[kind][decodeURIComponent(name)];
                return item ? respond(item) : fail(404, 'Not found');
            },
        ],
        [
            'PUT',
            new RegExp(`^/api/${kind}/([^/]+)$`),
            ([, name], { body }) => {
                const key = decodeURIComponent(name);
                store.registries[kind][key] = { ...body, name: key };
                return respond(store.registries[kind][key]);
            },
        ],
    ]);

    const guildRoute = (method, suffix, handler) => [
        method,
        new RegExp(`^/api/guilds/([^/]+)/${suffix}$`),
        ([, guildId, ...rest], request) => handler(request, guildId, rest),
    ];

    const ok = () => respond({ ok: true });

    const routes = [
        ['GET', /^\/api\/guilds$/, () => respond(store.guilds)],
        guildRoute('GET', 'channels', () => respond(store.channels)),
        guildRoute('POST', 'channels/([^/]+)/send', ok),
        guildRoute('GET', 'bot-profile', (_request, guildId) => respond(store.botProfiles[guildId] ?? {})),
        guildRoute('PUT', 'bot-profile', ({ body }, guildId) => {
            const current = store.botProfiles[guildId] ?? {};
            store.botProfiles[guildId] = {
                nick: body.nick ?? current.nick,
                avatar_url: body.avatar ?? current.avatar_url,
                banner_url: body.banner ?? current.banner_url,
            };
            return respond(store.botProfiles[guildId]);
        }),
        guildRoute('POST', 'dj/advance', ({ body }) => {
            setMode(modes.includes(body.state_type) ? body.state_type : nextMode(store.radio.mode));
            return ok();
        }),
        guildRoute('POST', 'hex/play', ({ body }) => {
            setMode('hex');
            store.radio.text = body.message;
            return ok();
        }),
        guildRoute('POST', 'dj/channel-status', ({ body }) => {
            const status = body.status?.fixed ?? (typeof body.status === 'string' ? body.status : null);
            store.radio.channelStatus = status;
            return ok();
        }),
        guildRoute('POST', 'dj/profile', ({ body }) => {
            store.radio.profile = body.profile ?? null;
            return ok();
        }),
        guildRoute('POST', 'dj/announcement-channels', ({ body }) => {
            const channel = store.channels.channels.find((candidate) => candidate.channel_id === body.channel_id);
            if (channel && !store.radio.announcements.some((entry) => entry.channel_id === channel.channel_id)) {
                store.radio.announcements.push(channel);
            }
            return ok();
        }),
        guildRoute('DELETE', 'dj/announcement-channels/([^/]+)', (_request, _guildId, [channelId]) => {
            store.radio.announcements = store.radio.announcements.filter((entry) => entry.channel_id !== channelId);
            return ok();
        }),
        guildRoute('POST', 'track/play', ok),
        guildRoute('POST', 'tracks', ok),
        [
            'POST',
            /^\/api\/activity\/set$/,
            (_match, { body }) => {
                store.radio.activity = { activity_type: body.activity_type, status: body.status };
                return ok();
            },
        ],
        ['GET', /^\/api\/files$/, (_match, { query }) => respond(listFiles(query.get('prefix') ?? ''))],
        [
            'POST',
            /^\/api\/files\/folders$/,
            (_match, { body }) => {
                store.folders.add(`${body.prefix}${body.name}/`);
                return ok();
            },
        ],
        ...registryRoutes,
    ];

    window.fetch = async (input, init = {}) => {
        const url = new URL(typeof input === 'string' ? input : input.url, window.location.origin);
        if (!url.pathname.startsWith('/api/')) {
            return realFetch(input, init);
        }
        await ready;
        const method = (init.method ?? 'GET').toUpperCase();
        const body = typeof init.body === 'string' ? JSON.parse(init.body) : undefined;
        for (const [routeMethod, pattern, handler] of routes) {
            const match = routeMethod === method ? pattern.exec(url.pathname) : null;
            if (match) {
                return handler(match, { body, query: url.searchParams });
            }
        }
        return fail(404, 'Not mocked in preview');
    };

    class MockWebSocket extends EventTarget {
        constructor() {
            super();
            this.readyState = 0;
            this.onopen = null;
            this.onmessage = null;
            this.onclose = null;
            this.onerror = null;
            ready.then(() => {
                this.readyState = 1;
                this.onopen?.({});
                this.sendSnapshot();
                this.timer = setInterval(() => this.sendSnapshot(), 1000);
            });
        }

        sendSnapshot() {
            this.onmessage?.({ data: JSON.stringify(buildSnapshot()) });
        }

        send() {}

        close() {
            clearInterval(this.timer);
            this.readyState = 3;
            this.onclose?.({});
        }
    }
    window.WebSocket = MockWebSocket;

    class MockUploadRequest {
        constructor() {
            this.upload = { onprogress: null };
            this.onload = null;
            this.onerror = null;
            this.status = 0;
            this.responseText = '';
        }

        open(_method, url) {
            this.prefix = new URL(url, window.location.origin).searchParams.get('prefix') ?? '';
        }

        send(form) {
            const file = form.get('file');
            let loaded = 0;
            const timer = setInterval(() => {
                loaded = Math.min(loaded + 25, 100);
                this.upload.onprogress?.({ lengthComputable: true, loaded, total: 100 });
                if (loaded === 100) {
                    clearInterval(timer);
                    const key = `${this.prefix}${file.name}`;
                    addFile(key, file.size);
                    this.status = 200;
                    this.responseText = JSON.stringify({ key });
                    this.onload?.({});
                }
            }, 150);
        }
    }
    window.XMLHttpRequest = MockUploadRequest;
})();
