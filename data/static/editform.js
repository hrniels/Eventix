function attendeeDisplayName(value) {
    return value.replace(/\s*<[^>]*>\s*$/, "").trim();
}

const FormExpandShrink = {
    shorteners: {},
    currentExpanded: null,

    registerShortener(type, fn) {
        this.shorteners[type] = fn;
    },

    getShortValue(row) {
        const collapsed = row.querySelector(".ev_form_row_collapsed");
        if (!collapsed) return "";

        const shortenerId = collapsed.dataset.shortenerId || "default";
        const shortener = this.shorteners[shortenerId] || this.shorteners.default;
        const expanded = row.querySelector(".ev_form_row_expanded");

        return shortener(expanded);
    },

    setShortValue(row, collapsed) {
        const shortValue = this.getShortValue(row);
        let currentEl = collapsed.querySelector(".ev_form_row_current");
        if (currentEl) currentEl.textContent = shortValue;
    },

    collapse(row) {
        const collapsed = row.querySelector(".ev_form_row_collapsed");
        const expanded = row.querySelector(".ev_form_row_expanded");
        if (!collapsed || !expanded) return;

        if (row.classList.contains("collapsed")) return;

        this.setShortValue(row, collapsed);

        const $expanded = $(expanded);
        $expanded.stop(true, true);
        $expanded.slideUp(200, function () {
            row.classList.add("collapsed");
        });
    },

    expand(row) {
        const collapsed = row.querySelector(".ev_form_row_collapsed");
        const expanded = row.querySelector(".ev_form_row_expanded");
        if (!collapsed || !expanded) return;

        row.classList.remove("collapsed");

        const firstInput = expanded.querySelector("input, select, textarea");
        const $expanded = $(expanded);

        $expanded.stop(true, true);
        $expanded.hide().slideDown(200);

        if (firstInput) {
            setTimeout(() => firstInput.focus(), 10);
        }
    },

    toggle(row) {
        if (row.classList.contains("collapsed")) {
            if (this.currentExpanded && this.currentExpanded !== row) {
                this.collapse(this.currentExpanded);
            }
            this.expand(row);
            this.currentExpanded = row;
        } else {
            this.collapse(row);
            this.currentExpanded = null;
        }
    },

    init(containerSelector) {
        const container = document.querySelector(containerSelector);
        const rows = container.querySelectorAll(".ev_form_row_collapsible");

        this.currentExpanded = null;

        rows.forEach((row) => {
            row.addEventListener("click", (e) => {
                if (!e.target.closest(".ev_form_row_header")) return;
                if (e.target.closest(".ev_form_row_segmented")) return;
                e.preventDefault();
                this.toggle(row);
            });

            const collapsed = row.querySelector(".ev_form_row_collapsed");
            if (collapsed) this.setShortValue(row, collapsed);

            row.classList.add("collapsed");
        });
    },
};

FormExpandShrink.registerShortener("default", function (expanded) {
    const inputs = expanded.querySelectorAll("input[type='text'], input[type='number'], textarea");
    for (const input of inputs) {
        if (input.value && input.value.trim()) {
            return input.value.trim();
        }
    }
    const selects = expanded.querySelectorAll("select");
    for (const select of selects) {
        const option = select.options[select.selectedIndex];
        if (option && option.value && option.textContent.trim()) {
            return option.textContent.trim();
        }
    }
    return "";
});

