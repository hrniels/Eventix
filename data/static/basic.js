function reloadPage() {
    window.location.reload();
}

function curUTCDay() {
    const date = new Date();
    const y = date.getUTCFullYear();
    const m = (date.getUTCMonth() + 1).toString().padStart(2, '0');
    const d = date.getUTCDate().toString().padStart(2, '0');
    return `${y}${m}${d}`;
}

function curISODate() {
    const date = new Date();
    const y = date.getUTCFullYear();
    const m = (date.getUTCMonth() + 1).toString().padStart(2, '0');
    const d = date.getUTCDate().toString().padStart(2, '0');
    const H = date.getUTCHours().toString().padStart(2, '0');
    const M = date.getUTCMinutes().toString().padStart(2, '0');
    const S = date.getUTCSeconds().toString().padStart(2, '0');
    return `${y}${m}${d}T${H}${M}${S}Z`;
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
    for(el in elems) {
        if(prop(elems[el]))
            return elems[el];
    }
    return null;
}

function invertSelection(prefix) {
    for(let i = 1;;i++)
    {
        let checkbox = document.getElementById(prefix + i);
        if(checkbox == null)
            break;

        checkbox.checked = checkbox.checked ? false : true;
    }
}

function toggleCheckbox(id) {
    let el = document.getElementById(id);
    el.checked = !el.checked;
}

function moveToTabCenter(elId, tabsId, tabBarId) {
    const pos = $('#' + tabsId).position().top;
    let tab = $('#' + tabBarId);
    let el = $('#' + elId);
    el.css({
        position: "absolute",
        top: (pos +
            (tab.outerHeight() / 2) - (el.outerHeight() / 2)) + "px",
        left: ((tab.outerWidth() / 2) - (el.outerWidth() / 2)) + "px"
    }).show();
}

function addArrowToDatePicker(input, inst) {
    // move the datepicker a bit down so that we can draw the arrow on top
    inst.dpDiv.css({
        marginTop: '10px',
    });
    inst.dpDiv.addClass('popup');
}

function hideArrowBottom(inst) {
    // ensure that the datepicker header is above the arrow
    $('.ui-datepicker-header').css('zIndex', 2);
}

function gotoWithPrev(url) {
    const prev = document.location.href;
    // check if we already have a prev URL
    const prev_url = new URL(prev);
    const prev_prev = prev_url.searchParams.get('prev');
    let full_url = new URL(url, prev_url.origin);
    // if so, go back to this one (the last none-edit/new page)
    full_url.searchParams.append('prev', prev_prev ?? prev);
    document.location.href = full_url;
}

function setPersonalOverwrite(id_prefix, overwrite) {
    const ids = ["none", "relative", "absolute", "datetime__time_"];
    for(id in ids) {
        $("#" + id_prefix + "_" + ids[id]).prop("disabled", !overwrite);
    }
    $("#" + id_prefix + "_durunit_").selectmenu("option", "disabled", !overwrite);
    $("#" + id_prefix + "_durtype_").selectmenu("option", "disabled", !overwrite);
    $("#" + id_prefix + "_duration").spinner(overwrite ? "enable" : "disable");
    $("#" + id_prefix + "_datetime__date_").datepicker("option", "disabled", !overwrite);
}

function replaceSmoothly(id, newHtml, delay) {
    let el = $('#' + id);

    // Replace content but hide overflow to avoid jumps
    const oldHeight = el.outerHeight();
    el.css({ height: oldHeight, overflow: "hidden" });

    // Create a hidden clone to measure target size
    const clone = el.clone()
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

    if(newHeight > oldHeight) {
        // Growing: show new content immediately, then expand
        el.html(newHtml);
        el.animate({ height: newHeight }, delay, () => el.css({ height: "", overflow: "" }));
    }
    else {
        // Shrinking: animate first, then swap content
        el.animate({ height: newHeight }, delay, () => {
            el.html(newHtml);
            el.css({ height: "", overflow: "" });
        });
    }
}
