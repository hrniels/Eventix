const MODAL_FOCUSABLE_SELECTOR =
    "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), " +
    'textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

window.ev.modal = {
    active: null,
};

function modalIsOpen() {
    return window.ev.modal.active !== null;
}

function modalLayer() {
    return document.getElementById("modal-layer");
}

function modalLabels() {
    const layer = modalLayer();
    return {
        close: layer.dataset.closeLabel,
        cancel: layer.dataset.cancelLabel,
        defaultTitle: layer.dataset.defaultTitle,
    };
}

function modalEscapesHtml(text) {
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
}

function modalFormatMessage(message) {
    return modalEscapesHtml(message).replace(/\n/g, "<br />");
}

function closeModal(result) {
    const active = window.ev.modal.active;
    if (active === null) return;

    $(document).off("keydown", active.keyHandler);
    $(modalLayer()).off("mousedown", active.overlayHandler);

    const layer = modalLayer();
    layer.hidden = true;

    document.getElementById("modal-title").textContent = "";
    document.getElementById("modal-title").hidden = true;
    document.getElementById("modal-message").textContent = "";
    document.getElementById("modal-actions").replaceChildren();

    setModalOpen(false);
    window.ev.modal.active = null;

    active.resolve(result);
}

function buildModalButton(action) {
    const button = document.createElement("a");
    button.className = "ev_button_medium " + action.className;
    button.textContent = action.label;
    button.addEventListener("click", function () {
        closeModal(action.result);
    });
    return button;
}

function openModal(options) {
    if (modalIsOpen()) {
        return Promise.reject(
            new Error("Tried to open a second modal while one is already active."),
        );
    }

    return new Promise(function (resolve) {
        const layer = modalLayer();
        const dialog = document.getElementById("modal-dialog");
        const title = document.getElementById("modal-title");
        const message = document.getElementById("modal-message");
        const actions = document.getElementById("modal-actions");

        // header is placed in the modal title (ev_header). Fallback to options.title
        const header = options.header || options.title || "";
        title.textContent = header;
        title.hidden = !header;
        message.innerHTML = modalFormatMessage(options.message);
        actions.replaceChildren(...options.actions.map(buildModalButton));

        const keyHandler = function (e) {
            if (!modalIsOpen()) return;
            e.preventDefault();
            if (e.key === "Escape") {
                closeModal(options.dismissResult);
                return;
            }
        };
        const overlayHandler = function (e) {
            if (e.target === layer || e.target.classList.contains("ev_modal_backdrop")) {
                closeModal(options.dismissResult);
            }
        };

        window.ev.modal.active = {
            resolve,
            keyHandler,
            overlayHandler,
        };

        setModalOpen(true);
        layer.hidden = false;

        $(document).on("keydown", keyHandler);
        $(layer).on("mousedown", overlayHandler);
    });
}

function showAlert(message, title = null) {
    const labels = modalLabels();
    return openModal({
        title: title ?? labels.defaultTitle,
        message,
        dismissResult: undefined,
        actions: [
            {
                label: labels.close,
                className: "ev_button_normal",
                result: undefined,
            },
        ],
    });
}

function showConfirm(message, confirmLabel, title = null, confirmClassName = "ev_button_critical") {
    const labels = modalLabels();
    return openModal({
        title: title ?? labels.defaultTitle,
        message,
        dismissResult: false,
        actions: [
            {
                label: labels.cancel,
                className: "ev_button_normal",
                result: false,
            },
            {
                label: confirmLabel,
                className: confirmClassName,
                result: true,
            },
        ],
    });
}

window.addEventListener("pagehide", function () {
    closeModal(false);
});
