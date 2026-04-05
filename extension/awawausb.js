// This is the extension's background script.
//
// It is _privileged_ and _persistent_, and it is responsible for
// maintaining all global state related to USB.
// It launches _once_ and maintains _one_ connection to the native stub.

// Connection to native stub
let nativeport = browser.runtime.connectNative("awawausb_native_stub");
nativeport.onDisconnect.addListener((p) => {
    console.warn("Native process disconnected!", p.error);
})

// All USB devices we possibly know about, indexed by session ID
let usb_devices = new Map();

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
        txn_id: txn_id,
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

// List of page ports, with numeric IDs
let page_ports = new Map();
let next_port_id = 1;
browser.runtime.onConnect.addListener((p) => {
    let this_port_id = next_port_id++;
    console.log("new page port!", this_port_id, p.sender);
    page_ports.set(this_port_id, p);

    p.onDisconnect.addListener((_p) => {
        console.log("page port disconnected!", this_port_id);
        page_ports.delete(this_port_id);
    });

    p.onMessage.addListener((m) => {
        console.log("test from bkg", m);
        nativeport.postMessage({
            "type": "EchoTest",
            "msg": m.toString(),
        });
    });
});

// Handle replies (and notifications) from native process
nativeport.onMessage.addListener(async (m) => {
    if (m.type == "NewDevice") {
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

        // TODO: BOS, WebUSB descriptors

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
            // TODO: Put other data here too
        });
        console.log(usb_devices);
    } else if (m.type == "UnplugDevice") {
        let sid = m.sid;
        let device = usb_devices.get(sid);
        if (device === undefined) {
            console.warn("Unplugging unknown device??", sid);
            return;
        }
        // TODO: Probably abort all outstanding transactions?
        usb_devices.delete(sid);
        console.log(usb_devices);
    } else if (m.type == "RequestComplete") {
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

        let [page_id, _txn_id] = txn_id.split('-');
        if (+page_id == 0) {
            // Request directed for internal use
            let [resolve, _reject] = txn;
            resolve({
                data: data,
                babble: m.babble,
            });
        } else {
            // TODO: Requests directed at pages
        }
    } else if (m.type == "RequestError") {
        let txn_id = m.txn_id;
        let txn = usb_txns.get(txn_id);
        if (txn === undefined) {
            console.warn("Completing unknown transaction?", txn_id);
            return;
        }
        usb_txns.delete(txn_id);

        let [page_id, _txn_id] = txn_id.split('-');
        if (+page_id == 0) {
            // Request directed for internal use
            let [_resolve, reject] = txn;
            reject(m.error);
        } else {
            // TODO: Requests directed at pages
        }
    } else {
        console.log("Unexpected reply from native stub!", m);
    }
})
