// This is the "main" content script.
//
// It has no privileges, and almost everything can potentially
// be stepped on by malicious page scripts.
//
// We choose to implement the bulk of the API here, but treating
// all data as suspect within the *background* script.

(() => {
    function message_from_background(m) {
        console.log("reply from awawausb", m);
    }
    __awawausb_register_callback(message_from_background);
})();

class USBConnectionEvent extends Event {
    get foo() {
        return "foo!";
    }
}

class USB extends EventTarget {
    test(x) {
        console.log("test?");
        window.__awawausb_send_request(x);
    }
}

navigator.usb = new USB();
