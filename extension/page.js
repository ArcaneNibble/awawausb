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
    async test(x) {
        console.log("test?");
        console.log(await window.__awawausb_send_request({
            type: "echo",
            msg: x,
        }));
    }
}

navigator.usb = new USB();
