const storageKey = 'theme';

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

applyTheme(readTheme());

document.addEventListener('DOMContentLoaded', () => {
    const select = document.getElementById('themeSwitch');
    select.value = readTheme();
    select.addEventListener('change', () => {
        applyTheme(select.value);
        saveTheme(select.value);
    });
});
