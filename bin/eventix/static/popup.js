let selected = null;

$(document).mousedown(function(e) {
    let popup = document.getElementById('popup');
    if(selected != null && !popup.contains(e.target) && !inBoundingBox(e, 'popup'))
        deselect(selected, null);
});
$(document).keydown(function(e) {
    if(e.key == 'Escape' && selected != null)
        deselect(selected, null);
});

$.fn.slideFadeToggle = function(easing, callback) {
    return this.animate({ opacity: 'toggle' }, 100, easing, callback);
};

function openPopup(uid, jsuid, rid, id) {
    const newid = {
        'uid': uid,
        'jsuid': jsuid,
        'rid': rid,
        'id': id,
    };
    if(selected != null)
        deselect(selected, newid);
    else
        select(newid);
}

function select(newid) {
    $('#' + newid.id).addClass('ev_current');
    $('.' + newid.jsuid).addClass('ev_selected');
    setPopupOpen(true);

    let el = document.getElementById(newid.id);
    const elRect = pageBoundingBox(el);
    const popWidth = 600;

    let popup = $('#popup');
    if(elRect.right + popWidth > window.innerWidth)
        popup.css('left', elRect.left - popWidth);
    else
        popup.css('left', elRect.right);
    popup.css('top', elRect.top);
    popup.css("position", "absolute");
    popup.slideFadeToggle(null, function() {
        selected = newid;
    });

    loadOccurrence(newid.id, newid.uid, newid.rid);
}

function correctPosition(id) {
    let el = document.getElementById(id);
    const elRect = pageBoundingBox(el);
    const popupRect = pageBoundingBox(document.getElementById('popup'));
    let popup = $('#popup');
    if(elRect.top + popupRect.height > window.innerHeight && elRect.bottom >= popupRect.height)
        popup.css('top', elRect.bottom - popupRect.height);
}

function deselect(oldid, newid) {
    selected = null;
    $('#popup').slideFadeToggle(function() {
        $('#' + oldid.id).removeClass('ev_current');
        $('.' + oldid.jsuid).removeClass('ev_selected');
        setPopupOpen(false);
        if(newid != null)
            select(newid);
    });
}

function pageBoundingBox(el) {
    let rect = el.getBoundingClientRect();
    const doc = document.documentElement;
    const left = (window.pageXOffset || doc.scrollLeft) - (doc.clientLeft || 0);
    const top = (window.pageYOffset || doc.scrollTop)  - (doc.clientTop || 0);
    rect.x += left;
    rect.y += top;
    rect.top += top;
    rect.bottom += top;
    rect.left += left;
    rect.right += left;
    return rect;
}

function inBoundingBox(e, id) {
    const box = pageBoundingBox(document.getElementById(id));
    return e.pageX >= box.left && e.pageX <= box.left + box.width &&
        e.pageY >= box.top && e.pageY <= box.top + box.height;
}

function loadOccurrence(id, uid, rid) {
    getRequest(
        '/details?uid=' + uid + (rid ? '&rid=' + rid : ''),
        function(data) {
            $('#popup').html(data.html);
            setTimeout(function() {
                correctPosition(id)
            }, 10);
        }
    );
}

