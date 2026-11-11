const newItem = '+new';

const editors = {
    'dj-configs': 'openDjConfigEditor',
    pools: 'openPoolEditor',
    'signal-profiles': 'openProfileEditor',
};

function parseRoute(hash) {
    const [view, kind, name] = hash.replace(/^#\/?/, '').split('/');
    if (view !== 'edit') {
        return { view: 'live', kind: null, name: null };
    }
    if (Object.hasOwn(editors, kind) && /^([\w-]+|\+new)$/.test(name)) {
        return { view, kind, name };
    }
    return { view, kind: null, name: null };
}

function routeHash({ view, kind, name }) {
    return kind ? `#/${view}/${kind}/${name}` : `#/${view}`;
}

export function router() {
    return {
        route: { view: null, kind: null, name: null },
        newItem,

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
                return;
            }
            if (previous.view !== 'edit') {
                this.loadDjConfigPools();
            }
            if (this.route.kind) {
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

        isSelected(kind, name) {
            return this.route.kind === kind && this.route.name === name;
        },
    };
}
