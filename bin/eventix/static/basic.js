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
