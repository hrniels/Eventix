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
