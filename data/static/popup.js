const POPUP_SPEED = 50;
const RESIZE_SPEED = 200;

const MAX_HEIGHT_EDIT = 500;
const HEIGHT_ADD_EVENT = 439;
const HEIGHT_ADD_TODO = 494;
const WIDTH_LOG = 800;
const WIDTH_HELP = 1024;
const WIDTH_AUTH = 600;
const WIDTH_DETAILS = 600;
const HEIGHT_LOG = 474;
const HEIGHT_HELP = 729;
const HEIGHT_AUTH = 200;
const ALARMS_HEIGHT = 100;
const WIDTH_COLLECTION = 700;
const HEIGHT_ADD_COLLECTION = 515;
const HEIGHT_EDIT_COLLECTION = 505;

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

class FormState extends State {
    constructor(ids, popup_pos, details_url) {
        super("form");
        this.ids = ids;
        this.popup_pos = popup_pos;
        this.details_url = details_url;
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
            case "page":
            case "form":
            case "large":
                await _deselect(state.ids);
                if (state.name == "page") _closePageLayer();
                return new InitState();

            default:
                return state;
        }
    }
}

class EditAlarmsEvent extends Event {
    constructor(btnid, ctype, uid, rid) {
        super("editalarms");
        this.data = {
            btnid: btnid,
            ctype: ctype,
            uid: uid,
            rid: rid,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
                let url = "/api/items/details?uid=" + this.data.uid;
                if (this.data.rid) url += "&rid=" + this.data.rid;
                url += "&edit=true";
                await _openEditAlarmsPopup(this.data, url);
                return new LargeState(state.ids, null);

            case "small":
                let popup_pos = _animateExpandPopup("alarms");
                return new LargeState(state.ids, popup_pos);

            case "page":
                console.assert(false, "This should not happen");

            default:
                return state;
        }
    }
}

class AddEvent extends Event {
    constructor(btnid, ctype, date, hour) {
        super("add");
        this.data = {
            btnid: btnid,
            ctype: ctype,
            date: date,
            hour: hour,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
            case "small":
                if (state.name === "small") {
                    await _deselect(state.ids);
                }
                await _openAddPopup(this.data);
                return new FormState(null, null, null);

            default:
                return state;
        }
    }
}

class EditEvent extends Event {
    constructor(btnid, ctype, uid, rid, mode = "Series") {
        super("edit");
        this.data = {
            btnid: btnid,
            ctype: ctype,
            uid: uid,
            rid: rid,
            id: null,
            mode: mode,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
            case "small":
                let popup_pos = state.name == "small" ? _animateExpandPopup("edit") : null;

                let url = "/api/items/edit?uid=" + this.data.uid;
                if (this.data.rid) url += "&rid=" + this.data.rid;
                url += "&mode=" + this.data.mode;

                if (state.name == "small") await _loadPage(url);
                else await _openEditPopup(this.data, url);

                return new FormState(state.ids, popup_pos);

            case "page":
                console.assert(false, "This should not happen");

            default:
                return state;
        }
    }
}

class AddCollectionEvent extends Event {
    constructor(btnid) {
        super("addcollection");
        this.data = {
            btnid: btnid,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
            case "small":
                if (state.name === "small") {
                    await _deselect(state.ids);
                }
                await _openAddCollectionPopup(this.data);
                return new FormState(null, null, null);

            default:
                return state;
        }
    }
}

class EditCollectionEvent extends Event {
    constructor(btnid, col_id) {
        super("editcollection");
        this.data = {
            btnid: btnid,
            col_id: col_id,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
            case "small":
                if (state.name === "small") {
                    await _deselect(state.ids);
                }
                await _openEditCollectionPopup(this.data);
                return new FormState(null, null, null);

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
            case "large":
            case "form":
                let new_state;
                if (state.popup_pos != null) {
                    if (state.name == "form") {
                        _shrinkPopup(state.popup_pos);
                        await _loadOccurrence(state.ids.uid, state.ids.rid, false);
                    } else await _shrinkPopup(state.popup_pos);
                    new_state = new SmallState(state.ids);
                } else {
                    await _deselect(state.ids);
                    new_state = new InitState();
                }
                return new_state;

            case "page":
                await _deselect(state.ids);
                _closePageLayer();
                return new InitState();

            default:
                return state;
        }
    }
}

