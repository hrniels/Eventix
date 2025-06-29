const POPUP_SPEED = 100;
const RESIZE_SPEED = 200;

class State {
    constructor(name) {
        this.name = name;
    }
}

class InitState extends State {
    constructor() {
        super("init");
    }
}

class SmallState extends State {
    constructor(ids) {
        super("small");
        this.ids = ids;
    }
}

class LargeState extends State {
    constructor(ids, popup_pos) {
        super("large");
        this.ids = ids;
        this.popup_pos = popup_pos;
    }
}

class HelpState extends State {
    constructor() {
        super("help");
    }
}

class Event {
    constructor(name) {
        this.name = name;
    }

    async trigger(state) {
    }
}

class SelectEvent extends Event {
    constructor(uid, jsuid, rid, id) {
        super("select");
        this.data = {
            'uid': uid,
            'jsuid': jsuid,
            'rid': rid,
            'id': id,
        };
    }

    async trigger(state) {
        switch(state.name) {
            case "init":
                await _select(this.data);
                return new SmallState(this.data);

            default:
                return state;
        }
    }
}

class DeselectEvent extends Event {
    constructor() {
        super("deselect");
    }

    async trigger(state) {
        switch(state.name) {
            case "small":
                await _deselect(state.ids);
                return new InitState();

            case "help":
            case "large":
                if(state.popup_pos != null) {
                    await _shrinkPopup(state.popup_pos);
                    await _deselect(state.ids);
                }
                else
                    await _deselect(state.ids);
                _animateBlur(0);
                return new InitState();

            default:
                return state;
        }
    }
}

class EditEvent extends Event {
    constructor(id, uid, rid) {
        super("edit");
        this.data = {
            'uid': uid,
            'rid': rid,
            'id': id,
        };
    }

    async trigger(state) {
        switch(state.name) {
            case "init":
                await _openLargePopup(this.data);
                return new LargeState(null, null);

            case "small":
                let popup_pos = {
                    'top': $('#popup').css('top'),
                    'left': $('#popup').css('left'),
                    'width': $('#popup').width(),
                };
                await _animateOpenPopup();
                return new LargeState(state.ids, popup_pos);

            case "help":
                console.assert(false, "This should not happen");

            default:
                return state;
        }
    }
}

class CancelEvent extends Event {
    constructor() {
        super("cancel");
    }

    async trigger(state) {
        switch(state.name) {
            case "help":
            case "large":
                let new_state;
                if(state.popup_pos != null) {
                    await _shrinkPopup(state.popup_pos);
                    new_state = new SmallState(state.ids);
                }
                else {
                    await _deselect(state.ids);
                    new_state = new InitState();
                }
                _animateBlur(0);
                return new_state;

            default:
                return state;
        }
    }
}

class HelpEvent extends Event {
    constructor() {
        super("help");
    }

    async trigger(state) {
        switch(state.name) {
            case "init":
                await _openHelpPopup();
                return new HelpState();

            case "small":
            case "large":
                await _loadHelp();
                await _animateOpenPopup();
                return new HelpState();

            default:
                return state;
        }
    }
}

let state = new InitState();
let queue = [];

async function fireEvent(ev) {
    queue.push(ev);
    // if the state is null, we are already processing an event
    while(state != null && queue.length > 0) {
        let ev = queue.shift();
        let cur_state = state;
        // mark us as busy until the future finishes
        state = null;
        state = await ev.trigger(cur_state);
    }
}

$(document).mousedown(function(e) {
    let popup = document.getElementById('popup');
    if(!popup.contains(e.target) && !_inBoundingBox(e, 'popup'))
        fireEvent(new DeselectEvent());
});
$(document).keydown(function(e) {
    if(e.key == 'Escape')
        fireEvent(new DeselectEvent());
});

$.fn.slideFadeToggle = function(easing, callback) {
    return this.animate({ opacity: 'toggle' }, POPUP_SPEED, easing, callback);
};

function _setBlur(el, radius) {
    $(el).css({
        "-webkit-filter": "blur(" + radius + "px)",
        "-moz-filter": "blur(" + radius + "px)",
        "-o-filter": "blur(" + radius + "px)",
        "-ms-filter": "blur(" + radius + "px)",
        "filter": "blur(" + radius + "px)"
    });
}

function _animateBlur(radius) {
    $("#outer").animate({blurRadius: radius}, {
        duration: RESIZE_SPEED,
        easing: 'linear',
        step: function() {
            _setBlur("#outer", this.blurRadius);
        },
        complete: function() {
            _setBlur("#outer", radius);
        }
    });
}

