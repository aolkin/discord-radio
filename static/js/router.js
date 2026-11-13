import { playForms } from './dj.js';
import { trackForms } from './tracks.js';

export const newItem = '+new';

export const guildTabs = [
    { id: 'status', label: 'Status' },
    { id: 'play', label: 'Play' },
    { id: 'tracks', label: 'Tracks' },
    { id: 'signal-profile', label: 'Signal profile' },
    { id: 'channel-status', label: 'Channel status' },
    { id: 'announcements', label: 'Announcements' },
    { id: 'bot-profile', label: 'Bot profile' },
    { id: 'logs', label: 'Logs' },
];

const editors = {
    'dj-configs': 'openDjConfigEditor',
    pools: 'openPoolEditor',
    'signal-profiles': 'openProfileEditor',
};

function parseRoute(hash) {
    const [view, kind, ...rest] = hash.replace(/^#\/?/, '').split('/');
    const name = rest.join('/');
    if (view !== 'edit') {
        const guild = /^\d+$/.test(kind ?? '');
        const tab = guildTabs.some((candidate) => candidate.id === rest[0]) ? rest[0] : 'status';
        return { view: 'live', kind: null, name: guild ? kind : null, tab: guild ? tab : null };
    }
    if (Object.hasOwn(editors, kind) && /^([\w-]+|\+new)$/.test(name)) {
        return { view, kind, name };
    }
    if (kind === 'files' && (name === '' || /^[\w-]+(\/[\w-]+)*$/.test(name))) {
        return { view, kind, name: name || 'tracks' };
    }
    return { view, kind: null, name: null };
}

function routeHash({ view, kind, name, tab }) {
    if (tab) {
        return `#/${view}/${name}/${tab}`;
    }
    if (kind) {
        return `#/${view}/${kind}/${name}`;
    }
    return name ? `#/${view}/${name}` : `#/${view}`;
}

export function router() {
    return {
        route: { view: null, kind: null, name: null, tab: null },
        newItem,
        guildTabs,

        initRouter() {
            window.addEventListener('hashchange', () => this.followHash());
            this.followHash();
        },

        followHash() {
            const previous = this.route;
            this.route = parseRoute(window.location.hash);
            const hash = routeHash(this.route);
            if (window.location.hash !== hash) {
                history.replaceState(null, '', hash);
            }

            if (this.route.view !== 'edit') {
                if (previous.name !== this.route.name || previous.tab !== this.route.tab) {
                    Object.assign(this, playForms(), trackForms());
                }
                return;
            }
            if (previous.view !== 'edit') {
                this.loadDjConfigPools();
            }
            if (this.route.kind === 'files') {
                this.openFileBrowserPage(this.route.name);
            } else if (this.route.kind) {
                this[editors[this.route.kind]](this.route.name === newItem ? null : this.route.name);
            }
        },

        replaceRoute(route) {
            this.route = route;
            history.replaceState(null, '', routeHash(route));
        },

        navigate(hash) {
            window.location.hash = hash;
        },

        openItem(kind, name) {
            this.navigate(routeHash({ view: 'edit', kind, name }));
        },

        openGuild(guildId, tab = 'status') {
            this.navigate(routeHash({ view: 'live', kind: null, name: guildId, tab }));
        },

        currentGuild() {
            return this.guilds.find((guild) => guild.guild_id === this.route.name);
        },

        isSelected(kind, name) {
            return this.route.kind === kind && this.route.name === name;
        },
    };
}
