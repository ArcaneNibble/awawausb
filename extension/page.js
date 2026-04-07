// This is the "main" content script.
//
// It has no privileges, and almost everything can potentially
// be stepped on by malicious page scripts.
//
// We choose to implement the bulk of the API here, but treating
// all data as suspect within the *background* script.

(function() {
    const DEBUG_DISABLE_TRANSIENT_ACTIVATION = true;

    // "Global" objects and event handling
    let the_usb_obj;
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

    // Get, and then immediately hide, our interface methods with the content script
    const __awawausb_send_request = window.__awawausb_send_request;
    delete window.__awawausb_send_request;

    // Map from numeric device handles to objects
    // (needed to maintain object equality when opening
    //  the same device over and over again)
    let dev_handle_to_obj_map = new Map();

    // Set up our local event dispatcher, and then hide the method
    function notification_handler(m) {
        if (m.event === "unplug") {
            let dev = dev_handle_to_obj_map.get(m.dev_handle);
            if (dev !== undefined) {
                dev_handle_to_obj_map.delete(m.dev_handle);
                the_usb_obj.dispatchEvent(new USBConnectionEvent('disconnect', {device: dev}));
            }
        } else if (m.event === "plug") {
            let usb_device = new USBDevice({
                [DEV_HANDLE]: m.dev_handle,
                descriptors: m.dev_data,
            });
            dev_handle_to_obj_map.set(m.dev_handle, usb_device);
            the_usb_obj.dispatchEvent(new USBConnectionEvent('connect', {device: usb_device}));
        } else {
            console.warn("Unknown WebUSB notification", m);
        }
    }
    window.__awawausb_set_event_cb(notification_handler);
    delete window.__awawausb_set_event_cb;

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

    // Communication protocol support
    function map_txn_error(e) {
        if (e.error === "not_found") {
            throw new DOMException("Device unplugged or not found", "NotFoundError");
        } else if (e.error === "not_open") {
            throw new DOMException("Device not open or interface not claimed", "InvalidStateError");
        } else if (e.error === "not_configured") {
            throw new DOMException("Device not configured", "InvalidStateError");
        } else if (e.error === "abort") {
            throw new DOMException("Transfer aborted", "AbortError");
        } else if (e.error === "invalid_value") {
            throw new DOMException("Specified USB value is not valid", "NotFoundError");
        } else if (e.error === "already_claimed") {
            throw new DOMException("Device or interface already open or claimed", "NetworkError");
        } else if (e.error === "bad_ep_type") {
            throw new DOMException("Wrong endpoint type", "InvalidAccessError");
        } else {
            console.log("ERR", e);
            throw new DOMException("Transfer error", "NetworkError");
        }
    }

    function check_control_xfer_params(inp) {
        if (inp.requestType !== "standard"
            && inp.requestType !== "class"
            && inp.requestType !== "vendor")
            throw new TypeError(`\`${inp.requestType}\` is not a valid USBRequestType`);
        if (inp.recipient !== "device"
            && inp.recipient !== "interface"
            && inp.recipient !== "endpoint"
            && inp.recipient !== "other")
            throw new TypeError(`\`${inp.recipient}\` is not a valid USBRecipient`);
        return {
            requestType: inp.requestType,
            recipient: inp.recipient,
            request: inp.request,
            value: inp.value,
            index: inp.index,
        };
    }

    // Classes for the USB devices
    const DEV_HANDLE = Symbol("USBDevice.device_handle");
    const DEV_DESC = Symbol("USBDevice.descriptors");
    const BACKDOOR_SET_ACTIVE_IFACE = Symbol("USBDevice._backdoor_set_active_interface");
    const BACKDOOR_IS_CLAIMED = Symbol("USBDevice._backdoor_is_claimed");
    window.USBDevice = class {
        #device_handle;
        [DEV_DESC];
        #configurations;
        #active_config;

        // This state is tracked twice, but the state here is mostly useless.
        // The state here is only used as indications to user code.
        #opened = false;
        [BACKDOOR_IS_CLAIMED] = new Array();

        constructor(dev_data) {
            // This song-and-dance helps to prevent user code from
            // trying to construct a USBDevice manually.
            // It's *NOT* a security boundary, since there's always
            // a chance __awawausb_send_request can leak.
            if (dev_data === undefined) {
                throw new TypeError("Illegal constructor");
            }
            let devid_ = dev_data[DEV_HANDLE];
            if (devid_ === undefined) {
                throw new TypeError("Illegal constructor");
            }
            this.#device_handle = devid_;
            this[DEV_DESC] = dev_data.descriptors;

            // Now we need to mangle all the descriptors yet again
            let active_config = null;
            let configurations = new Array();
            for (let conf of dev_data.descriptors.configs) {
                let this_conf = new USBConfiguration(this, conf.bConfigurationValue);
                configurations.push(this_conf);
                if (conf.bConfigurationValue === dev_data.descriptors.current_config)
                    active_config = this_conf;
            }
            Object.freeze(configurations);
            this.#configurations = configurations;
            this.#active_config = active_config;
        }

        // Here are the actual USB methods
        async open() {
            try {
                await __awawausb_send_request({
                    type: "open",
                    dev_handle: this.#device_handle,
                });

                // Successful open?
                this.#opened = true;
            } catch (e) {
                map_txn_error(e);
            }
        }
        async close() {
            try {
                await __awawausb_send_request({
                    type: "close",
                    dev_handle: this.#device_handle,
                });

                // Successful close?
                this.#opened = false;
                this[BACKDOOR_IS_CLAIMED] = new Array();
            } catch (e) {
                map_txn_error(e);
            }
        }

        async forget() {
            await __awawausb_send_request({
                type: "forget",
                dev_handle: this.#device_handle,
            });
        }

        async reset() {
            try {
                await __awawausb_send_request({
                    type: "reset",
                    dev_handle: this.#device_handle,
                });
            } catch (e) {
                map_txn_error(e);
            }
        }

        async selectConfiguration(configurationValue) {
            try {
                configurationValue = configurationValue & 0xff;

                await __awawausb_send_request({
                    type: "set_config",
                    dev_handle: this.#device_handle,
                    configurationValue,
                });

                this[BACKDOOR_IS_CLAIMED] = new Array();
                let found_conf = null;
                for (let conf of this.#configurations) {
                    if (conf.configurationValue === configurationValue) {
                        found_conf = conf;
                        break;
                    }
                }
                this.#active_config = found_conf;
                for (let iface of this.#active_config.interfaces) {
                    this[BACKDOOR_IS_CLAIMED][iface.interfaceNumber] = false;
                    let found_alt = null;
                    for (let alt of iface.alternates) {
                        if (alt.alternateSetting === 0) {
                            found_alt = alt;
                            break;
                        }
                    }
                    iface[BACKDOOR_SET_ACTIVE_IFACE](found_alt);
                }
            } catch (e) {
                map_txn_error(e);
            }
        }

        async claimInterface(interfaceNumber) {
            try {
                interfaceNumber = interfaceNumber & 0xff;

                await __awawausb_send_request({
                    type: "claim_interface",
                    dev_handle: this.#device_handle,
                    interfaceNumber,
                });

                this[BACKDOOR_IS_CLAIMED][interfaceNumber] = true;
            } catch (e) {
                map_txn_error(e);
            }
        }
        async releaseInterface(interfaceNumber) {
            try {
                interfaceNumber = interfaceNumber & 0xff;

                await __awawausb_send_request({
                    type: "release_interface",
                    dev_handle: this.#device_handle,
                    interfaceNumber,
                });

                this[BACKDOOR_IS_CLAIMED][interfaceNumber] = false;
            } catch (e) {
                map_txn_error(e);
            }
        }
        async selectAlternateInterface(interfaceNumber, alternateSetting) {
            try {
                interfaceNumber = interfaceNumber & 0xff;
                alternateSetting = alternateSetting & 0xff;

                await __awawausb_send_request({
                    type: "set_alt_interface",
                    dev_handle: this.#device_handle,
                    interfaceNumber,
                    alternateSetting,
                });

                for (let iface of this.#active_config.interfaces) {
                    if (iface.interfaceNumber === interfaceNumber) {
                        let found_alt = null;
                        for (let alt of iface.alternates) {
                            if (alt.alternateSetting === alternateSetting) {
                                found_alt = alt;
                                break;
                            }
                        }
                        iface[BACKDOOR_SET_ACTIVE_IFACE](found_alt);
                    }
                }
            } catch (e) {
                map_txn_error(e);
            }
        }

        async controlTransferIn(setup, length) {
            setup = check_control_xfer_params(setup);
            try {
                let res = await __awawausb_send_request({
                    type: "ctrl_xfer",
                    dev_handle: this.#device_handle,
                    setup,
                    length: length & 0xffff
                });

                let data = new DataView(res.data.buffer);
                let ok_babble = res.babble ? "babble" : "ok";
                return new USBInTransferResult(ok_babble, data);
            } catch (e) {
                if (e.error === "stall")
                    return new USBInTransferResult("stall");
                map_txn_error(e);
            }
        }
        async controlTransferOut(setup, data) {
            setup = check_control_xfer_params(setup);
            if (!(data instanceof ArrayBuffer) && !ArrayBuffer.isView(data)) {
                throw new TypeError("parameter is not a BufferSource");
            }
            try {
                let res = await __awawausb_send_request({
                    type: "ctrl_xfer",
                    dev_handle: this.#device_handle,
                    setup,
                    data,
                });

                return new USBOutTransferResult("ok", res.bytes_written);
            } catch (e) {
                if (e.error === "stall")
                    return new USBOutTransferResult("stall", e.bytes_written);
                map_txn_error(e);
            }
        }

        async transferIn(endpointNumber, length) {
            endpointNumber = (endpointNumber & 0xf) | 0x80;
            try {
                let res = await __awawausb_send_request({
                    type: "data_xfer",
                    dev_handle: this.#device_handle,
                    endpointNumber,
                    length: length & 0xffffffff
                });

                let data = new DataView(res.data.buffer);
                let ok_babble = res.babble ? "babble" : "ok";
                return new USBInTransferResult(ok_babble, data);
            } catch (e) {
                if (e.error === "stall")
                    return new USBInTransferResult("stall");
                map_txn_error(e);
            }
        }
        async transferOut(endpointNumber, data) {
            endpointNumber = (endpointNumber & 0xf);
            if (!(data instanceof ArrayBuffer) && !ArrayBuffer.isView(data)) {
                throw new TypeError("parameter is not a BufferSource");
            }
            try {
                let res = await __awawausb_send_request({
                    type: "data_xfer",
                    dev_handle: this.#device_handle,
                    endpointNumber,
                    data,
                });

                return new USBOutTransferResult("ok", res.bytes_written);
            } catch (e) {
                if (e.error === "stall")
                    return new USBOutTransferResult("stall", e.bytes_written);
                map_txn_error(e);
            }
        }
        async clearHalt(direction, endpointNumber) {
            endpointNumber = (endpointNumber & 0xf);
            if (direction === "in") {
                endpointNumber |= 0x80;
            } else if (direction === "out") {}
            else {
                throw new TypeError(`\`${direction}\` is not a valid USBDirection`);
            }
            try {
                await __awawausb_send_request({
                    type: "clear_halt",
                    dev_handle: this.#device_handle,
                    endpointNumber,
                });
            } catch (e) {
                map_txn_error(e);
            }
        }


        async isochronousTransferIn(endpointNumber, packetLengths) {
            endpointNumber = (endpointNumber & 0xf) | 0x80;
            packetLengths = Array.from(packetLengths, (x) => x & 0xffffffff);
            try {
                let res = await __awawausb_send_request({
                    type: "isoc_xfer",
                    dev_handle: this.#device_handle,
                    endpointNumber,
                    packetLengths,
                });

                console.log("isoc in done", res);
            } catch (e) {
                map_txn_error(e);
            }
        }
        async isochronousTransferOut(endpointNumber, data, packetLengths) {
            endpointNumber = (endpointNumber & 0xf);
            if (!(data instanceof ArrayBuffer) && !ArrayBuffer.isView(data)) {
                throw new TypeError("parameter is not a BufferSource");
            }
            packetLengths = Array.from(packetLengths, (x) => x & 0xffffffff);
            try {
                let res = await __awawausb_send_request({
                    type: "isoc_xfer",
                    dev_handle: this.#device_handle,
                    endpointNumber,
                    data,
                    packetLengths,
                });

                console.log("isoc out done", res);
            } catch (e) {
                map_txn_error(e);
            }
        }

        get usbVersionMajor() {
            return (this[DEV_DESC].bcdUSB >> 8) & 0xff;
        }
        get usbVersionMinor() {
            return (this[DEV_DESC].bcdUSB >> 4) & 0xf;
        }
        get usbVersionSubminor() {
            return this[DEV_DESC].bcdUSB & 0xf;
        }
        get deviceClass() {
            return this[DEV_DESC].bDeviceClass;
        }
        get deviceSubclass() {
            return this[DEV_DESC].bDeviceSubClass;
        }
        get deviceProtocol() {
            return this[DEV_DESC].bDeviceProtocol;
        }
        get vendorId() {
            return this[DEV_DESC].idVendor;
        }
        get productId() {
            return this[DEV_DESC].idProduct;
        }
        get deviceVersionMajor() {
            return (this[DEV_DESC].bcdDevice >> 8) & 0xff;
        }
        get deviceVersionMinor() {
            return (this[DEV_DESC].bcdDevice >> 4) & 0x4f;
        }
        get deviceVersionSubminor() {
            return this[DEV_DESC].bcdDevice & 0xf;
        }
        get manufacturerName() {
            return this[DEV_DESC].manufacturer;
        }
        get productName() {
            return this[DEV_DESC].product;
        }
        get serialNumber() {
            return this[DEV_DESC].serial;
        }

        get configuration() {
            return this.#active_config;
        }
        get configurations() {
            return this.#configurations;
        }

        get opened() {
            return this.#opened;
        }
    };

    // FIXME: The below constructors do a bunch of redundant iterating
    const DEV_DESC_PARENT = Symbol("awawausb.descriptor_parent");
    window.USBConfiguration = class {
        [DEV_DESC_PARENT];
        [DEV_DESC];
        #configurationValue;
        #configurationName;
        #interfaces;
        constructor(device, configurationValue) {
            if (!(device instanceof USBDevice)) {
                throw new TypeError("expected a USBDevice");
            }
            this[DEV_DESC_PARENT] = device;

            let dev_desc = device[DEV_DESC];
            for (let conf of dev_desc.configs) {
                if (conf.bConfigurationValue === configurationValue) {
                    this[DEV_DESC] = conf;
                    this.#configurationValue = conf.bConfigurationValue;
                    this.#configurationName = conf.config_name;

                    let interfaces = new Array();
                    for (let iface of conf.interfaces) {
                        interfaces.push(new USBInterface(this, iface.bInterfaceNumber));
                    }
                    Object.freeze(interfaces);
                    this.#interfaces = interfaces;
                    return;
                }
            }

            throw new RangeError(`configuration ${configurationValue} invalid`)
        }

        get configurationValue() {
            return this.#configurationValue
        }
        get configurationName() {
            return this.#configurationName
        }
        get interfaces() {
            return this.#interfaces;
        }
    };

    window.USBInterface = class {
        [DEV_DESC_PARENT];
        [DEV_DESC];
        #interfaceNumber;
        #alts;
        #active_alt;
        constructor(configuration, interfaceNumber) {
            if (!(configuration instanceof USBConfiguration)) {
                throw new TypeError("expected a USBConfiguration");
            }
            this[DEV_DESC_PARENT] = configuration;

            let conf_desc = configuration[DEV_DESC];
            for (let iface of conf_desc.interfaces) {
                if (iface.bInterfaceNumber === interfaceNumber) {
                    this[DEV_DESC] = iface;
                    this.#interfaceNumber = iface.bInterfaceNumber;

                    let active_alt = null;
                    let alts = new Array();
                    for (let alt of iface.alts) {
                        let this_alt = new USBAlternateInterface(this, alt.bAlternateSetting);
                        alts.push(this_alt);
                        if (alt.bAlternateSetting === iface.current_alt_setting)
                            active_alt = this_alt;
                    }
                    Object.freeze(alts);
                    this.#alts = alts;
                    this.#active_alt = active_alt;
                    return;
                }
            }

            throw new RangeError(`interface ${interfaceNumber} invalid`)
        }

        [BACKDOOR_SET_ACTIVE_IFACE](iface) {
            this.#active_alt = iface;
        }

        get interfaceNumber() {
            return this.#interfaceNumber;
        }
        get alternate() {
            return this.#active_alt;
        }
        get alternates() {
            return this.#alts;
        }

        get claimed() {
            let conf_obj = this[DEV_DESC_PARENT];
            let dev_obj = conf_obj[DEV_DESC_PARENT];
            return !!dev_obj[BACKDOOR_IS_CLAIMED][this.#interfaceNumber];
        }
    };

    const EP_DIRS = ["out", "in"];
    const EP_TYPES = ["control", "isochronous", "bulk", "interrupt"];

    window.USBAlternateInterface = class {
        [DEV_DESC];
        #alternateSetting;
        #interfaceClass;
        #interfaceSubclass;
        #interfaceProtocol;
        #interfaceName;
        #endpoints;
        constructor(deviceInterface, alternateSetting) {
            if (!(deviceInterface instanceof USBInterface)) {
                throw new TypeError("expected a USBInterface");
            }

            let iface_desc = deviceInterface[DEV_DESC];
            for (let alt of iface_desc.alts) {
                if (alt.bAlternateSetting === alternateSetting) {
                    this[DEV_DESC] = alt;
                    this.#alternateSetting = alt.bAlternateSetting;
                    this.#interfaceClass = alt.bInterfaceClass;
                    this.#interfaceSubclass = alt.bInterfaceSubClass;
                    this.#interfaceProtocol = alt.bInterfaceProtocol;
                    this.#interfaceName = alt.intf_name;

                    let endpoints = new Array();
                    for (let ep of alt.endpoints) {
                        endpoints.push(new USBEndpoint(this,
                            ep.bEndpointAddress & 0xf, EP_DIRS[(ep.bEndpointAddress >> 7) & 1]));
                    }
                    Object.freeze(endpoints);
                    this.#endpoints = endpoints;
                    return;
                }
            }

            throw new RangeError(`interface ${deviceInterface.alternateSetting} alt ${alternateSetting} invalid`)
        }

        get alternateSetting() {
            return this.#alternateSetting;
        }
        get interfaceClass() {
            return this.#interfaceClass;
        }
        get interfaceSubclass() {
            return this.#interfaceSubclass;
        }
        get interfaceProtocol() {
            return this.#interfaceProtocol;
        }
        get interfaceName() {
            return this.#interfaceName;
        }
        get endpoints() {
            return this.#endpoints
        }
    };

    window.USBEndpoint = class {
        #endpointNumber;
        #direction;
        #type;
        #packetSize;
        constructor(alternate, endpointNumber, direction) {
            if (!(alternate instanceof USBAlternateInterface)) {
                throw new TypeError("expected a USBAlternateInterface");
            }

            let addr = endpointNumber;
            if (direction === "in") {
                addr |= 0x80;
            } else if (direction === "out") {}
            else {
                throw new TypeError(`\`${direction}\` is not a valid USBDirection`);
            }

            let iface_desc = alternate[DEV_DESC];
            for (let ep of iface_desc.endpoints) {
                if (ep.bEndpointAddress === addr) {
                    this.#endpointNumber = endpointNumber;
                    this.#direction = direction;
                    this.#type = EP_TYPES[ep.bmAttributes & 3];
                    // XXX the spec is entirely contradictory as to how to interpret packetSize
                    this.#packetSize = ep.wMaxPacketSize;
                    return;
                }
            }

            throw new RangeError(`endpoint ${endpointNumber} ${direction} invalid`)
        }

        get endpointNumber() {
            return this.#endpointNumber;
        }
        get direction() {
            return this.#direction;
        }
        get type() {
            return this.#type;
        }
        get packetSize() {
            return this.#packetSize;
        }
    };

    function handle_null_undef(x) {
        if (x === null || x === undefined)
            return null;
        return x;
    }
    function validate_filter(filt_in) {
        let filt_out = {
            vendorId: handle_null_undef(filt_in.vendorId),
            productId: handle_null_undef(filt_in.productId),
            classCode: handle_null_undef(filt_in.classCode),
            subclassCode: handle_null_undef(filt_in.subclassCode),
            protocolCode: handle_null_undef(filt_in.protocolCode),
            serialNumber: handle_null_undef(filt_in.serialNumber),
        };

        // > A USBDeviceFilter filter is valid if the following steps return valid
        if (filt_out.productId !== null && filt_out.vendorId === null)
            throw new TypeError("Invalid USBDeviceFilter");
        if (filt_out.subclassCode !== null && filt_out.classCode === null)
            throw new TypeError("Invalid USBDeviceFilter");
        if (filt_out.protocolCode !== null && filt_out.subclassCode === null)
            throw new TypeError("Invalid USBDeviceFilter");
        return filt_out;
    }

    let allow_usb_to_construct = true;
    window.USB = class extends EventTarget {
        constructor() {
            if (!allow_usb_to_construct) {
                throw new TypeError("Illegal constructor");
            }
            super();
        }

        // Actual functionality
        async requestDevice(options) {
            // Validate args
            let filters = options.filters;
            if (filters === undefined) {
                throw new TypeError("missing `filters` in USBDeviceRequestOptions");
            }
            let exclusionFilters = options.exclusionFilters;
            if (exclusionFilters === undefined) {
                exclusionFilters = [];
            }

            filters = Array.from(filters, x => validate_filter(x));
            exclusionFilters = Array.from(exclusionFilters, x => validate_filter(x));

            if (!DEBUG_DISABLE_TRANSIENT_ACTIVATION) {
                // Check for transient activation
                // This is (somewhat) of a security feature,
                // but not one worth being extremely paranoid over.
                if (!navigator.userActivation.isActive) {
                    throw new DOMException("requestDevice() requires transient activation!", "SecurityError");
                }
            }

            try {
                let resp = await __awawausb_send_request({
                    type: "request_device",
                    filters,
                    exclusionFilters,
                });

                let existing_device = dev_handle_to_obj_map.get(resp.dev_handle);
                if (existing_device !== undefined)
                    return existing_device;

                let usb_device = new USBDevice({
                    [DEV_HANDLE]: resp.dev_handle,
                    descriptors: resp.dev_data,
                });
                dev_handle_to_obj_map.set(resp.dev_handle, usb_device);
                return usb_device;
            } catch (e) {
                // Whatever the failure reason may be, we report it as "not found"
                throw new DOMException("No USB device found or selected", "NotFoundError");
            }
        }

        async getDevices() {
            // NOTE: We don't actually actively re-enumerate anything
            // It's fine to just return all cached devices.
            // This works because we are actively notified when devices are unplugged.
            return Array.from(dev_handle_to_obj_map.values());
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
    the_usb_obj = new USB();
    navigator.usb = the_usb_obj;
    allow_usb_to_construct = false;
})();