async function _animateOpenPopup() {
    await new Promise(function(resolve) {
        _animateBlur(10);

        const distance = 200;
        const old_width = $('#popup').width();
        const width = Math.min(1024, $(window).width() - distance * 2);
        // set the width temporarily to get the final height of the popup below
        $('#popup').css('display', 'block');
        $('#popup').css('width', width + 'px');

        const doc = document.documentElement;
        const yoff = (window.pageYOffset || doc.scrollTop)  - (doc.clientTop || 0);
        const height = document.getElementById('popup').getBoundingClientRect().height;
        const top = height > $(window).height() ? distance : ($(window).height() - height) / 2;
        const left = ($(window).width() - width) / 2;

        $('#popup').css('width', old_width + 'px');
        $('#popup').animate({
            left: left + 'px',
            top: (yoff + top) + 'px',
            width: width + 'px',
            opacity: 100,
        }, RESIZE_SPEED, 'swing', () => resolve());
    });
}

async function _openLargePopup(el) {
    await new Promise(async function(resolve) {
        let button = $('#' + el.id);
        $('#popup').animate({
            left: button.offset().left + 'px',
            top: button.offset().top + 'px',
            width: button.width() + 'px',
        }, 10);

        await _loadOccurrence(el.uid, el.rid, true);
        setTimeout(async () => {
            await _animateOpenPopup();
            resolve();
        }, 10);
    });
}

async function _openHelpPopup(el) {
    await new Promise(async function(resolve) {
        let button = $('#link-help');
        $('#popup').animate({
            left: button.offset().left + 'px',
            top: button.offset().top + 'px',
            width: button.width() + 'px',
        }, 10);

        await _loadHelp();
        setTimeout(async () => {
            await _animateOpenPopup();
            resolve();
        }, 10);
    });
}

async function _shrinkPopup(pos) {
    await new Promise(function(resolve) {
        $('#popup').animate({
            left: pos['left'],
            top: pos['top'],
            width: pos['width'] + 'px',
        }, RESIZE_SPEED, function() {
            resolve();
        });
    });
}

async function _select(newid) {
    await new Promise(async function(resolve) {
        $('#' + newid.id).addClass('ev_current');
        $('.' + newid.jsuid).addClass('ev_selected');
        setPopupOpen(true);

        let el = document.getElementById(newid.id);
        const elRect = _pageBoundingBox(el);
        const popWidth = 600;

        let popup = $('#popup');
        if(elRect.right + popWidth > window.innerWidth)
            popup.css('left', elRect.left - popWidth);
        else
            popup.css('left', elRect.right);
        popup.css('width', popWidth + 'px');
        popup.css('top', elRect.top);
        popup.css("position", "absolute");
        popup.slideFadeToggle();

        await _loadOccurrence(newid.uid, newid.rid, false);
        setTimeout(() => {
            _correctPosition(newid.id);
            resolve();
        }, 10);
    });
}

function _correctPosition(id) {
    let el = document.getElementById(id);
    const elRect = _pageBoundingBox(el);
    const popupRect = _pageBoundingBox(document.getElementById('popup'));
    let popup = $('#popup');
    if(elRect.top + popupRect.height > window.innerHeight && elRect.bottom >= popupRect.height)
        popup.css('top', elRect.bottom - popupRect.height);
}

async function _deselect(oldid) {
    await new Promise(function(resolve) {
        $('#popup').slideFadeToggle(function() {
            if(oldid) {
                $('#' + oldid.id).removeClass('ev_current');
                $('.' + oldid.jsuid).removeClass('ev_selected');
            }
            setPopupOpen(false);
            resolve();
        });
    });
}

async function _loadOccurrence(uid, rid, edit) {
    let url = '/details?uid=' + uid + '&edit=' + (edit ? 'true' : 'false');
    if(rid)
        url += '&rid=' + rid;
    await _loadPage(url);
}

async function _loadHelp() {
    await _loadPage('/help');
}

async function _loadPage(url) {
    await new Promise(function(resolve) {
        getRequest(
            url,
            function(data) {
                $('#popup').html(data.html);
                resolve();
            }
        );
    });
}

function _pageBoundingBox(el) {
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

function _inBoundingBox(e, id) {
    const box = _pageBoundingBox(document.getElementById(id));
    return e.pageX >= box.left && e.pageX <= box.left + box.width &&
        e.pageY >= box.top && e.pageY <= box.top + box.height;
}
