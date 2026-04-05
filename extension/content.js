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
console.log("content port", port);

function testfunc(x) {
    console.log("test from content", x);
    port.postMessage(x);
}

exportFunction(testfunc, window, { defineAs: "__awawausb_testfunc" });
