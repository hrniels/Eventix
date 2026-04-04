const POPUP_SPEED = 50;
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

class PageState extends State {
    constructor(url) {
        super("page");
        this.url = url;
    }
}

class Event {
    constructor(name) {
        this.name = name;
    }

    async trigger(state) {}
}

class SelectEvent extends Event {
    constructor(uid, jsuid, rid, id, clickEv) {
        super("select");
        const doc = document.documentElement;
        const scrollTop = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
        this.data = {
            uid: uid,
            jsuid: jsuid,
            rid: rid,
            id: id,
            // Page-absolute Y of the click; used as fallback anchor when the event element
            // extends outside the visible viewport.
            clickPageY: clickEv ? clickEv.clientY + scrollTop : null,
        };
    }

    async trigger(state) {
        switch (state.name) {
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
        switch (state.name) {
            case "small":
                await _deselect(state.ids);
                return new InitState();

            case "page":
            case "large":
                if (state.popup_pos != null) {
                    await _shrinkPopup(state.popup_pos);
                    await _deselect(state.ids);
                } else await _deselect(state.ids);
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
            uid: uid,
            rid: rid,
            id: id,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
                await _openLargePopup(this.data);
                return new LargeState(null, null);

            case "small":
                let popup_pos = {
                    top: $("#popup").css("top"),
                    left: $("#popup").css("left"),
                    width: $("#popup").width(),
                };
                await _animateOpenPopup();
                return new LargeState(state.ids, popup_pos);

            case "page":
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
        switch (state.name) {
            case "page":
            case "large":
                let new_state;
                if (state.popup_pos != null) {
                    await _shrinkPopup(state.popup_pos);
                    new_state = new SmallState(state.ids);
                } else {
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

class PageEvent extends Event {
    constructor(url) {
        super("page");
        this.data = {
            url: url,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
                await _openPagePopup(this.data["url"]);
                return new PageState(this.data["url"]);

            case "small":
            case "large":
                await _loadPage(this.data["url"]);
                await _animateOpenPopup();
                return new PageState(this.data["url"]);

            default:
                return state;
        }
    }
}

function createHelpEvent() {
    return new PageEvent("/api/help");
}

function createAuthEvent(cal, url, op_url, spinnerId) {
    return new PageEvent(
        "/api/auth?calendar=" +
            cal +
            "&url=" +
            encodeURIComponent(url) +
            "&op_url=" +
            encodeURIComponent(op_url) +
            "&spinner_id=" +
            encodeURIComponent(spinnerId),
    );
}

let state = new InitState();
let queue = [];

async function fireEvent(ev) {
    queue.push(ev);
    // if the state is null, we are already processing an event
    while (state != null && queue.length > 0) {
        let ev = queue.shift();
        let cur_state = state;
        // mark us as busy until the future finishes
        state = null;
        state = await ev.trigger(cur_state);
    }
}

$(document).mousedown(function (e) {
    let popup = document.getElementById("popup");
    if (!popup.contains(e.target) && !_inBoundingBox(e, "popup")) fireEvent(new DeselectEvent());
});
$(document).keydown(function (e) {
    if (e.key == "Escape") fireEvent(new DeselectEvent());
});

$.fn.slideFadeToggle = function (easing, callback) {
    return this.animate({ opacity: "toggle" }, POPUP_SPEED, easing, callback);
};

function _setBlur(el, radius) {
    $(el).css({
        "-webkit-filter": "blur(" + radius + "px)",
        "-moz-filter": "blur(" + radius + "px)",
        "-o-filter": "blur(" + radius + "px)",
        "-ms-filter": "blur(" + radius + "px)",
        filter: "blur(" + radius + "px)",
    });
}

function _animateBlur(radius) {
    $("#outer").animate(
        { blurRadius: radius },
        {
            duration: RESIZE_SPEED,
            easing: "linear",
            step: function () {
                _setBlur("#outer", this.blurRadius);
            },
            complete: function () {
                _setBlur("#outer", radius);
            },
        },
    );
}

async function _animateOpenPopup() {
    await new Promise(function (resolve) {
        _animateBlur(10);

        const distance = 200;
        const old_width = $("#popup").width();
        const width = Math.min(1024, $(window).width() - distance * 2);
        // set the width temporarily to get the final height of the popup below
        $("#popup").css("display", "block");
        $("#popup").css("width", width + "px");

        const doc = document.documentElement;
        const yoff = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
        const height = document.getElementById("popup").getBoundingClientRect().height;
        const top = height > $(window).height() ? distance : ($(window).height() - height) / 2;
        const left = ($(window).width() - width) / 2;

        $("#popup").css("width", old_width + "px");
        $("#popup").animate(
            {
                left: left + "px",
                top: yoff + top + "px",
                width: width + "px",
                opacity: 100,
            },
            RESIZE_SPEED,
            "swing",
            () => resolve(),
        );
    });
}

async function _openLargePopup(el) {
    await _openFromElement("#" + el.id, async function () {
        await _loadOccurrence(el.uid, el.rid, true);
    });
}

async function _openPagePopup(url) {
    await _openFromElement("#link-refresh", async function () {
        await _loadPage(url);
    });
}

async function _openFromElement(id, func) {
    // remove old content
    $("#popup").html('<div style="height: 300px"></div>');

    await new Promise(async function (resolve) {
        let button = $(id);
        $("#popup").animate(
            {
                left: button.offset().left + "px",
                top: button.offset().top + "px",
                width: button.width() + "px",
            },
            10,
        );

        await func();
        setTimeout(async () => {
            await _animateOpenPopup();
            resolve();
        }, 10);
    });
}

async function _shrinkPopup(pos) {
    await new Promise(function (resolve) {
        $("#popup").animate(
            {
                left: pos["left"],
                top: pos["top"],
                width: pos["width"] + "px",
            },
            RESIZE_SPEED,
            function () {
                resolve();
            },
        );
    });
}

// Returns the page-absolute Y coordinate at which the small popup should be anchored (its top
// edge) before the popup height is known. A precise correction is applied later in
// _correctPosition() once the content has loaded and the popup height can be measured.
//
// Strategy:
//   - If the event's top edge is visible, use it as-is (normal case).
//   - If the event starts above the viewport, use the viewport top as a temporary anchor;
//     _correctPosition() will shift the popup to align its bottom with the event's bottom edge.
//   - If the event is entirely outside the viewport (defensive fallback), use the click position.
function _visibleAnchorTop(elRect, clickPageY) {
    const doc = document.documentElement;
    const scrollTop = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
    const viewTop = scrollTop;
    const viewBottom = scrollTop + window.innerHeight;

    const eventVisibleAtTop = elRect.top >= viewTop && elRect.top < viewBottom;
    const eventSpansViewport = elRect.top < viewTop && elRect.bottom > viewBottom;
    const eventVisibleAtBottom = elRect.bottom > viewTop && elRect.bottom <= viewBottom;

    if (eventVisibleAtTop) {
        // Normal case: the event starts within the visible area.
        return elRect.top;
    } else if (eventSpansViewport || eventVisibleAtBottom) {
        // Event top is above the viewport; use viewport top as a temporary position.
        // _correctPosition() will refine this to bottom-align with the event's bottom edge.
        return viewTop;
    } else if (clickPageY !== null) {
        // Entire event is outside the viewport (defensive fallback): use click position.
        return clickPageY;
    }
    return elRect.top;
}

async function _select(newid) {
    await new Promise(async function (resolve) {
        $("#" + newid.id).addClass("ev_current");
        $("." + newid.jsuid).addClass("ev_selected");
        setPopupOpen(true);

        let el = document.getElementById(newid.id);
        const elRect = _pageBoundingBox(el);
        const popWidth = 600;

        let popup = $("#popup");
        if (elRect.right + popWidth > window.innerWidth) popup.css("left", elRect.left - popWidth);
        else popup.css("left", elRect.right);
        popup.css("width", popWidth + "px");
        popup.css("top", _visibleAnchorTop(elRect, newid.clickPageY));
        popup.css("position", "absolute");
        popup.slideFadeToggle();

        await _loadOccurrence(newid.uid, newid.rid, false);
        setTimeout(() => {
            _correctPosition(newid.id);
            resolve();
        }, 10);
    });
}

// Adjusts the popup's vertical position after its content has loaded and its final height is known.
//
// Cases handled (all comparisons in page-absolute coordinates):
//   - Top clipped, bottom visible: align the popup bottom with the event's bottom edge.
//   - Top visible, popup overflows viewport bottom: shift the popup upward, clamping its bottom to
//     the event's bottom edge or the viewport bottom, whichever is higher.
//   - In both cases, ensure the popup never goes above the current scroll top.
function _correctPosition(id) {
    let el = document.getElementById(id);
    const elRect = _pageBoundingBox(el);
    const popupRect = _pageBoundingBox(document.getElementById("popup"));
    const doc = document.documentElement;
    const scrollTop = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
    const viewTop = scrollTop;
    const viewBottom = scrollTop + window.innerHeight;

    const topClipped = elRect.top < viewTop;
    const bottomVisible = elRect.bottom > viewTop && elRect.bottom <= viewBottom;

    let top = parseFloat($("#popup").css("top"));

    if (topClipped && bottomVisible) {
        // Align the popup bottom with the visible bottom edge of the event.
        top = elRect.bottom - popupRect.height;
    } else if (top + popupRect.height > viewBottom) {
        // Popup overflows the bottom of the viewport: shift upward.
        // Prefer aligning with the event's bottom edge; fall back to the viewport bottom.
        const anchor = elRect.bottom > viewBottom ? viewBottom : elRect.bottom;
        top = anchor - popupRect.height;
    }

    // Ensure we do not push the popup above the current scroll top.
    top = Math.max(top, scrollTop);
    $("#popup").css("top", top);
}

async function _deselect(oldid) {
    await new Promise(function (resolve) {
        $("#popup").slideFadeToggle(function () {
            if (oldid) {
                $("#" + oldid.id).removeClass("ev_current");
                $("." + oldid.jsuid).removeClass("ev_selected");
            }
            setPopupOpen(false);
            resolve();
        });
    });
}

async function _loadOccurrence(uid, rid, edit) {
    let url = "/api/items/details?uid=" + uid + "&edit=" + (edit ? "true" : "false");
    if (rid) url += "&rid=" + rid;
    await _loadPage(url);
}

async function _loadPage(url) {
    await new Promise(function (resolve) {
        getRequest(url, function (data) {
            $("#popup").html(data.html);
            resolve();
        });
    });
}

function _pageBoundingBox(el) {
    let rect = el.getBoundingClientRect();
    const doc = document.documentElement;
    const left = (window.pageXOffset || doc.scrollLeft) - (doc.clientLeft || 0);
    const top = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
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
    return (
        e.pageX >= box.left &&
        e.pageX <= box.left + box.width &&
        e.pageY >= box.top &&
        e.pageY <= box.top + box.height
    );
}
