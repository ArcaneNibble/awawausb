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
