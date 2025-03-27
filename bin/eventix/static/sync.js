var syncPopupOpen = false;
var syncFormPage = false;
var syncRecentActivity = false;
var syncForce = false;
var syncing = false;
var outOfSync = false;

function reloadDBEvery(period, lastReloadId, spinnerId, iconId) {
    setInterval(function() {
        reloadDB(lastReloadId, spinnerId, iconId, false);
    }, period);

    // determine whether the user is active
    let timeout;
    function onActivity() {
        // is active now
        syncRecentActivity = true;

        clearTimeout(timeout);
        timeout = setTimeout(() => {
            // timeout: user wasn't active recently
            syncRecentActivity = false;
        }, 5 * 60 * 1000);
    }

    document.addEventListener("mousemove", onActivity);
    document.addEventListener("keydown", onActivity);
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
        if(data.changed) {
            if(syncForce || (!syncRecentActivity && !syncFormPage && !syncPopupOpen))
                reloadPage();
            else {
                outOfSync = true;
                $('#' + iconId).css('color', 'red');
            }
        }
        syncing = false;
    });
}
