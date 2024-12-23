function invertSelection(prefix)
{
    for(var i = 1;;i++)
    {
        var checkbox = document.getElementById(prefix + i);
        if(checkbox == null)
            break;

        checkbox.checked = checkbox.checked ? false : true;
    }
}

function toggleCheckbox(id)
{
    var el = document.getElementById(id);
    el.checked = !el.checked;
}

function moveToTabCenter(elId, tabsId, tabBarId)
{
    var pos = $('#' + tabsId).position().top;
    var tab = $('#' + tabBarId);
    var el = $('#' + elId);
    el.css({
        position: "absolute",
        top: (pos +
            (tab.outerHeight() / 2) - (el.outerHeight() / 2)) + "px",
        left: ((tab.outerWidth() / 2) - (el.outerWidth() / 2)) + "px"
    }).show();
}

function addArrowToDatePicker(input, inst)
{
    // move the datepicker a bit down so that we can draw the arrow on top
    inst.dpDiv.css({
        marginTop: '10px',
    });
    inst.dpDiv.addClass('popup');
}

function hideArrowBottom(inst)
{
    // ensure that the datepicker header is above the arrow
    $('.ui-datepicker-header').css('zIndex', 2);
}
