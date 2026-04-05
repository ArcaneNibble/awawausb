// This is the "main" content script.
//
// It has no privileges, and almost everything can potentially
// be stepped on by malicious page scripts.
//
// We choose to implement the bulk of the API here, but treating
// all data as suspect within the *background* script.

class USBConnectionEvent extends Event {
    get foo() {
        return "foo!";
    }
}

class USB extends EventTarget {
    test() {
        console.log("test?");
        window.__awawausb_testfunc(123);
    }
}

navigator.usb = new USB();
