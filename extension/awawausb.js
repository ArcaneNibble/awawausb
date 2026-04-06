// This is the extension's background script.
//
// It is _privileged_ and _persistent_, and it is responsible for
// maintaining all global state related to USB.
// It launches _once_ and maintains _one_ connection to the native stub.

// Open the debugging page
browser.browserAction.onClicked.addListener(() => {
    browser.tabs.create({
        url: "/debug-page/debug.html"
    });
});

// Connection to native stub
let nativeport = browser.runtime.connectNative("awawausb_native_stub");
nativeport.onDisconnect.addListener((p) => {
    console.warn("Native process disconnected!", p.error);
})

const WEBUSB_PLATFORM_CAPABILITY = [0x38, 0xB6, 0x08, 0x34, 0xA9, 0x09, 0xA0, 0x47, 0x8B, 0xFD, 0xA0, 0x76, 0x88, 0x15, 0xB6, 0x65];

// All USB devices we possibly know about, indexed by session ID
// XXX this structure is rather ad-hoc, because it contains a lot of Chapter-9 fields
let usb_devices = new Map();

function clean_up_usb_device_for_page(dev) {
    let ret = structuredClone(dev);

    delete ret.webusb_landing_page;

    return ret;
}

// All outstanding transactions, indexed by transaction ID,
// which is a string of the form `pageID-txnID`
// (page ID of 0 is reserved for special global control transfers)
let usb_txns = new Map();

// (Next) transaction ID for use in global scope
let usb_global_txn = 0;
const INTERNAL_CONTROL_XFER_TIMEOUT_MS = 1000;
function internal_perform_control_transfer(
    sid,
    request_type, request, value, index, length_or_buffer,
    timeout=INTERNAL_CONTROL_XFER_TIMEOUT_MS
) {
    let resolve, reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });

    let this_txn_id = usb_global_txn++;
    let txn_id = `0-${this_txn_id}`;
    usb_txns.set(txn_id, [resolve, reject]);

    let obj = {
        _timeout_internal: timeout,
        type: "ControlTransfer",
        sid: sid,
        txn_id,
        request_type,
        request,
        value,
        index,
    }
    if (request_type & 0x80) {
        // device to host
        obj.length = length_or_buffer;
    } else {
        // host to device
        obj.data = length_or_buffer.toBase64({ alphabet: "base64url", omitPadding: true });
    }
    nativeport.postMessage(obj);

    return promise;
}

// Page state

// What we need to know (here, globally) about USBDevice objects in pages
class PerPageUSBDevice {
    sid;
    constructor(sid) {
        this.sid = sid;
    }
}

// Actual per-page state. Pages are referenced by a numeric ID,
// and this state contains the reply messaging port as well as
// the "authoritative" copy of the page's opened devices (USBDevice objects)
class PerPageState {
    port;
    constructor(port) {
        this.port = port;
    }

    // Map from device handle (numeric ID) to authoritative state
    opened_devices = new Map();
    // "Reverse" map from session ID to device handle
    // This is used to make sure we don't open duplicate devices
    // TODO: How does this get invalidated?
    sid_to_handle = new Map();
    #next_device_id = 0;
    open_device(sid) {
        let existing_handle = this.sid_to_handle.get(sid);
        if (existing_handle !== undefined)
            return existing_handle;

        let device_state = new PerPageUSBDevice(sid);
        let this_device_handle = this.#next_device_id++;
        this.opened_devices.set(this_device_handle, device_state);
        this.sid_to_handle.set(sid, this_device_handle);
        return this_device_handle;
    }

