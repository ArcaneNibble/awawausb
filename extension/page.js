// This is the "main" content script.
//
// It has no privileges, and almost everything can potentially
// be stepped on by malicious page scripts.
//
// We choose to implement the bulk of the API here, but treating
// all data as suspect within the *background* script.

(function() {
    // Get, and then immediately hide, our interface methods with the content script
    const __awawausb_send_request = window.__awawausb_send_request;
    delete window.__awawausb_send_request;

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

    // Classes for the USB devices
    const DEV_DEVID = Symbol("USBDevice.device_id");
    window.USBDevice = class {
        #device_id
        constructor(devid) {
            // This song-and-dance helps to prevent user code from
            // trying to construct a USBDevice manually.
            // It's *NOT* a security boundary, since there's always
            // a chance __awawausb_send_request can leak.
            if (devid === undefined) {
                throw new TypeError("Illegal constructor");
            }
            let devid_ = devid[DEV_DEVID];
            if (devid_ === undefined) {
                throw new TypeError("Illegal constructor");
            }
            this.#device_id = devid_;
        }
        get test() {
            return this.#device_id;
        }
    };

    const DEV_DESC_PARENT = Symbol("awawausb.descriptor_parent");
    window.USBConfiguration = class {
        [DEV_DESC_PARENT];
        constructor(device, configurationValue) {
            if (!(device instanceof USBDevice)) {
                throw new TypeError("expected a USBDevice");
            }
            this[DEV_DESC_PARENT] = device;
        }
    };

    window.USBInterface = class {
        [DEV_DESC_PARENT];
        constructor(configuration, interfaceNumber) {
            if (!(configuration instanceof USBConfiguration)) {
                throw new TypeError("expected a USBConfiguration");
            }
            this[DEV_DESC_PARENT] = configuration;
        }
    };

    window.USBAlternateInterface = class {
        [DEV_DESC_PARENT];
        constructor(deviceInterface, alternateSetting) {
            if (!(deviceInterface instanceof USBInterface)) {
                throw new TypeError("expected a USBInterface");
            }
            this[DEV_DESC_PARENT] = deviceInterface;
        }
    };

    window.USBEndpoint = class {
        [DEV_DESC_PARENT];
        constructor(alternate, endpointNumber, direction) {
            if (!(alternate instanceof USBAlternateInterface)) {
                throw new TypeError("expected a USBAlternateInterface");
            }
            this[DEV_DESC_PARENT] = alternate;
        }
    };

    // "Global" objects and event handling
    window.USBConnectionEvent = class extends Event {
        #device
        constructor(type, eventInitDict) {
            super(type, eventInitDict);
            let device = eventInitDict.device;
            if (device === undefined) {
                throw new TypeError("missing `device` in USBConnectionEventInit");
            }
            if (!(device instanceof USBDevice)) {
                throw new TypeError("expected a USBDevice");
            }
            this.#device = device;
        }
        get device() {
            return this.#device;
        }
    };

    let allow_usb_to_construct = true;
    window.USB = class extends EventTarget {
        constructor() {
            if (!allow_usb_to_construct) {
                throw new TypeError("Illegal constructor");
            }
            super();
        }
        async test(x) {
            console.log("test?");
            console.log(await __awawausb_send_request({
                type: "echo",
                msg: x,
            }));
        }
        test2() {
            let devid = {};
            devid[DEV_DEVID] = 12345;
            let dev = new USBDevice(devid);
            return dev;
        }

        // Event dispatching
        #onconnect;
        #onconnect_set = false;
        #ondisconnect;
        #ondisconnect_set = false;

        get onconnect() {
            return this.#onconnect;
        }
        set onconnect(x) {
            this.#onconnect = x;
            if (!this.#onconnect_set) {
                let that = this;
                this.addEventListener('connect', function (e) {
                    that.#onconnect.call(this, e);
                });
            }
            this.#onconnect_set = true;
        }

        get ondisconnect() {
            return this.#ondisconnect;
        }
        set ondisconnect(x) {
            this.#ondisconnect = x;
            if (!this.#ondisconnect_set) {
                let that = this;
                this.addEventListener('disconnect', function (e) {
                    that.#ondisconnect.call(this, e);
                });
            }
            this.#ondisconnect_set = true;
        }
    };
    navigator.usb = new USB();
    allow_usb_to_construct = false;
})();
