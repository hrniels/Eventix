const MAX_AJAX_ERRORS = 10;
let ajaxErrors = [];

function errorListLabel(name, fallback) {
    const list = $("#error-list");
    return (list.length && list.data(name)) || fallback;
}

function formatTemplate(template, value) {
    return template.replace("{}", value);
}

function formatAJAXErrorMessage(jqXHR, textStatus, errorThrown) {
    if (jqXHR && jqXHR.responseJSON && jqXHR.responseJSON.error) return jqXHR.responseJSON.error;
    if (errorThrown) return errorThrown;
    if (textStatus && textStatus !== "error") return textStatus;
    if (jqXHR && jqXHR.status) {
        return formatTemplate(
            errorListLabel("statusLabel", "Request failed with status {}"),
            jqXHR.status,
        );
    }
    return errorListLabel("unknownLabel", "Unknown error");
}

function escapeHTML(value) {
    return $("<div>").text(value).html();
}

function renderAJAXErrors() {
    const list = $("#error-list");
    if (!list.length) return;

    if (ajaxErrors.length === 0) {
        list.html(
            '<div class="ev_error_list_empty">' +
                escapeHTML(list.data("emptyLabel") || "No recent errors") +
                "</div>",
        );
        return;
    }

    const items = ajaxErrors
        .map(function (entry) {
            return (
                '<div class="ev_error_entry">' +
                '<div class="ev_error_entry_time">' +
                escapeHTML(entry.time) +
                "</div>" +
                '<div class="ev_error_entry_message">' +
                escapeHTML(entry.message) +
                "</div>" +
                "</div>"
            );
        })
        .join("");
    list.html(items);
}

function setErrorButtonAlert(active) {
    const button = $("#link-errors");
    if (!button.length) return;
    button.toggleClass("ev_button_error", active);
}

function isErrorListOpen() {
    return !$("#error-list").prop("hidden");
}

function closeErrorList() {
    const button = $("#link-errors");
    const list = $("#error-list");
    if (!button.length || !list.length || list.prop("hidden")) return;

    list.prop("hidden", true);
    button.attr("aria-expanded", "false");
}

function openErrorList() {
    const button = $("#link-errors");
    const list = $("#error-list");
    if (!button.length || !list.length) return;

    renderAJAXErrors();
    list.prop("hidden", false);
    button.attr("aria-expanded", "true");
    setErrorButtonAlert(false);
}

function toggleErrorList() {
    if (isErrorListOpen()) closeErrorList();
    else openErrorList();
}

function notifyAJAXError(message) {
    const msg = message || errorListLabel("unknownLabel", "Unknown error");
    ajaxErrors.unshift({
        time: new Date().toLocaleTimeString(),
        message: msg,
    });
    ajaxErrors = ajaxErrors.slice(0, MAX_AJAX_ERRORS);
    renderAJAXErrors();

    const button = $("#link-errors");
    if (button.length) {
        if (!isErrorListOpen()) setErrorButtonAlert(true);
        button.removeClass("ev_error_shake");
        button[0].offsetWidth;
        button.addClass("ev_error_shake");
    }

    console.error(msg);
}

$(function () {
    $(document).on("click", "#link-errors", function (e) {
        e.preventDefault();
        e.stopPropagation();
        toggleErrorList();
    });

    $(document).on("mousedown", function (e) {
        const button = document.getElementById("link-errors");
        const list = document.getElementById("error-list");
        if (!button || !list || list.hidden) return;
        if (button.contains(e.target) || list.contains(e.target)) return;
        closeErrorList();
    });

    $(document).on("keydown", function (e) {
        if (e.key === "Escape") closeErrorList();
    });

    $(document).on("animationend", "#link-errors", function () {
        $(this).removeClass("ev_error_shake");
    });

    renderAJAXErrors();
});