class PageEvent extends Event {
    constructor(btnid, url, minWidth, heightEstimate) {
        super("page");
        this.data = {
            btnid: btnid,
            url: url,
            minWidth: minWidth,
            heightEstimate: heightEstimate,
        };
    }

    async trigger(state) {
        switch (state.name) {
            case "init":
                _openPageLayer(true);
                await _openPagePopup(this.data, this.data["url"]);
                return new PageState(this.data["url"]);

            case "small":
            case "large":
                _openPageLayer(true);
                await _loadPage(this.data["url"]);
                await _animateOpenPopup(this.data["minWidth"], this.data["heightEstimate"]);
                return new PageState(this.data["url"]);

            default:
                return state;
        }
    }
}

function createLogEvent(btnid, col) {
    return new PageEvent(btnid, "/api/collections/log?col_id=" + col, WIDTH_LOG, HEIGHT_LOG);
}

function createHelpEvent(btnid) {
    return new PageEvent(btnid, "/api/help", WIDTH_HELP, HEIGHT_HELP);
}

function createAuthEvent(cal, url, op_url, spinnerId) {
    return new PageEvent(
        "link-refresh",
        "/api/auth?calendar=" +
            cal +
            "&url=" +
            encodeURIComponent(url) +
            "&op_url=" +
            encodeURIComponent(op_url) +
            "&spinner_id=" +
            encodeURIComponent(spinnerId),
        WIDTH_AUTH,
        HEIGHT_AUTH,
    );
}

function createAddEvent(btnid, ctype, date, hour) {
    return new AddEvent(btnid, ctype, date, hour);
}

function createAddCollectionEvent(btnid) {
    return new AddCollectionEvent(btnid);
}

function createEditCollectionEvent(btnid, col_id) {
    return new EditCollectionEvent(btnid, col_id);
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
    if (e.target.closest(".ev_layer")) return;

    if (e.target.closest(".ui-datepicker")) return;
    if (e.target.closest(".clockpicker-popover")) return;
    if (e.target.closest(".ui-autocomplete")) return;

    let popup = document.getElementById("popup");
    if (!popup.contains(e.target) && !_inBoundingBox(e, "popup")) fireEvent(new DeselectEvent());
});
$(document).keydown(function (e) {
    if (e.isDefaultPrevented()) return;
    if (e.key == "Escape") {
        if ($(".ui-datepicker:visible").length > 0) return;
        if ($(".clockpicker-popover:visible").length > 0) return;
        if ($(".ui-autocomplete:visible").length > 0) return;
        fireEvent(new DeselectEvent());
    }
});

$.fn.slideFadeToggle = function (easing, callback) {
    return this.animate({ opacity: "toggle" }, POPUP_SPEED, easing, callback);
};

function _pageLayer() {
    return window.ev.getLayer("popup-layer");
}

function _openPageLayer(animate = false) {
    _pageLayer().open(function () {
        fireEvent(new DeselectEvent());
    }, animate);
}

function _closePageLayer() {
    _pageLayer().close();
}

async function _animateOpenPopup(minWidth, heightEstimate) {
    await new Promise(function (resolve) {
        const distance = 200;
        const width = Math.min(minWidth, $(window).width() - distance * 2);

        const doc = document.documentElement;
        const pgcontent = $("#page-content");
        const yoff = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
        const top =
            heightEstimate > $(window).height()
                ? distance
                : ($(window).height() - heightEstimate) / 2;
        const left = pgcontent.offset().left + (pgcontent.width() - width + 70) / 2;

        $("#popup").css("display", "block");
        $("#popup").css("opacity", "0%");

        $("#popup").animate(
            {
                left: left + "px",
                top: yoff + top + "px",
                width: width + "px",
                height: heightEstimate + "px",
                opacity: "100%",
            },
            RESIZE_SPEED,
            "swing",
            () => {
                $("#popup").css("height", "");
                resolve();
            },
        );
    });
}