    static #next_page_id = 1;
    static #pages = new Map();
    static new_page(port) {
        let state = new PerPageState(port);

        let this_page_id = PerPageState.#next_page_id++;
        console.log("new page port!", this_page_id, port.sender);
        PerPageState.#pages.set(this_page_id, state);

        return [this_page_id, state];
    }
    static delete_page(page_id) {
        console.log("page port disconnected!", page_id);
        // TODO: Clean up opened devices?
        PerPageState.#pages.delete(page_id);
    }
    static debug_pages() {
        let ret = new Array();
        for (let [page_id, state] of PerPageState.#pages) {
            let handles = new Array();
            for (let [handle_id, usb_dev] of state.opened_devices) {
                handles.push([handle_id, usb_dev.sid]);
            }
            ret.push({
                page_id,
                url: state.port.sender.url,
                handles,
            });
        }
        return ret;
    }
}

function matches_iface_filter(iface, filt) {
    // > A USB interface interface matches an interface filter filter if the following steps return match
    if (filt.classCode !== null && iface.bInterfaceClass !== filt.classCode) return false;
    if (filt.subclassCode !== null && iface.bInterfaceSubClass !== filt.subclassCode) return false;
    if (filt.protocolCode !== null && iface.bInterfaceProtocol !== filt.protocolCode) return false;
    return true;
}

function matches_device_filter(dev, filt) {
    // > A USB device device matches a device filter filter if the following steps return match
    if (filt.vendorId !== null && dev.idVendor !== filt.vendorId) return false;
    if (filt.productId !== null && dev.idProduct !== filt.productId) return false;
    if (filt.serialNumber !== null && dev.serial !== filt.serialNumber) return false;
    if (filt.classCode !== null) {
        for (let cfg of dev.configs) {
            for (let iface of cfg.interfaces) {
                for (let iface_alt of iface.alts) {
                    if (matches_iface_filter(iface_alt, filt))
                        return true;
                }
            }
        }
        // TODO: interface filter
    }
    if (filt.classCode !== null && dev.bDeviceClass !== filt.classCode) return false;
    if (filt.subclassCode !== null && dev.bDeviceSubClass !== filt.subclassCode) return false;
    if (filt.protocolCode !== null && dev.bDeviceProtocol !== filt.protocolCode) return false;
    return true;
}

// Handle requests from pages
browser.runtime.onConnect.addListener((p) => {
    // These are *internal* pages which have special permissions
    if (p.sender.id === "awawausb@arcanenibble.com" && p.sender.url.startsWith("moz-extension://")) {
        if (p.sender.url.endsWith("/debug-page/debug.html")) {
            p.onMessage.addListener((m) => {
                if (m === "list_devices") {
                    p.postMessage({
                        type: m,
                        devices: usb_devices,
                    })
                } else if (m === "list_pages") {
                    p.postMessage({
                        type: m,
                        pages: PerPageState.debug_pages(),
                    });
                } else if (m === "list_txns") {
                    let txns = new Array();
                    for (let txn_id of usb_txns.keys()) {
                        txns.push(txn_id);
                    }
                    p.postMessage({
                        type: m,
                        txns,
                    });
                } else {
                    console.warn("Debug page sent bad request!", m)
                }
            });
        }
        return;
    }

    // Create and stash the per-page state
    let [this_page_id, this_page] = PerPageState.new_page(p);

    p.onDisconnect.addListener((_p) => {
        PerPageState.delete_page(this_page_id);
    });

    p.onMessage.addListener((m) => {
        if (m.type === "echo") {
            p.postMessage({
                txn_id: m.txn_id,
                success: true,
                msg: m.msg,
            });
        } else if (m.type === "request_device") {
            let filters = m.filters;
            let exclusionFilters = m.exclusionFilters;

            let possible_devices = new Array();
            for (let [sid, usb_dev] of usb_devices) {
                let matches_a_filter = false;
                for (let filt of filters) {
                    if (matches_device_filter(usb_dev, filt)) {
                        matches_a_filter = true;
                        break;
                    }
                }
                // XXX if there are no filters, accept the device
                // This appears to be contrary to the pedantic wording of the spec,
                // but it's what Chrome does and it logically makes sense
                if (!matches_a_filter && filters.length > 0)
                    continue;

                let matches_an_exclusion = false;
                for (let filt of exclusionFilters) {
                    if (matches_device_filter(usb_dev, filt)) {
                        matches_an_exclusion = true;
                        break;
                    }
                }
                if (matches_an_exclusion)
                    continue;

                possible_devices.push(sid);
            }

            // If there's no devices, don't even bother to pop up a dialog, just fail
            if (possible_devices.length === 0) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                });
                return;
            }

            // TODO: Implement permission dialog
            let selected_sid = possible_devices[0];

            let dev_data = clean_up_usb_device_for_page(usb_devices.get(selected_sid));
            let dev_handle = this_page.open_device(selected_sid);
            p.postMessage({
                txn_id: m.txn_id,
                success: true,
                dev_handle,
                dev_data,
            });
        } else {
            console.warn("Unknown request from a page", m, p.sender.url);
            p.postMessage({
                txn_id: m.txn_id,
                success: false,
            });
        }
        // console.log("test from bkg", m);
        // // p.postMessage(m * 2);
        // // browser.windows.create({
        // //     type: "panel",
        // //     url: `/permission-page/permission.html?test=${m}`
        // // });

        // // Test
        // let this_txn_id = port_txn_id++;
        // let txn_id = `${this_port_id}-${this_txn_id}`;
        // // TODO: Do we need this? Can we get rid of this?
        // usb_txns.set(txn_id, true);

        // let obj = {
        //     _timeout_internal: 0,
        //     type: "ControlTransfer",
        //     sid: m,
        //     txn_id,
        //     request_type: 0xc0,
        //     request: 'e'.charCodeAt(0),
        //     value: 0,
        //     index: 0,
        //     length: 4,
        // }
        // nativeport.postMessage(obj);
    });
});

