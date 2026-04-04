let port = browser.runtime.connect();
console.log("content port", port);

function testfunc(x) {
    console.log("test from content", x);
    port.postMessage(x);
}

exportFunction(testfunc, window, { defineAs: "__awawausb_testfunc" });
