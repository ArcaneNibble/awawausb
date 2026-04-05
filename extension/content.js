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
// objects exist in the page "main" script only; we only expose
// very simple functions.

let port = browser.runtime.connect();
let callback = undefined;

function register_callback(cb) {
    if (callback !== undefined)
        console.warn("awawausb callback already set!");
    else
        callback = cb;
}
exportFunction(register_callback, window, { defineAs: "__awawausb_register_callback" });

port.onMessage.addListener((m) => {
    // XXX is this actually safe?
    // It doesn't use any of the super-dangerous methods, and the callback is set only once,
    // and it's set to a function in a local scope. Hopefully this is fine?
    if (callback !== undefined)
        callback(m);
});

function send_request(x) {
    port.postMessage(x);
}
exportFunction(send_request, window, { defineAs: "__awawausb_send_request" });
