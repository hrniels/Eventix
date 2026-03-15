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

function reloadPage() {
    window.location.reload();
}

// Reloads the main content area by re-fetching it via AJAX, without a full page reload. Closes the
// popup first so it is not left open with stale data. Falls back to a full reload when the SPA
// shell is absent (e.g. on error pages).
function reloadContent() {
    if (!document.getElementById('page-content')) {
        reloadPage();
        return;
    }
    const slug  = history.state && history.state.slug;
    const query = history.state && history.state.query || '';
    if (!slug) {
        reloadPage();
        return;
    }
    fireEvent(new DeselectEvent());
    fetchContent(slug, '#page-content', query, null);
}

function resetPage() {
    let url = window.location.href;
    const pound = url.indexOf('#');
    url = pound !== -1 ? url.slice(0, pound) : url;
    window.location.href = url;
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
// Falls back to a full navigation when `#page-content` is absent (e.g. on error
// pages) or when the target path is not a known SPA page.
function navigateTo(url) {
    const parsed = new URL(url, window.location.origin);
    const slug = PAGE_SLUGS[parsed.pathname];
    if (!slug || !document.getElementById('page-content')) {
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

// Replaces the content of the given container with `html`, making sure that our state is refreshed
// properly (e.g., clearing previously registered event handlers).
function replaceContent(html, containerId = "#page-content") {
    clearContentHandler(containerId.slice(1));
    $(containerId).html(html);
    if(typeof resizeBoxes === 'function')
        resizeBoxes();
}

// Fetches the content fragment for `pageSlug` and injects it into `containerId`.
// `queryStr` is a pre-encoded query string such as `"keywords=foo&page=1"` or
// `"date=2025-03"` (pass an empty string for no parameters). `onLoaded` is an
// optional callback invoked after the HTML has been injected and `resizeBoxes`
// has run. Does not touch browser history.
function fetchContent(pageSlug, containerId, queryStr, onLoaded) {
    const params = queryStr ? '?' + queryStr : '';
    getRequest('/pages/' + pageSlug + '/content' + params, function(html) {
        replaceContent(html, containerId);
        if(onLoaded)
            onLoaded();
    }, 'html');
}

// Reloads the sidebar by re-fetching its content fragment. This is a no-op when
// the sidebar placeholder is absent (e.g. on error pages).
function reloadSidebar() {
    if (document.getElementById('sidebar-content'))
        fetchContent('sidebar', '#sidebar-content', '', null);
}

// Navigates to a new state for `pageSlug` by fetching the content fragment and
// pushing a new history entry. Delegates rendering to `fetchContent`.
function loadPageContent(pageSlug, containerId, queryStr, onLoaded) {
    // remember history
    const params = queryStr ? '?' + queryStr : '';
    history.pushState({ slug: pageSlug, query: queryStr || '' }, '', '/pages/' + pageSlug + params);
    // close popup in case there is one
    fireEvent(new DeselectEvent());
    // now replace content block
    fetchContent(pageSlug, containerId, queryStr, onLoaded);
}
