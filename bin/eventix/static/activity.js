var popupOpen = false;
var formPage = false;
var keyMouse = false;

function userIsActive() {
    return popupOpen || formPage || keyMouse;
}

function setFormPage(val) {
    formPage = val;
}

function setPopupOpen(val) {
    popupOpen = val;
}

function trackActivity() {
    // determine whether the user is active
    let timeout;
    function onActivity() {
        // is active now
        keyMouse = true;

        clearTimeout(timeout);
        timeout = setTimeout(() => {
            // timeout: user wasn't active recently
            keyMouse = false;
        }, 5 * 60 * 1000);
    }

    document.addEventListener("mousemove", onActivity);
    document.addEventListener("keydown", onActivity);
}
