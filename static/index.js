import { activity, modals as activityModals } from './js/activity.js';
import { botProfile, modals as botProfileModals } from './js/bot-profile.js';
import { channels, modals as channelsModals } from './js/channels.js';
import { connection } from './js/connection.js';
import { dj, modals as djModals } from './js/dj.js';
import { djConfig, modals as djConfigModals } from './js/dj-config.js';
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
    djConfigModals,
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
        ...fileBrowser(),
        ...botProfile(),
        ...activity(),
        ...djConfig(),
        ...channels(),
        ...actionMethods(),
        ...modalMethods,
    };
}

window.botPortal = botPortal;