function _animateExpandPopup(type) {
    let popup = $("#popup");
    let popup_pos = {
        top: popup.css("top"),
        left: popup.css("left"),
        width: popup.width(),
        height: popup.height(),
    };

    popup.css("overflow", "hidden");
    popup.css("maxHeight", popup.height());

    const estimatedHeight = type == "edit" ? MAX_HEIGHT_EDIT : popup.height() + ALARMS_HEIGHT;
    const doc = document.documentElement;
    const scrollTop = (window.pageYOffset || doc.scrollTop) - (doc.clientTop || 0);
    const viewBottom = scrollTop + window.innerHeight;
    const currentTop = parseFloat(popup.css("top"));
    const expandDown = currentTop + estimatedHeight < viewBottom;

    let animProps = { maxHeight: estimatedHeight + "px" };
    if (!expandDown) {
        animProps.top = currentTop - estimatedHeight + popup.height() + "px";
    }

    popup.animate(animProps, RESIZE_SPEED, "swing", () => {
        popup.css("max-height", "");
        popup.css("overflow", "");
        if (!expandDown) {
            const newTop = parseFloat(popup.css("top"));
            const newHeight = popup.height();
            const originalBottom = currentTop + popup_pos.height;
            popup.css("top", originalBottom - newHeight + "px");
        }
    });

    return popup_pos;
}

async function _openAddPopup(data) {
    const heightEstimate = data.ctype == "Event" ? HEIGHT_ADD_EVENT : HEIGHT_ADD_TODO;
    await _openFromElement("#" + data.btnid, 600, heightEstimate, async function () {
        let url = "/api/items/add?ctype=" + data.ctype;
        if (data.date) url += "&date=" + data.date;
        if (data.hour !== undefined && data.hour !== null) url += "&hour=" + data.hour;
        else url += "&allday=true";
        await _loadPage(url);
    });
}

async function _openEditPopup(data, url) {
    const heightEstimate = data.ctype == "Event" ? HEIGHT_ADD_EVENT : HEIGHT_ADD_TODO;
    await _openFromElement("#" + data.btnid, 600, heightEstimate, async function () {
        await _loadPage(url);
    });
}

async function _openEditAlarmsPopup(data, url) {
    let heightEstimate = data.ctype == "Event" ? HEIGHT_ADD_EVENT : HEIGHT_ADD_TODO;
    heightEstimate += ALARMS_HEIGHT;
    await _openFromElement("#" + data.btnid, 600, heightEstimate, async function () {
        await _loadPage(url);
    });
}

async function _openAddCollectionPopup(data) {
    await _openFromElement(
        "#" + data.btnid,
        WIDTH_COLLECTION,
        HEIGHT_ADD_COLLECTION,
        async function () {
            await _loadPage("/api/collections/add");
        },
    );
}

async function _openEditCollectionPopup(data) {
    await _openFromElement(
        "#" + data.btnid,
        WIDTH_COLLECTION,
        HEIGHT_EDIT_COLLECTION,
        async function () {
            await _loadPage("/api/collections/edit?col_id=" + encodeURIComponent(data.col_id));
        },
    );
}

async function _openPagePopup(data, url) {
    await _openFromElement("#" + data.btnid, data.minWidth, data.heightEstimate, async function () {
        await _loadPage(url);
    });
}

async function _openFromElement(id, minWidth, heightEstimate, func) {
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

        func();
        await _animateOpenPopup(minWidth, heightEstimate);
        resolve();
    });
}

async function _shrinkPopup(pos) {
    await new Promise(function (resolve) {
        let popup = $("#popup");
        popup.css("overflow", "hidden");
        popup.css("max-height", popup.height());
        popup.css("height", popup.height());
        popup.animate(
            {
                left: pos["left"],
                top: pos["top"],
                width: pos["width"] + "px",
                maxHeight: pos["height"] + "px",
            },
            RESIZE_SPEED,
            function () {
                popup.css("overflow", "");
                popup.css("max-height", "");
                popup.css("height", "");
                resolve();
            },
        );
    });
}

async function _select(newid) {
    await new Promise(async function (resolve) {
        $("#" + newid.id).addClass("ev_current");
        $("." + newid.jsuid).addClass("ev_selected");
        setPopupOpen(true);

        let el = document.getElementById(newid.id);
        const elRect = _pageBoundingBox(el);
        const popWidth = WIDTH_DETAILS;

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

function closePopup() {
    fireEvent(new CancelEvent());
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

// Adjusts the popup's vertical position after its content has loaded and its final height is known.
//
// Cases handled (all comparisons in page-absolute coordinates):
//   - Top clipped, bottom visible: align the popup bottom with the event's bottom edge.
//   - Top visible, popup overflows viewport bottom: shift the popup upward, clamping its bottom to
//     the event's bottom edge or the viewport bottom, whichever is higher.
//   - In both cases, ensure the popup never goes above the current scroll top.
function _correctPosition(id) {
    let el = document.getElementById(id);
    if (!el) return;

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
