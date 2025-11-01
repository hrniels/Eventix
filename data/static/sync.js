let syncForce = false;
let syncing = false;
let outOfSync = false;

function postWithSpinner(spinnerId, url, onsuccess) {
    $('#' + spinnerId).addClass('ev_spin');
    postRequest(url, function(data) {
        $('#' + spinnerId).removeClass('ev_spin');
        onsuccess(data);
    });
}

function discoverCollection(col_id, spinnerId, onsuccess) {
    const url = '/api/calendars/syncop?op[type]=DiscoverCollection&op[data][col_id]=' + col_id;
    postWithSpinner(spinnerId, url, onsuccess);
}

function syncCollection(col_id, spinnerId, onsuccess) {
    const url = '/api/calendars/syncop?op[type]=SyncCollection&op[data][col_id]=' + col_id;
    postWithSpinner(spinnerId, url, onsuccess);
}

function reloadCollection(col_id, spinnerId, onsuccess) {
    const url = '/api/calendars/syncop?op[type]=ReloadCollection&op[data][col_id]=' + col_id;
    postWithSpinner(spinnerId, url, onsuccess);
}

function reloadCalendar(col_id, cal_id, spinnerId, onsuccess) {
    let url = '/api/calendars/syncop?op[type]=ReloadCalendar';
    url += '&op[data][col_id]=' + col_id;
    url += '&op[data][cal_id]=' + cal_id;
    postWithSpinner(spinnerId, url, onsuccess);
}

function reloadDBEvery(period, lastReloadId, spinnerId, iconId) {
    setInterval(function() {
        reloadDB(lastReloadId, spinnerId, iconId, false);
    }, period);
}

function reloadDB(lastReloadId, spinnerId, iconId, force, auth_url) {
    // if it's already out of sync, and the user clicks the refresh button, directly reload
    if(outOfSync && force) {
        reloadPage();
        return;
    }

    // don't sync in parallel
    if(syncing) {
        // but add force, if requested
        syncForce |= force;
        return;
    }

    syncing = true;
    syncForce = force;

    let url = '/api/calendars/syncop?op[type]=ReloadAll';
    if(auth_url)
        url += '&auth_url=' + encodeURIComponent(auth_url);

    postWithSpinner(spinnerId, url, function(data) {
        $('#' + lastReloadId).html(data.date);
        let auth = false;
        for(var cal in data.calendars) {
            // auth problem? Then show auth popup
            if(!auth && data.calendars[cal].AuthFailed) {
                fireEvent(new AuthEvent(cal, data.calendars[cal].AuthFailed));
                auth = true;
            }

            let btn = $('#ev_cal_src_button_' + cal);
            let error = $('#calendar_enabled' + cal);
            let calname = error.attr('data-name');

            // update calendar buttons
            if(data.calendars[cal].Error) {
                error.attr('title', calname + ": " + data.calendars[cal].Error);
                btn.show();
            }
            else {
                error.attr('title', calname);
                btn.hide();
            }
        }
        if(!auth && data.changed)
            requestReload(iconId, syncForce);
        syncing = false;
    });
}

function requestReload(iconId, force) {
    if(force || !userIsActive())
        reloadPage();
    else {
        outOfSync = true;
        $('#' + iconId).css('color', 'red');
    }
}
