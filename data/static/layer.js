const LAYER_SPEED = 150;

window.ev.layers = {};

class EvLayer {
    constructor(id) {
        this.element = document.getElementById(id);
        if (!this.element) {
            throw new Error(`Missing layer element: ${id}`);
        }

        this.dismissHandler = null;
        this.pointerHandler = (e) => {
            if (this.dismissHandler === null) return;
            if (e.target === this.element || e.target.closest(".ev_layer_backdrop")) {
                this.dismissHandler(e);
            }
        };
    }

    open(dismissHandler = null, animate = false) {
        this.close();
        this.dismissHandler = dismissHandler;
        this.element.hidden = false;

        if (animate) {
            $(this.element).css("opacity", 0).fadeTo(LAYER_SPEED, 1);
        }

        if (dismissHandler !== null) {
            $(this.element).on("mousedown", this.pointerHandler);
        }
    }

    close() {
        $(this.element).off("mousedown", this.pointerHandler);
        this.dismissHandler = null;
        this.element.hidden = true;
    }
}

function getLayer(id) {
    if (!window.ev.layers[id]) {
        window.ev.layers[id] = new EvLayer(id);
    }
    return window.ev.layers[id];
}

window.ev.getLayer = getLayer;
