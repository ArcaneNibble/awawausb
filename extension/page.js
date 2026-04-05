// This is the "main" content script.
//
// It has no privileges, and almost everything can potentially
// be stepped on by malicious page scripts.
//
// We choose to implement the bulk of the API here, but treating
// all data as suspect within the *background* script.

(function() {
    function check_xfer_status(status) {
        if (status === "ok") {
            return;
        }
        else if (status === "stall") {
            return;
        }
        else if (status === "babble") {
            return;
        }
        throw new TypeError(`\`${status}\` is not a valid USBTransferStatus`);
    }
    function check_dataview(dv) {
        if (dv === undefined) {
            return null;
        } else if (dv === null) {
            return null;
        } else if (dv instanceof DataView) {
            return dv;
        }
        throw new TypeError(`parameter is not a DataView`);
    }

    // Define misc interface types
    window.USBInTransferResult = class {
        #data;
        #status;
        constructor(status, data = null) {
            check_xfer_status(status);
            this.#status = status;
            this.#data = check_dataview(data);
        }
        get status() {
            return this.#status;
        }
        get data() {
            return this.#data;
        }
    };

    window.USBOutTransferResult = class {
        #bytesWritten;
        #status;
        constructor(status, bytesWritten = 0) {
            check_xfer_status(status);
            this.#status = status;
            this.#bytesWritten = bytesWritten>>>0;
        }
        get status() {
            return this.#status;
        }
        get bytesWritten() {
            return this.#bytesWritten;
        }
    };

    window.USBIsochronousInTransferPacket = class {
        #data;
        #status;
        constructor(status, data = null) {
            check_xfer_status(status);
            this.#status = status;
            this.#data = check_dataview(data);
        }
        get status() {
            return this.#status;
        }
        get data() {
            return this.#data;
        }
    };
    window.USBIsochronousInTransferResult = class {
        #data;
        #packets;
        constructor(packets, data = null) {
            let packets_ = Array.from(packets, (x) => {
                if (!(x instanceof USBIsochronousInTransferPacket))
                    throw new TypeError("expected a USBIsochronousInTransferPacket");
                return x;
            });
            Object.freeze(packets_);
            this.#packets = packets_;
            this.#data = check_dataview(data);
        }
        get packets() {
            return this.#packets;
        }
        get data() {
            return this.#data;
        }
    };

    window.USBIsochronousOutTransferPacket = class {
        #bytesWritten;
        #status;
        constructor(status, bytesWritten = 0) {
            check_xfer_status(status);
            this.#status = status;
            this.#bytesWritten = bytesWritten>>>0;
        }
        get status() {
            return this.#status;
        }
        get bytesWritten() {
            return this.#bytesWritten;
        }
    };
    window.USBIsochronousOutTransferResult = class {
        #packets;
        constructor(packets) {
            let packets_ = Array.from(packets, (x) => {
                if (!(x instanceof USBIsochronousOutTransferPacket))
                    throw new TypeError("expected a USBIsochronousOutTransferPacket");
                return x;
            });
            Object.freeze(packets_);
            this.#packets = packets_;
        }
        get packets() {
            return this.#packets;
        }
    };
})();

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
