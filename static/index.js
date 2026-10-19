import { activity, modals as activityModals } from './js/activity.js';
import { botProfile, modals as botProfileModals } from './js/bot-profile.js';
import { channels, modals as channelsModals } from './js/channels.js';
import { connection } from './js/connection.js';
import { dj, modals as djModals } from './js/dj.js';
import { djConfigs, modals as djConfigsModals } from './js/dj-configs.js';
import { fileBrowser, modals as fileBrowserModals } from './js/file-browser.js';
import { modalMethods, modals } from './js/modal.js';
import { actionMethods } from './js/run-action.js';
import { signalProfiles, modals as signalProfilesModals } from './js/signal-profiles.js';
import { tracks, modals as tracksModals } from './js/tracks.js';

Object.assign(
    modals,
    activityModals,
    botProfileModals,
    channelsModals,
    djModals,
    djConfigsModals,
    fileBrowserModals,
    signalProfilesModals,
    tracksModals,
);

function botPortal() {
    return {
        ...connection(),
        ...dj(),
        ...tracks(),
        ...signalProfiles(),
        ...djConfigs(),
        ...fileBrowser(),
        ...botProfile(),
        ...activity(),
        ...channels(),
        ...actionMethods(),
        ...modalMethods,
    };
}

window.botPortal = botPortal;
