function handleAJAXError(jqXHR, textStatus, errorThrown) {
    const msg = jqXHR.responseJSON.error || "Unknown error";
    console.log(msg);
}

function getRequest(url, success, type = "json") {
    $.ajax({
        type: "GET",
        url: url,
        dataType: type,
        success: success,
        error: handleAJAXError,
    });
}

function postRequest(url, success) {
    $.ajax({
        type: "POST",
        url: url,
        dataType: "json",
        success: function (data) {
            success(data);
            reloadSidebar();
        },
        error: handleAJAXError,
    });
}

function formRequest(id, success) {
    const form = $("#" + id);
    $.ajax({
        url: form.attr("action"),
        type: form.attr("method"),
        data: form.serialize(),
        success: function (data) {
            success(data);
            reloadSidebar();
        },
        error: handleAJAXError,
    });
}

function completeTodo(uid, rid, onsuccess) {
    let url = "/api/items/complete?uid=" + uid;
    if (rid) url += "&rid=" + rid;
    postRequest(url, onsuccess);
}

function toggleExcl(uid, rid, onsuccess) {
    postRequest("/api/items/toggle?uid=" + uid + "&rid=" + rid, function (data) {
        onsuccess(data);
    });
}

function moveEvent(uid, rid, date, hour, onsuccess) {
    let url = "/api/items/shift?uid=" + uid + "&date=" + date;
    if (rid) url += "&rid=" + rid;
    if (hour) url += "&hour=" + hour;
    postRequest(url, function (data) {
        onsuccess(data);
    });
}

function resizeEvent(uid, rid, startHour, startMinute, endHour, endMinute, onsuccess) {
    let url = "/api/items/resize?uid=" + uid;
    if (rid) url += "&rid=" + rid;
    if (startHour != null) url += "&start_hour=" + startHour + "&start_minute=" + startMinute;
    if (endHour != null) url += "&end_hour=" + endHour + "&end_minute=" + endMinute;
    postRequest(url, function (data) {
        onsuccess(data);
    });
}

function copyEvent(uid, date, hour, onsuccess) {
    let url = "/api/items/copy?uid=" + uid + "&date=" + date;
    if (hour) url += "&hour=" + hour;
    postRequest(url, function (data) {
        onsuccess(data);
    });
}

function cancelOcc(uid, rid, onsuccess) {
    postRequest("/api/items/cancel?uid=" + uid + "&rid=" + rid, onsuccess);
}

function respond(uid, rid, stat, onsuccess) {
    let url = "/api/items/respond?stat=" + stat + "&uid=" + uid;
    if (rid) url += "&rid=" + rid;
    postRequest(url, function (data) {
        onsuccess(data);
    });
}

function deleteItem(uid, rid, onDeleted) {
    let url = "/api/items/delete?uid=" + uid;
    if (rid != null) url += "&rid=" + rid;
    postRequest(url, onDeleted);
}

function toggleCalendar(id) {
    postRequest("/api/togglecal?id=" + id, function (data) {
        if (!userOnForm()) reloadContent();
        reloadCalList();
    });
}

function calendarOperation(col_id, cal_id, op, onsuccess) {
    const url = "/api/calendars/calop?col_id=" + col_id + "&cal_id=" + cal_id + "&op=" + op;
    postRequest(url, onsuccess);
}

function deleteRemoteCalendar(col_id, folder, spinnerId, onsuccess) {
    const url =
        "/api/calendars/calop?col_id=" +
        encodeURIComponent(col_id) +
        "&folder=" +
        encodeURIComponent(folder) +
        "&op=Delete";
    postWithSpinner(spinnerId, url, function (data, auth_success) {
        if (auth_success) onsuccess(data);
    });
}

function deleteCollection(col_id, onDeleted) {
    postRequest("/api/collections/delete?col_id=" + col_id, onDeleted);
}

function setLang(lang) {
    postRequest("/api/setlang?lang=" + lang, reloadPage);
}
