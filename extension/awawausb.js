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

        // Query extra data
        // TODO: Query actually useful data
        let ret = await internal_perform_control_transfer(sid, 0xC0, 'e'.charCodeAt(0), 0, 0, 4, 0);
        console.log("ret", ret);
        ret = await internal_perform_control_transfer(sid, 0x40, 'E'.charCodeAt(0), 0, 0, new Uint8Array([11, 22, 33, 44]));
        console.log("ret", ret);
        ret = await internal_perform_control_transfer(sid, 0xC0, 'E'.charCodeAt(0), 0, 0, 4);
        console.log("ret", ret);

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
    } else {
        console.log("Unexpected reply from native stub!", m);
    }
})
