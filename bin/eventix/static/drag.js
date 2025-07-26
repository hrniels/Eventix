let curDrag = null;

function dragMouseMoveHandler(e) {
    curDrag._clear();
    let el = getElementAtWith(e.clientX, e.clientY, function(el) {
        return $(el).hasClass(curDrag.targetClass);
    });
    if(el) {
        $(el).addClass('ev_drag_hover');
        curDrag.lastHover = $(el);
    }
}

class DragOperation {
    constructor(owner, sourceClass, targetClass) {
        this.owner = owner;
        this.sourceClass = sourceClass;
        this.targetClass = targetClass;
        this.lastHover = null;
        this.dragging = null;
        this.lastMousePos = null;
    }

    settings() {
        let settings = {
            opacity: 0.7,
            zIndex: 100,
            addClasses: false,
            helper: 'clone',
            distance: 10,
            start: function(e, ui) {
                ui.helper.css('width', $(this).css('width'));
                ui.helper.css('height', $(this).css('height'));
                const drag = $(this).data('drag');
                if(!drag.owner)
                    return drag._startForeign($(this));
                return drag._startOwned(e);
            },
        };

        if(this.owner) {
            settings.revert = function(dropped) {
                const drag = $(this).data('drag');
                const pos = drag.lastMousePos;
                const el = getElementAtWith(pos.clientX, pos.clientY, function(e) {
                    return $(e).hasClass(drag.targetClass);
                });
                return el == null;
            };
            settings.drag = function(e, ui) {
                let drag = $(this).data('drag');
                drag.lastMousePos = {
                    clientX: e.clientX,
                    clientY: e.clientY
                };
            };
            settings.stop = function() {
                let drag = $(this).data('drag');
                const pos = drag.lastMousePos;

                const el = getElementAtWith(pos.clientX, pos.clientY, function(e) {
                    return $(e).hasClass(drag.targetClass);
                });
                if(el) {
                    const uid = drag.dragging[0].dataset.uid;
                    const rid = drag.dragging[0].dataset.rid;
                    const $date = el.dataset.date;
                    const hour = el.dataset.hour;
                    moveEvent(uid, rid, $date, hour, reloadPage);
                }

                drag._clear();
                drag.dragging.removeClass('ev_drag_event');
                $(document).off('mousemove', dragMouseMoveHandler);
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
        $(document).one("mouseup", function() {
            drag.dragging.css("cursor", "");
            $("body").css("cursor", "");
        });
        return false;
    }

    _startOwned(e) {
        const sourceClass = this.sourceClass;
        let el = getElementAtWith(e.clientX, e.clientY, function(el) {
            return $(el).hasClass(sourceClass) && !$(el).hasClass('ui-draggable-dragging');
        });
        if(el) {
            this.dragging = $(el);
            $(el).addClass('ev_drag_event');
        }
        curDrag = this;
        $(document).on('mousemove', dragMouseMoveHandler);
        return true;
    }

    _clear() {
        if(this.lastHover) {
            this.lastHover.removeClass('ev_drag_hover');
            this.lastHover = null;
        }
    }
}
