function reloadPage() {
    window.location.reload();
}

function curUTCDay() {
    var date = new Date();
    var y = date.getUTCFullYear();
    var m = (date.getUTCMonth() + 1).toString().padStart(2, '0');
    var d = date.getUTCDate().toString().padStart(2, '0');
    return `${y}${m}${d}`;
}

function curISODate() {
    var date = new Date();
    var y = date.getUTCFullYear();
    var m = (date.getUTCMonth() + 1).toString().padStart(2, '0');
    var d = date.getUTCDate().toString().padStart(2, '0');
    var H = date.getUTCHours().toString().padStart(2, '0');
    var M = date.getUTCMinutes().toString().padStart(2, '0');
    var S = date.getUTCSeconds().toString().padStart(2, '0');
    return `${y}${m}${d}T${H}${M}${S}Z`;
}

function invertSelection(prefix) {
    for(var i = 1;;i++)
    {
        var checkbox = document.getElementById(prefix + i);
        if(checkbox == null)
            break;

        checkbox.checked = checkbox.checked ? false : true;
    }
}

function toggleCheckbox(id) {
    var el = document.getElementById(id);
    el.checked = !el.checked;
}

function moveToTabCenter(elId, tabsId, tabBarId) {
    var pos = $('#' + tabsId).position().top;
    var tab = $('#' + tabBarId);
    var el = $('#' + elId);
    el.css({
        position: "absolute",
        top: (pos +
            (tab.outerHeight() / 2) - (el.outerHeight() / 2)) + "px",
        left: ((tab.outerWidth() / 2) - (el.outerWidth() / 2)) + "px"
    }).show();
}

function addArrowToDatePicker(input, inst) {
    // move the datepicker a bit down so that we can draw the arrow on top
    inst.dpDiv.css({
        marginTop: '10px',
    });
    inst.dpDiv.addClass('popup');
}

function hideArrowBottom(inst) {
    // ensure that the datepicker header is above the arrow
    $('.ui-datepicker-header').css('zIndex', 2);
}

function getRequest(url, success) {
    $.ajax({
        type: 'GET',
        url: url,
        dataType: 'json',
        success: success,
    });
}

function postRequest(url, success) {
    $.ajax({
        type: 'POST',
        url: url,
        dataType: 'json',
        success: success,
    });
}

function completeTodo(uid, rid, onsuccess) {
    var url = '/complete?uid=' + uid;
    if(rid != null)
        url += '&rid=' + rid;
    getRequest(url, onsuccess);
}

function toggleExcl(uid, rid, onsuccess) {
    postRequest('/toggleexcl?uid=' + uid + '&rid=' + rid, function(data) {
        onsuccess(data);
    });
}

function deleteItem(uid, rid, onDeleted) {
    var url = '/delete?uid=' + uid;
    if(rid != null)
        url += '&rid=' + rid;
    postRequest(url, onDeleted);
}

function toggleCalendar(id) {
    postRequest('/toggle-calendar?id=' + id, reloadPage);
}

function setPersonalOverwrite(id_prefix, overwrite) {
    var ids = ["none", "relative", "absolute", "durunit_", "durtype_", "datetime__time"];
    for(id in ids) {
        $("#" + id_prefix + "_" + ids[id]).prop("disabled", !overwrite);
    }
    $("#" + id_prefix + "_duration").spinner(overwrite ? "enable" : "disable");
    $("#" + id_prefix + "_datetime__date_").datepicker("option", "disabled", !overwrite);
}
