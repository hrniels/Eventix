// Global state namespace for all cross-fragment mutable state in templates.
// Each module that uses cross-fragment state is responsible for initializing
// its own keys here.
window.ev = {};

function curUTCDay() {
    const date = new Date();
    const y = date.getUTCFullYear();
    const m = (date.getUTCMonth() + 1).toString().padStart(2, "0");
    const d = date.getUTCDate().toString().padStart(2, "0");
    return `${y}${m}${d}`;
}

function curDate(timezone) {
    const parts = new Intl.DateTimeFormat("en-GB", {
        timeZone: timezone,
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false,
    }).formatToParts(new Date());

    return Object.fromEntries(
        // remove separators like "/", ":", " "
        parts.filter((p) => p.type !== "literal").map((p) => [p.type, Number(p.value)]),
    );
}

function curDateStr(timezone) {
    const date = curDate(timezone);
    const y = date.year;
    const m = date.month.toString().padStart(2, "0");
    const d = date.day.toString().padStart(2, "0");
    const H = date.hour.toString().padStart(2, "0");
    const M = date.minute.toString().padStart(2, "0");
    const S = date.second.toString().padStart(2, "0");
    return `TU${y}-${m}-${d}T${H}:${M}:${S}`;
}

function copyToClipboard(text) {
    var $temp = $("<input>");
    $("body").append($temp);
    $temp.val(text).select();
    document.execCommand("copy");
    $temp.remove();
}

function getElementAtWith(x, y, prop) {
    const elems = document.elementsFromPoint(x, y);
    for (el in elems) {
        if (prop(elems[el])) return elems[el];
    }
    return null;
}

function invertSelection(prefix) {
    for (let i = 1; ; i++) {
        let checkbox = document.getElementById(prefix + i);
        if (checkbox == null) break;

        checkbox.checked = checkbox.checked ? false : true;
    }
}

function toggleCheckbox(id) {
    let el = document.getElementById(id);
    el.checked = !el.checked;
}

function moveToTabCenter(elId, tabsId, tabBarId) {
    const pos = $("#" + tabsId).position().top;
    let tab = $("#" + tabBarId);
    let el = $("#" + elId);
    el.css({
        position: "absolute",
        top: pos + tab.outerHeight() / 2 - el.outerHeight() / 2 + "px",
        left: tab.outerWidth() / 2 - el.outerWidth() / 2 + "px",
    }).show();
}

function addArrowToDatePicker(input, inst) {
    // move the datepicker a bit down so that we can draw the arrow on top
    inst.dpDiv.css({
        marginTop: "10px",
    });
    inst.dpDiv.addClass("popup");
}

function hideArrowBottom(inst) {
    // ensure that the datepicker header is above the arrow
    $(".ui-datepicker-header").css("zIndex", 2);
}

function setPersonalOverwrite(id_prefix, overwrite) {
    const ids = ["none", "relative", "absolute", "datetime__time_"];
    for (id in ids) {
        $("#" + id_prefix + "_" + ids[id]).prop("disabled", !overwrite);
    }
    $("#" + id_prefix + "_durunit_").selectmenu("option", "disabled", !overwrite);
    $("#" + id_prefix + "_durtype_").selectmenu("option", "disabled", !overwrite);
    $("#" + id_prefix + "_duration").spinner(overwrite ? "enable" : "disable");
    $("#" + id_prefix + "_datetime__date_").datepicker("option", "disabled", !overwrite);
}

function replaceSmoothly(id, newHtml, delay) {
    let el = $("#" + id);

    // Replace content but hide overflow to avoid jumps
    const oldHeight = el.outerHeight();
    el.css({ height: oldHeight, overflow: "hidden" });

    // Create a hidden clone to measure target size
    const clone = el
        .clone()
        .css({
            visibility: "hidden",
            position: "absolute",
            height: "auto",
            width: el.width(),
        })
        .html(newHtml)
        .appendTo("body");

    // measure and remove again
    const newHeight = clone.outerHeight();
    clone.remove();

    if (newHeight > oldHeight) {
        // Growing: show new content immediately, then expand
        el.html(newHtml);
        el.animate({ height: newHeight }, delay, () => el.css({ height: "", overflow: "" }));
    } else {
        // Shrinking: animate first, then swap content
        el.animate({ height: newHeight }, delay, () => {
            el.html(newHtml);
            el.css({ height: "", overflow: "" });
        });
    }
}
