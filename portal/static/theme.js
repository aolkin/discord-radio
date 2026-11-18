const storageKey = 'theme';

const themes = {
    system: { label: 'System', icon: '◐', next: 'light' },
    light: { label: 'Light', icon: '☀', next: 'dark' },
    dark: { label: 'Dark', icon: '☾', next: 'system' },
};

function readTheme() {
    try {
        const value = localStorage.getItem(storageKey);
        return value === 'light' || value === 'dark' ? value : 'system';
    } catch {
        return 'system';
    }
}

function applyTheme(theme) {
    if (theme === 'system') {
        delete document.documentElement.dataset.theme;
    } else {
        document.documentElement.dataset.theme = theme;
    }
}

function saveTheme(theme) {
    try {
        if (theme === 'system') {
            localStorage.removeItem(storageKey);
        } else {
            localStorage.setItem(storageKey, theme);
        }
    } catch {
        return;
    }
}

function showTheme(button, theme) {
    const label = `Theme: ${themes[theme].label}`;
    button.textContent = themes[theme].icon;
    button.title = label;
    button.setAttribute('aria-label', label);
}

applyTheme(readTheme());

document.addEventListener('DOMContentLoaded', () => {
    const button = document.getElementById('themeSwitch');
    let theme = readTheme();
    showTheme(button, theme);
    button.addEventListener('click', () => {
        theme = themes[theme].next;
        applyTheme(theme);
        saveTheme(theme);
        showTheme(button, theme);
    });
});
