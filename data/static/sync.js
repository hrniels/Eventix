let syncForce = false;
let syncing = false;
let outOfSync = false;

function startSpinning(spinnerId) {
    const spinner = $("#" + spinnerId);
    if (!spinner.data("originalClass")) {
        spinner.data("originalClass", spinner.attr("class") || "");
    }
    spinner.attr("class", "bi bi-arrow-clockwise ev_spin");
    $("#" + spinnerId)
        .parent()
        .removeClass("ev_button_error");
}

function stopSpinning(spinnerId, error) {
    const spinner = $("#" + spinnerId);
    const originalClass = spinner.data("originalClass");
    if (originalClass) {
        spinner.attr("class", originalClass);
    } else {
        spinner.removeClass("ev_spin");
    }
    if (error) {
        $("#" + spinnerId)
            .parent()
            .addClass("ev_button_error")
            .effect("shake");
    }
}

function postWithSpinner(spinnerId, url, onresponse) {
    startSpinning(spinnerId);
    postRequest(url, function (data) {
        const error = handleCalErrors(data);
        const auth_error = handleAuthErrors(data, url, spinnerId);
        stopSpinning(spinnerId, error || auth_error);
        onresponse(data, !auth_error);
    });
}

function handleAuthErrors(data, op_url, spinnerId) {
    var error = false;
    if (!data || !data.collections) return false;
    for (var cal in data.collections) {
        // auth problem? Then show auth popup
        if (data.collections[cal].AuthFailed) {
            fireEvent(createAuthEvent(cal, data.collections[cal].AuthFailed, op_url, spinnerId));
            return true;
        } else if (data.collections[cal].Error) error = true;
    }
    return error;
}

function handleCalErrors(data) {
    var error = false;
    if (!data || !data.calendars) return false;
    for (var cal in data.calendars) {
        let btn = $("#ev_cal_src_button_" + cal);
        if (data.calendars[cal]) {
            error = true;
            btn.show();
        } else btn.hide();
    }
    return error;
}

function defaultSyncFinish(onsuccess) {
    return function (data, auth_success) {
        if (auth_success && data.changed) onsuccess();
    };
}

function discoverCollection(col_id, spinnerId, onsuccess) {
    const url = "/api/calendars/syncop?op[type]=DiscoverCollection&op[data][col_id]=" + col_id;
    postWithSpinner(spinnerId, url, defaultSyncFinish(onsuccess));
}

function syncCollection(col_id, spinnerId, onsuccess) {
    const url = "/api/calendars/syncop?op[type]=SyncCollection&op[data][col_id]=" + col_id;
    postWithSpinner(spinnerId, url, defaultSyncFinish(onsuccess));
}

function reloadCollection(col_id, spinnerId, onsuccess) {
    const url = "/api/calendars/syncop?op[type]=ReloadCollection&op[data][col_id]=" + col_id;
    postWithSpinner(spinnerId, url, defaultSyncFinish(onsuccess));
}

function reloadCalendar(col_id, cal_id, spinnerId, onsuccess) {
    let url = "/api/calendars/syncop?op[type]=ReloadCalendar";
    url += "&op[data][col_id]=" + col_id;
    url += "&op[data][cal_id]=" + cal_id;
    postWithSpinner(spinnerId, url, defaultSyncFinish(onsuccess));
}

let syncAllTimer = null;
let syncAllArgs = null;

// Schedules a sync to run once, `period` ms after the last user interaction.
// The timer is reset on every interaction via `resetSyncTimer`, so a sync only
// fires after the user has been idle for the full period.
function syncAllEvery(period, lastReloadId, spinnerId, iconId) {
    syncAllArgs = { period, lastReloadId, spinnerId, iconId };
    resetSyncTimer();
}

// Cancels any pending sync timer and reschedules it for `period` ms from now.
// Called by `trackActivity` on every user interaction.
function resetSyncTimer() {
    if (syncAllArgs === null) return;
    clearTimeout(syncAllTimer);
    const { period, lastReloadId, spinnerId, iconId } = syncAllArgs;
    syncAllTimer = setTimeout(function () {
        syncAll(lastReloadId, spinnerId, iconId, false);
        syncAllEvery(period, lastReloadId, spinnerId, iconId);
    }, period);
}

function syncAll(lastReloadId, spinnerId, iconId, force, auth_url) {
    // if it's already out of sync, and the user clicks the refresh button, directly reload
    if (outOfSync && force) {
        reloadPage();
        return;
    }

    // don't sync in parallel
    if (syncing) {
        // but add force, if requested
        syncForce |= force;
        return;
    }

    syncing = true;
    syncForce = force;

    let url = "/api/calendars/syncop?op[type]=SyncAll";
    if (auth_url) url += "&auth_url=" + encodeURIComponent(auth_url);

    postWithSpinner(spinnerId, url, function (data, auth_success) {
        $("#" + lastReloadId).html(data.date);
        syncing = false;
        if (auth_success && data.changed) requestReload(iconId, syncForce);
    });
}

function requestReload(iconId, force, sidebar = false) {
    if (force || !userIsActive()) {
        reloadContent();
        if (sidebar) reloadSidebar();
    } else {
        outOfSync = true;
        $("#" + iconId).css("color", "red");
    }
}
