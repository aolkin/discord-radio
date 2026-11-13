import { activity, modals as activityModals } from './js/activity.js';
import { botProfile, modals as botProfileModals } from './js/bot-profile.js';
import { channels, modals as channelsModals } from './js/channels.js';
import { connection } from './js/connection.js';
import { dj, modals as djModals } from './js/dj.js';
import { djConfigs } from './js/dj-configs.js';
import { fileBrowser, modals as fileBrowserModals } from './js/file-browser.js';
import { modalMethods, modals } from './js/modal.js';
import { pools } from './js/pools.js';
import { drafts } from './js/registry-editor.js';
import { router } from './js/router.js';
import { actionMethods } from './js/run-action.js';
import { signalProfiles, modals as signalProfilesModals } from './js/signal-profiles.js';
import { tracks } from './js/tracks.js';
import { weightSliders } from './js/weights.js';

Object.assign(
    modals,
    activityModals,
    botProfileModals,
    channelsModals,
    djModals,
    fileBrowserModals,
    signalProfilesModals,
);

function botPortal() {
    return {
        ...connection(),
        ...router(),
        ...dj(),
        ...tracks(),
        ...signalProfiles(),
        ...pools(),
        ...weightSliders(),
        ...djConfigs(),
        ...drafts(),
        ...fileBrowser(),
        ...botProfile(),
        ...activity(),
        ...channels(),
        ...actionMethods(),
        ...modalMethods,
    };
}

window.botPortal = botPortal;