// Handle replies (and notifications) from native process
nativeport.onMessage.addListener(async (m) => {
    if (m.type === "NewDevice") {
        let sid = m.sid;
        if (usb_devices.has(sid)) {
            console.warn("Duplicate device??", sid);
        }

        console.log("Probing new USB device!", m.idVendor, m.idProduct);

        // Query extra data
        // We try as hard as possible to *not* generate unnecessary traffic,
        // but sometimes we can't avoid it (need strings, need webusb descriptor).

        // undefined -> we haven't tried to query it yet
        // null -> we tried to query it, but something went wrong
        // u16 -> use this language
        let usb_lang_id = undefined;
        async function get_lang_id() {
            if (usb_lang_id === undefined) {
                try {
                    // Query the langid string descriptor
                    let langid_desc = await internal_perform_control_transfer(sid, 0x80, 6, 0x0300, 0, 4);
                    if (langid_desc.data.length < 4 || langid_desc.data[1] != 0x03) {
                        usb_lang_id = null;
                    } else {
                        usb_lang_id = (langid_desc.data[3] << 8) | langid_desc.data[2];
                    }
                } catch (e) {
                    console.warn("Getting langid failed!", e);
                    usb_lang_id = null;
                }
                return usb_lang_id;
            } else {
                return usb_lang_id;
            }
        }

        async function get_string_desc(idx) {
            let langid = await get_lang_id();
            if (langid === null) return null;

            try {
                let initial_desc = await internal_perform_control_transfer(sid, 0x80, 6, 0x0300 | idx, langid, 4);
                if (initial_desc.data.length < 2 || initial_desc.data[1] != 0x03) return null;
                let actual_len = initial_desc.data[0];

                let string_desc = await internal_perform_control_transfer(sid, 0x80, 6, 0x0300 | idx, langid, actual_len);
                if (string_desc.data.length != actual_len || string_desc.data[1] != 0x03) return null;

                let str = "";
                let dv = new DataView(string_desc.data.buffer);
                for (let i = 2; i < string_desc.data.length; i += 2) {
                    str += String.fromCharCode(dv.getUint16(i, true));
                }
                return str;
            } catch (e) {
                console.warn("Getting string descriptor failed!", idx, langid);
                return null;
            }
        }

        // Big data shuffle, for descriptors
        let configs = new Array();
        for (let cfg of m.configs) {
            // Configuration name string
            let config_name = null;
            if (cfg.iConfiguration !== 0) {
                config_name = await get_string_desc(cfg.iConfiguration);
            }

            // Interfaces (some shuffling needed to be convenient for webusb order)
            let interfaces = new Array();
            let binterface_to_idx = new Map();
            for (let intf of cfg.interfaces) {
                // Shuffle
                if (!binterface_to_idx.has(intf.bInterfaceNumber)) {
                    let idx = interfaces.length;
                    interfaces.push({
                        bInterfaceNumber: intf.bInterfaceNumber,
                        alts: new Array(),
                    });
                    binterface_to_idx.set(intf.bInterfaceNumber, idx);
                }

                // Interface (alternate) name string
                let intf_name = null;
                if (intf.iInterface !== 0) {
                    intf_name = await get_string_desc(intf.iInterface);
                }

                // Endpoints
                let eps = new Array();
                for (let ep of intf.endpoints) {
                    eps.push(ep);
                }

                let intf_obj = interfaces[binterface_to_idx.get(intf.bInterfaceNumber)];
                if (intf_obj.current_alt_setting !== undefined && intf_obj.current_alt_setting !== intf.current_alt_setting) {
                    console.warn("Something fucky with alt settings?", intf.bInterfaceNumber, intf.bAlternateSetting);
                }
                intf_obj.current_alt_setting = intf.current_alt_setting;
                intf_obj.alts.push({
                    bAlternateSetting: intf.bAlternateSetting,
                    bInterfaceClass: intf.bInterfaceClass,
                    bInterfaceSubClass: intf.bInterfaceSubClass,
                    bInterfaceProtocol: intf.bInterfaceProtocol,

                    intf_name,
                    endpoints: eps,
                });
            }

            configs.push({
                bConfigurationValue: cfg.bConfigurationValue,
                config_name,
                interfaces,
            });
        }

        // Try to query the WebUSB descriptors
        // TODO: Do we need quirks here?
        let try_webusb = undefined;
        let webusb_landing_page = undefined;
        async function try_get_bos_desc() {
            try {
                // Fetch the BOS descriptor
                let initial_desc = await internal_perform_control_transfer(sid, 0x80, 6, 0x0f00, 0, 5);
                if (initial_desc.data.length < 5 || initial_desc.data[1] != 0x0f) return;
                let actual_len = (initial_desc.data[3] << 8) | initial_desc.data[2];

                let bos_desc = await internal_perform_control_transfer(sid, 0x80, 6, 0x0f00, 0, actual_len);
                if (bos_desc.data.length != actual_len || bos_desc.data[1] != 0x0f) return;

                let offs = 5;
                while (offs < bos_desc.data.length - 2) {
                    const desc_len = bos_desc.data[offs];
                    const desc_ty = bos_desc.data[offs + 1];

                    // XXX the desc_len check may have to change if this descriptor ever gets an update
                    if (desc_len >= 24 && desc_ty == 16 && bos_desc.data[offs + 2] == 5) {
                        // Platform capability descriptor
                        const uuid = new Uint8Array(bos_desc.data.buffer, offs + 4, 16);

                        // Compare UUID
                        let is_webusb = true;
                        for (let i = 0; i < 16; i++) {
                            if (uuid[i] != WEBUSB_PLATFORM_CAPABILITY[i]) {
                                is_webusb = false;
                                break;
                            }
                        }

                        if (is_webusb) {
                            let bcdVersion = (bos_desc.data[offs + 20 + 1] << 8) | bos_desc.data[offs + 20];
                            let bVendorCode = bos_desc.data[offs + 20 + 2];
                            let iLandingPage = bos_desc.data[offs + 20 + 3];

                            if (bcdVersion == 0x0100) {
                                try_webusb = {
                                    bVendorCode,
                                    iLandingPage
                                }
                                break;
                            }
                        }
                    }

                    offs += desc_len;
                }
            } catch (e) {
                console.log("BOS descriptor fetch failed!", e);
            }
        }
        async function try_get_webusb_desc() {
            try {
                // Fetch the URL descriptor
                let initial_desc = await internal_perform_control_transfer(sid,
                    0xC0, try_webusb.bVendorCode, try_webusb.iLandingPage, 2, 3);
                if (initial_desc.data.length < 3 || initial_desc.data[1] != 0x03) return;
                let actual_len = initial_desc.data[0];

                let url_desc = await internal_perform_control_transfer(sid,
                    0xC0, try_webusb.bVendorCode, try_webusb.iLandingPage, 2, actual_len);
                if (url_desc.data.length != actual_len || url_desc.data[1] != 0x03) return;

                let ret = "";
                if (url_desc.data[2] == 0)
                    ret += "http://";
                else if (url_desc.data[2] == 1)
                    ret += "https://";
                else {
                    console.log("WebUSB descriptor unknown scheme!", url_desc.data[2]);
                    return;
                }
                let url = new Uint8Array(url_desc.data.buffer, 3);
                ret += new TextDecoder().decode(url);
                webusb_landing_page = ret;
            } catch (e) {
                console.log("WebUSB descriptor fetch failed!", e);
            }
        }
        if (m.bcdUSB >= 0x0201) {
            await try_get_bos_desc();
            if (try_webusb !== undefined) {
                await try_get_webusb_desc();
            }
        }

        if (webusb_landing_page !== undefined) {
            console.log("TODO: Do something WebUSB landing page", webusb_landing_page);
        }

        usb_devices.set(sid, {
            bcdUSB: m.bcdUSB,
            bDeviceClass: m.bDeviceClass,
            bDeviceSubClass: m.bDeviceSubClass,
            bDeviceProtocol: m.bDeviceProtocol,
            idVendor: m.idVendor,
            idProduct: m.idProduct,
            bcdDevice: m.bcdDevice,
            manufacturer: m.manufacturer,
            product: m.product,
            serial: m.serial,

            current_config: m.current_config,
            configs,
            webusb_landing_page,
        });
    } else if (m.type === "UnplugDevice") {
        let sid = m.sid;
        let device = usb_devices.get(sid);
        if (device === undefined) {
            console.warn("Unplugging unknown device??", sid);
            return;
        }
        // TODO: Probably abort all outstanding transactions?
        usb_devices.delete(sid);
    } else if (m.type === "RequestComplete") {
        let data = undefined;
        if (m.data !== null && m.data !== undefined) {
            data = Uint8Array.fromBase64(m.data, { alphabet: "base64url" });
        }

        let txn_id = m.txn_id;
        let txn = usb_txns.get(txn_id);
        if (txn === undefined) {
            console.warn("Completing unknown transaction?", txn_id);
            return;
        }
        usb_txns.delete(txn_id);

        let [page_id, page_txn_id] = txn_id.split('-');
        if (+page_id == 0) {
            // Request directed for internal use
            let [resolve, _reject] = txn;
            resolve({
                data,
                babble: m.babble,
            });
        } else {
            // Requests directed at pages
            let page_port = page_ports.get(+page_id);
            console.log(page_port);
            page_port.postMessage({
                type: "RequestComplete",
                txn: +page_txn_id,
                data,
                babble: m.babble,
            });
        }
    } else if (m.type === "RequestError") {
        let txn_id = m.txn_id;
        let txn = usb_txns.get(txn_id);
        if (txn === undefined) {
            console.warn("Completing unknown transaction?", txn_id);
            return;
        }
        usb_txns.delete(txn_id);

        let [page_id, page_txn_id] = txn_id.split('-');
        if (+page_id == 0) {
            // Request directed for internal use
            let [_resolve, reject] = txn;
            reject(m.error);
        } else {
            // Requests directed at pages
            let page_port = page_ports.get(+page_id);
            console.log(page_port);
            page_port.postMessage({
                type: "RequestError",
                txn: +page_txn_id,
                error: m.error,
            });
        }
    } else {
        console.log("Unexpected reply from native stub!", m);
    }
})
