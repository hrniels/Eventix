let syncForce = false;
let syncing = false;
let outOfSync = false;

function reloadDBEvery(period, lastReloadId, spinnerId, iconId) {
    setInterval(function() {
        reloadDB(lastReloadId, spinnerId, iconId, false);
    }, period);
}

function reloadDB(lastReloadId, spinnerId, iconId, force) {
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
    $('#' + spinnerId).addClass('ev_spin');

    postRequest('/reload', function(data) {
        $('#' + spinnerId).removeClass('ev_spin');
        $('#' + lastReloadId).html(data.date);
        if(data.changed)
            requestReload(iconId, syncForce);
        for(var cal in data.calendars) {
            let btn = $('#ev_cal_src_button_' + cal);
            if(data.calendars[cal])
                btn.hide();
            else
                btn.show();
        }
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
