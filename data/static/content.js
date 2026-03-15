let contentHandlers = {};

// Binds an event handler and remembers it for later removal.
// The handlers are removed whenever the content area is replaced.
function bindContentHandler(namespace, target, event, handler) {
    if(!contentHandlers[namespace])
        contentHandlers[namespace] = []
    contentHandlers[namespace].push({ target, event, handler });
    $(target).on(event, handler);
}

function clearContentHandler(namespace) {
    if(contentHandlers[namespace]) {
        for (const { target, event, handler } of contentHandlers[namespace]) {
            $(target).off(event, handler);
        }
        contentHandlers[namespace] = [];
    }
}

// Maps the pathname of each SPA page to the content slug used to build the AJAX
// request URL `/pages/<slug>/content`. Pages not listed here fall back to a full
// navigation in `navigateTo`.
const PAGE_SLUGS = {
    '/pages/monthly':          'monthly',
    '/pages/weekly':           'weekly',
    '/pages/list':             'list',
    '/pages/calendars':        'calendars',
    '/pages/items/add':        'items/add',
    '/pages/items/edit':       'items/edit',
    '/pages/collections/add':  'collections/add',
    '/pages/collections/edit': 'collections/edit',
};

// Navigates to a SPA page by AJAX-loading its content fragment into
// `#page-content` and pushing a new history entry, without a full page reload.
// If the target path is not a known SPA page the browser performs a normal
// full navigation instead.
function navigateTo(url) {
    const parsed = new URL(url, window.location.origin);
    const slug = PAGE_SLUGS[parsed.pathname];
    if (!slug) {
        window.location.href = url;
        return;
    }
    const queryStr = parsed.search ? parsed.search.slice(1) : '';
    loadPageContent(slug, '#page-content', queryStr, null);
}

// Navigates to an add/edit form page via AJAX, appending the current URL as the
// `prev` parameter so the form's Back button knows where to return. If the current
// page already carries a `prev` parameter (i.e. the user is already on a form),
// that earlier origin is used instead, so the chain always points back to the
// last non-form page.
function loadWithPrev(url) {
    const prevFull = document.location.href;
    const prevUrl = new URL(prevFull);
    const prevPrev = prevUrl.searchParams.get('prev');
    const fullUrl = new URL(url, prevUrl.origin);
    fullUrl.searchParams.append('prev', prevPrev ?? prevFull);
    navigateTo(fullUrl.toString());
}

// Fetches the content fragment for `pageSlug` and injects it into `containerId`.
// `queryStr` is a pre-encoded query string such as `"keywords=foo&page=1"` or
// `"date=2025-03"` (pass an empty string for no parameters). `onLoaded` is an
// optional callback invoked after the HTML has been injected and `resizeBoxes`
// has run. Does not touch browser history.
function fetchContent(pageSlug, containerId, queryStr, onLoaded) {
    const params = queryStr ? '?' + queryStr : '';
    getRequest('/pages/' + pageSlug + '/content' + params, function(html) {
        clearContentHandler(containerId.slice(1));
        $(containerId).html(html);
        resizeBoxes();
        if(onLoaded)
            onLoaded();
    }, 'html');
}

// Navigates to a new state for `pageSlug` by fetching the content fragment and
// pushing a new history entry. Delegates rendering to `fetchContent`.
function loadPageContent(pageSlug, containerId, queryStr, onLoaded) {
    const params = queryStr ? '?' + queryStr : '';
    history.pushState({ slug: pageSlug, query: queryStr || '' }, '', '/pages/' + pageSlug + params);
    fetchContent(pageSlug, containerId, queryStr, onLoaded);
}
