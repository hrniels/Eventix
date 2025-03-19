var selected = null;

$(document).click(function(e) {
    if(selected != null && !inBoundingBox(e, 'popup'))
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
    var newid = {
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
    var e = $('.' + newid.jsuid);
    e.addClass('ev_selected');

    var el = document.getElementById(newid.id);
    var elRect = pageBoundingBox(el);

    var popup = $('#popup');
    var popWidth = 500;
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
    var el = document.getElementById(id);
    var elRect = pageBoundingBox(el);
    var popup = $('#popup');
    var popupRect = pageBoundingBox(document.getElementById('popup'));
    if(elRect.top + popupRect.height > window.innerHeight && elRect.bottom >= popupRect.height)
        popup.css('top', elRect.bottom - popupRect.height);
}

function deselect(oldid, newid) {
    selected = null;
    $('#popup').slideFadeToggle(function() {
        var old = $('.' + oldid.jsuid);
        old.removeClass('ev_selected');
        if(newid != null)
            select(newid);
    });
}

function pageBoundingBox(el) {
    var rect = el.getBoundingClientRect();
    var doc = document.documentElement;
    var left = (window.pageXOffset || doc.scrollLeft) - (doc.clientLeft || 0);
    var top = (window.pageYOffset || doc.scrollTop)  - (doc.clientTop || 0);
    rect.x += left;
    rect.y += top;
    rect.top += top;
    rect.bottom += top;
    rect.left += left;
    rect.right += left;
    return rect;
}

function inBoundingBox(e, id) {
    var box = pageBoundingBox(document.getElementById(id));
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

