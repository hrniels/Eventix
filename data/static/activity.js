let popupOpen = false;
let formPage = false;
let keyMouse = false;
let openForms = 0;

function userIsActive() {
    return keyMouse || popupOpen || formPage || openForms > 0;
}

function setFormPage(val) {
    formPage = val;
}

function setPopupOpen(val) {
    popupOpen = val;
}

function openForm() {
    openForms += 1;
}

function closeForm() {
    openForms -= 1;
}

function trackActivity() {
    // determine whether the user is active
    let timeout;
    function onActivity() {
        // is active now
        keyMouse = true;

        clearTimeout(timeout);
        timeout = setTimeout(
            () => {
                // timeout: user wasn't active recently
                keyMouse = false;
            },
            5 * 60 * 1000,
        );

        // reset the background sync timer so it only fires after a full period
        // of inactivity, not while the user is actively using the page
        if (typeof resetSyncTimer === "function") resetSyncTimer();
    }

    document.addEventListener("mousemove", onActivity);
    document.addEventListener("keydown", onActivity);
    document.addEventListener("click", onActivity);
}
