class SegmentedControl {
    constructor(id, options) {
        this.id = id;
        this.segmented = document.getElementById(`${id}_segmented`);
        this.inputName = options.inputName;
        this.panels = options.panels || {};
        this.changeHandlers = [];
    }

    mount() {
        const slot = this.segmented
            ?.closest(".ev_form_row_expanded")
            ?.previousElementSibling?.querySelector(".ev_form_row_segmented");
        if (this.segmented && slot && this.segmented.parentElement !== slot) {
            slot.appendChild(this.segmented);
        }
        return this;
    }

    value() {
        return this.inputs().filter(":checked").val();
    }

    select(value) {
        this.input(value)?.prop("checked", true);
        return this.refresh();
    }

    onChange(handler) {
        this.changeHandlers.push(handler);
        return this;
    }

    setOptionEnabled(value, enabled, fallbackValue) {
        const input = this.input(value);
        if (!input.length) return this;

        input.prop("disabled", !enabled);
        this.label(value).toggle(enabled);

        if (!enabled && input.is(":checked") && fallbackValue) {
            this.input(fallbackValue).prop("checked", true);
        }

        return this;
    }

    setLabelStyle(value, style) {
        this.label(value).css(style);
        return this;
    }

    refresh() {
        const value = this.value();
        for (const [panelValue, selector] of Object.entries(this.panels)) {
            $(selector).toggle(panelValue === value);
        }
        for (const handler of this.changeHandlers) {
            handler(value, this);
        }
        return this;
    }

    bind() {
        this.inputs().on("change", () => this.refresh());
        return this;
    }

    input(value) {
        return this.inputs().filter(`[value='${value}']`);
    }

    label(value) {
        const input = this.input(value);
        return input.length ? $(`label[for='${input.attr("id")}']`) : $();
    }

    inputs() {
        return $("input[name='" + this.inputName + "']");
    }
}
