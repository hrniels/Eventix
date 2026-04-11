let curDrag = null;

// Style element injected during a copy-mode drag to override the cursor globally.
let copyCursorStyle = null;

function setCopyCursor(active) {
    if (active && !copyCursorStyle) {
        copyCursorStyle = $("<style>* { cursor: copy !important; }</style>").appendTo("head");
    } else if (!active && copyCursorStyle) {
        copyCursorStyle.remove();
        copyCursorStyle = null;
    }
}

function dragKeyHandler(e) {
    if (!curDrag || curDrag.recurrent) return;
    curDrag.isCopy = e.ctrlKey;
    setCopyCursor(curDrag.isCopy);
}

function dragMouseMoveHandler(e) {
    curDrag._clear();
    let el = getElementAtWith(e.clientX, e.clientY, function (el) {
        return $(el).hasClass(curDrag.targetClass);
    });
    if (el) {
        $(el).addClass("ev_drag_hover");
        curDrag.lastHover = $(el);
    }
}

class DragOperation {
    constructor(owner, recurrent, sourceClass, targetClass) {
        this.owner = owner;
        this.recurrent = recurrent;
        this.sourceClass = sourceClass;
        this.targetClass = targetClass;
        this.lastHover = null;
        this.dragging = null;
        this.lastMousePos = null;
        this.isCopy = false;
    }

    settings() {
        let settings = {
            opacity: 0.7,
            zIndex: 100,
            addClasses: false,
            helper: "clone",
            distance: 10,
            start: function (e, ui) {
                ui.helper.css("width", $(this).css("width"));
                ui.helper.css("height", $(this).css("height"));
                const drag = $(this).data("drag");
                if (!drag.owner) return drag._startForeign($(this));
                return drag._startOwned(e);
            },
        };

        if (this.owner) {
            settings.revert = function (dropped) {
                const drag = $(this).data("drag");
                const pos = drag.lastMousePos;
                const el = getElementAtWith(pos.clientX, pos.clientY, function (e) {
                    return $(e).hasClass(drag.targetClass);
                });
                return el == null;
            };
            settings.drag = function (e, ui) {
                let drag = $(this).data("drag");
                drag.lastMousePos = {
                    clientX: e.clientX,
                    clientY: e.clientY,
                };
                // Keep isCopy in sync on mouse moves as well (handles the case where Ctrl was
                // already held when the drag started).
                if (!drag.recurrent) {
                    drag.isCopy = e.ctrlKey;
                    setCopyCursor(drag.isCopy);
                }
            };
            settings.stop = function () {
                let drag = $(this).data("drag");
                const pos = drag.lastMousePos;

                const el = getElementAtWith(pos.clientX, pos.clientY, function (e) {
                    return $(e).hasClass(drag.targetClass);
                });
                if (el) {
                    const uid = drag.dragging[0].dataset.uid;
                    const rid = drag.dragging[0].dataset.rid;
                    const $date = el.dataset.date;
                    const hour = el.dataset.hour;
                    if (drag.isCopy) {
                        copyEvent(uid, $date, hour, reloadContent);
                    } else {
                        moveEvent(uid, rid, $date, hour, reloadContent);
                    }
                }

                drag.isCopy = false;
                setCopyCursor(false);
                drag._clear();
                drag.dragging.removeClass("ev_drag_event");
                $(document).off("mousemove", dragMouseMoveHandler);
                $(document).off("keydown keyup", dragKeyHandler);
                curDrag = null;
            };
        }
        return settings;
    }

    _startForeign(el) {
        this.dragging = el;
        this.dragging.css("cursor", "not-allowed");
        $("body").css("cursor", "not-allowed");
        let drag = this;
        $(document).one("mouseup", function () {
            drag.dragging.css("cursor", "");
            $("body").css("cursor", "");
        });
        return false;
    }

    _startOwned(e) {
        const sourceClass = this.sourceClass;
        let el = getElementAtWith(e.clientX, e.clientY, function (el) {
            return $(el).hasClass(sourceClass) && !$(el).hasClass("ui-draggable-dragging");
        });
        if (el) {
            this.dragging = $(el);
            $(el).addClass("ev_drag_event");
        }
        curDrag = this;
        $(document).on("mousemove", dragMouseMoveHandler);
        $(document).on("keydown keyup", dragKeyHandler);
        return true;
    }