FormExpandShrink.registerShortener("datetimerange", function (expanded) {
    const dateRoot = expanded.querySelector(".ev_date");
    if (!dateRoot) return "";

    const allDay = dateRoot.querySelector("input[id$='all_day']")?.checked ?? false;
    const fromEnabled = dateRoot.querySelector("input[id$='from_enabled']")?.checked ?? true;
    const toEnabled = dateRoot.querySelector("input[id$='to_enabled']")?.checked ?? true;

    const fromDate = dateRoot.querySelector("input[id$='from_date']");
    const fromTime = dateRoot.querySelector("input[id$='_from__time_']");
    const toDate = dateRoot.querySelector("input[id$='to_date']");
    const toTime = dateRoot.querySelector("input[id$='_to__time_']");
    const tz = dateRoot.querySelector("input[id$='_timezone_']");

    let fromValue = "";
    let toValue = "";

    if (fromEnabled && fromDate?.value.trim()) {
        fromValue = fromDate.value.trim();
        if (!allDay && fromTime?.value.trim()) {
            fromValue += " " + fromTime.value.trim();
        }
    }

    if (toEnabled && toDate?.value.trim()) {
        toValue = toDate.value.trim();
        if (!allDay && toTime?.value.trim()) {
            toValue += " " + toTime.value.trim();
        }
    }

    if (!fromValue && !toValue) return "None";
    let res = fromValue + " - " + toValue;
    if (!allDay && tz.value) res += ", " + tz.value;
    return res;
});

FormExpandShrink.registerShortener("alarm", function (expanded) {
    const personalMode = expanded.parentElement.querySelector("input[id$='_mode_personal']");
    const isPersonal = personalMode?.checked ?? false;

    const panelId = isPersonal ? "_panel_personal" : "_panel_calendar";
    const prefix = isPersonal ? "Personal: " : "Calendar: ";
    const panel = expanded.querySelector("[id$='" + panelId + "']");
    if (!panel) return prefix + "None";

    const checkedTrigger = panel.querySelector("input[name$='[trigger]']:checked");

    if (checkedTrigger.value === "RELATIVE") {
        const duration = panel.querySelector("input[id$='_duration']");
        const durunit = panel.querySelector("select[id$='_durunit_']");
        const durtype = panel.querySelector("select[id$='_durtype_']");

        const durValue = duration?.value.trim() || "";
        const durunitText = durunit?.options[durunit.selectedIndex]?.textContent.trim() || "";
        const durtypeText = durtype?.options[durtype.selectedIndex]?.textContent.trim() || "";

        if (!durValue) return prefix + "Relative";
        return prefix + [durValue, durunitText, durtypeText].filter(Boolean).join(" ");
    }

    if (checkedTrigger.value === "ABSOLUTE") {
        const dateInput = panel.querySelector("input[id$='_datetime__date_']");
        const timeInput = panel.querySelector("input[id$='_datetime__time_']");

        const dateValue = dateInput?.value.trim() || "";
        const timeValue = timeInput?.value.trim() || "";

        if (!dateValue && !timeValue) return prefix + "Absolute";
        return prefix + [dateValue, timeValue].filter(Boolean).join(" ");
    }

    return prefix + "None";
});

FormExpandShrink.registerShortener("recur", function (expanded) {
    const ro = expanded.querySelector("#form-row-expanded-rrule-ro");
    if (ro) return ro.innerHTML.replace("<br>", ", ");

    const freq = expanded.querySelector("input[id$='_freq']")?.value;
    if (freq) {
        switch (freq) {
            case "HOURLY":
                return "Hourly";
            case "DAILY":
                return "Daily";
            case "WEEKLY":
                return "Weekly";
            case "MONTHLY":
                return "Monthly";
            case "YEARLY":
                return "Annually";
        }
    }
    return "None";
});

FormExpandShrink.registerShortener("attendees", function (expanded) {
    const nameInputs = expanded.querySelectorAll("input[name*='[name][']");
    const names = [];

    nameInputs.forEach((input) => {
        if (input.value && input.value.trim()) {
            const displayName = attendeeDisplayName(input.value.trim());
            names.push(displayName || input.value.trim());
        }
    });

    if (names.length === 0) {
        const container = expanded.querySelector("[id$='-attendees']");
        if (container && container.textContent.trim() === "-") return "None";
        const newInput = expanded.querySelector("[id$='-new-attendee']");
        if (newInput && newInput.value.trim()) {
            const displayName = attendeeDisplayName(newInput.value.trim());
            return displayName || newInput.value.trim();
        }
    }

    if (names.length === 0) return "None";
    if (names.length <= 3) return names.join(", ");

    const extra = names.length - 3;
    return `${names.slice(0, 3).join(", ")} and ${extra} more`;
});
