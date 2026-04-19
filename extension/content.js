// This is the "isolated" content script.
//
// It has the ability to send messages to the background script.
// It is loaded many times, into every frame.
//
// The primary purpose of this script is to proxy information
// back and forth between the background script and the "main"
// script which runs within the page context. This is so that
// we do not need to go through very complicated mechanisms to
// export an entire class to the page. All of the "complicated"
// objects exist in the page "main" script only, and we only
// expose a single async function.
//
// This code *does* need to pair up async requests and responses,
// using a transaction ID number.

(function() {
    if (!window.isSecureContext) {
        return;
    }

    let port = browser.runtime.connect();

    // Hopefully this is safe and cannot be hijacked?
    // Need a way to call into page context to send events
    let page_event_cb;
    function set_page_event_cb(cb) {
        if (page_event_cb !== undefined)
            throw new Error("event callback already set");
        page_event_cb = cb;
    }
    exportFunction(set_page_event_cb, window, { defineAs: "__awawausb_set_event_cb" });

    let txn_id = 0;
    let txn_map = new Map();

    port.onMessage.addListener((m) => {
        if (m.event !== undefined) {
            if (page_event_cb !== undefined) {
                page_event_cb(cloneInto(m, window));
            }
            return;
        }

        let [resolve, reject] = txn_map.get(m.txn_id);
        txn_map.delete(m.txn_id);

        if (m.success) {
            resolve(cloneInto(m, window));
        } else {
            reject(cloneInto(m, window));
        }
    });

    function send_request(x) {
        let resolve, reject;
        let promise = new window.Promise((res, rej) => {
            resolve = res;
            reject = rej;
        });
        let this_txn_id = txn_id++;
        txn_map.set(this_txn_id, [resolve, reject]);
        x.txn_id = this_txn_id;
        port.postMessage(x);
        return promise;
    }
    exportFunction(send_request, window, { defineAs: "__awawausb_send_request" });
})();