    _clear() {
        if (this.lastHover) {
            this.lastHover.removeClass("ev_drag_hover");
            this.lastHover = null;
        }
    }
}

// Style element injected during a resize drag to override the cursor globally.
let resizeCursorStyle = null;

function setResizeCursor(cursor) {
    if (resizeCursorStyle) {
        resizeCursorStyle.remove();
        resizeCursorStyle = null;
    }
    if (cursor) {
        resizeCursorStyle = $("<style>* { cursor: " + cursor + " !important; }</style>").appendTo(
            "head",
        );
    }
}

class ResizeOperation {
    constructor(uid, rid) {
        this.uid = uid;
        this.rid = rid;
        // State set during an active resize.
        this._edge = null;
        this._el = null;
        this._columnTop = 0;
        this._startTotalMin = 0;
        this._endTotalMin = 0;
        this._boundMove = this._move.bind(this);
        this._boundStop = this._stop.bind(this);
    }

    // Begins a resize drag. `el` is the event <div>, `edge` is "top" or "bottom".
    start(e, el, edge) {
        // Prevent the event's onclick (SelectEvent) from firing after the mousedown.
        e.stopPropagation();
        e.preventDefault();

        this._edge = edge;
        this._el = el;

        // The column container is the `position: relative; height: 1440px` div that is the
        // direct parent of the event element. Its top in viewport coordinates gives us the
        // reference point for converting mouse Y to minutes.
        const columnRect = el.parentElement.getBoundingClientRect();
        this._columnTop = columnRect.top;

        this._startTotalMin =
            parseInt(el.dataset.startHour, 10) * 60 + parseInt(el.dataset.startMin, 10);
        this._endTotalMin = parseInt(el.dataset.endHour, 10) * 60 + parseInt(el.dataset.endMin, 10);

        setResizeCursor(edge === "top" ? "n-resize" : "s-resize");

        $(document).on("mousemove", this._boundMove);
        $(document).on("mouseup", this._boundStop);
    }

    _snapToGrid(rawMinutes) {
        return Math.round(rawMinutes / 30) * 30;
    }

    _move(e) {
        const rawMinutes = e.clientY - this._columnTop;
        const snapped = Math.max(0, Math.min(1440, this._snapToGrid(rawMinutes)));

        if (this._edge === "top") {
            // New start must be at least 30 min before the existing end.
            const newStart = Math.min(snapped, this._endTotalMin - 30);
            this._el.style.top = newStart + 1 + "px";
            this._el.style.height = "calc(" + (this._endTotalMin - newStart) + "px - 4px)";
            // Keep the inner content div height in sync.
            const inner = this._el.querySelector("div[style*='height: calc']");
            if (inner) {
                inner.style.height = "calc(" + (this._endTotalMin - newStart) + "px - 8px)";
            }
        } else {
            // New end must be at least 30 min after the existing start.
            const newEnd = Math.max(snapped, this._startTotalMin + 30);
            this._el.style.height = "calc(" + (newEnd - this._startTotalMin) + "px - 4px)";
            const inner = this._el.querySelector("div[style*='height: calc']");
            if (inner) {
                inner.style.height = "calc(" + (newEnd - this._startTotalMin) + "px - 8px)";
            }
        }
    }

    _stop(e) {
        $(document).off("mousemove", this._boundMove);
        $(document).off("mouseup", this._boundStop);
        setResizeCursor(null);

        const rawMinutes = e.clientY - this._columnTop;
        const snapped = Math.max(0, Math.min(1440, this._snapToGrid(rawMinutes)));

        if (this._edge === "top") {
            const newStart = Math.min(snapped, this._endTotalMin - 30);
            const hour = Math.floor(newStart / 60);
            const minute = newStart % 60;
            resizeEvent(this.uid, this.rid, hour, minute, null, null, reloadContent);
        } else {
            const newEnd = Math.max(snapped, this._startTotalMin + 30);
            const hour = Math.floor(newEnd / 60);
            const minute = newEnd % 60;
            resizeEvent(this.uid, this.rid, null, null, hour, minute, reloadContent);
        }

        this._edge = null;
        this._el = null;
    }
}
