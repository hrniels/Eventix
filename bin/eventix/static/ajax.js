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
    let url = '/complete?uid=' + uid;
    if(rid)
        url += '&rid=' + rid;
    getRequest(url, onsuccess);
}

function toggleExcl(uid, rid, onsuccess) {
    postRequest('/toggleexcl?uid=' + uid + '&rid=' + rid, function(data) {
        onsuccess(data);
    });
}

function moveEvent(uid, rid, date, hour, onsuccess) {
    let url = '/moveevent?uid=' + uid+ '&date=' + date;
    if(rid)
        url += '&rid=' + rid;
    if(hour)
        url += '&hour=' + hour;
    postRequest(url, function(data) {
        onsuccess(data);
    });
}

function cancelOcc(uid, rid, onsuccess) {
    postRequest('/cancel?uid=' + uid + '&rid=' + rid, onsuccess);
}

function changePartStat(uid, rid, stat, onsuccess) {
    let url = '/setpartstat?stat=' + stat + '&uid=' + uid;
    if(rid)
        url += '&rid=' + rid;
    postRequest(url, function(data) {
        onsuccess(data);
    });
}

function deleteItem(uid, rid, onDeleted) {
    let url = '/delete?uid=' + uid;
    if(rid != null)
        url += '&rid=' + rid;
    postRequest(url, onDeleted);
}

function toggleCalendar(id) {
    postRequest('/toggle-calendar?id=' + id, reloadPage);
}
