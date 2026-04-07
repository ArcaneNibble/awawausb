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

const EXTENSION_ID = "awawausb@arcanenibble.com";
const WEBUSB_PLATFORM_CAPABILITY = [0x38, 0xB6, 0x08, 0x34, 0xA9, 0x09, 0xA0, 0x47, 0x8B, 0xFD, 0xA0, 0x76, 0x88, 0x15, 0xB6, 0x65];

// All USB devices we possibly know about, indexed by session ID
// XXX this structure is rather ad-hoc, because it contains a lot of Chapter-9 fields
let usb_devices = new Map();

// All outstanding transactions, indexed by transaction ID,
// which is a string of the form `pageID-txnID`
// (page ID of 0 is reserved for special global control transfers)
//
// The result of the map is either [resolve, reject] (internal control transfers),
// or else it's the below type (page transfers, global close)
let usb_txns = new Map();

class PageTransaction {
    alive = true;
    callback;
    constructor(cb) {
        this.callback = cb;
    }
}

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

// Permission dialog
class UserPermissionDialog {
    static #request_id = 0;
    // Map a request ID to a _resolve_ promise
    static #request_resolve = new Map();
    // Map a permission window ID to [a _resolve_ promise, permission request ID]
    // This is used to indicate user cancellation (by resolving with null)
    static #windows = new Map();

    static {
        browser.windows.onRemoved.addListener((window_id) => {
            if (UserPermissionDialog.#windows.has(window_id)) {
                let [resolve, permission_request_id] = UserPermissionDialog.#windows.get(window_id);
                UserPermissionDialog.#windows.delete(window_id);

                let request_was_outstanding = UserPermissionDialog.#request_resolve.delete(permission_request_id);
                if (request_was_outstanding) {
                    resolve(null);
                }
            }
        });

        browser.runtime.onMessage.addListener((m, sender, response) => {
            if (sender.id === EXTENSION_ID && sender.url.startsWith("moz-extension://")) {
                if (sender.url.split('?', 1)[0].endsWith("/permission-page/permission.html")) {
                    if (m.type === "finished") {
                        let resolve = UserPermissionDialog.#request_resolve.get(m.req);
                        UserPermissionDialog.#request_resolve.delete(m.req);
                        resolve(m.result);
                    } else if (m.type === "get_devices") {
                        let devices = m.devices.map(sid => usb_devices.get(sid));
                        response(devices);
                    } else {
                        console.warn("Permission page sent bad request!", m)
                    }
                }
            }
        });
    }

    static request_user_permissions(possible_sids) {
        let this_permission_request_id = UserPermissionDialog.#request_id++;
        let resolve;
        const promise = new Promise((res, _rej) => {
            resolve = res;
        });
        UserPermissionDialog.#request_resolve.set(this_permission_request_id, resolve);

        let args = new URLSearchParams();
        args.set("req", this_permission_request_id);
        for (let sid of possible_sids)
            args.append("sid", sid);

        return browser.windows.create({
            type: "panel",
            url: `/permission-page/permission.html?${args.toString()}`,
            width: 600,
            height: 400,
        }).then((window) => {
            UserPermissionDialog.#windows.set(window.id, [resolve, this_permission_request_id]);
            return promise;
        });
    }
}

// Page state

// What we need to know (here, globally) about USBDevice objects in pages
class PerPageUSBDevice {
    dev_handle;
    page;
    sid;
    global_usb_dev;
    constructor(dev_handle, page, sid, global_usb_dev) {
        this.dev_handle = dev_handle;
        this.page = page;
        this.sid = sid;
        this.global_usb_dev = global_usb_dev;
    }

    // The following state is tracked twice: here, and in the page.
    opened = false;
    claimed_interfaces = new Array();

    transactions = new Set();
    // Queues a transaction, both locally *and* globally.
    // Callback will be called when it finishes
    queue_transaction(global_txn_id, page_txn_id, intf, cb) {
        let txn_ref = {
            global_txn_id,
            page_txn_id,
            intf,
        };
        this.transactions.add(txn_ref);
        usb_txns.set(global_txn_id, new PageTransaction((res) => {
            this.transactions.delete(txn_ref);
            cb(res);
        }));
    }
    // -2   --> abort *everything* (closing, reset)
    // -1   --> abort all interfaces, but not device-targeted transfers (change configuration)
    // >=0  --> abort on this interface
    abort_transactions(intf) {
        for (let txn_ref of this.transactions) {
            if (intf === -2 || (intf === -1 && txn_ref.intf !== -1) || (intf === txn_ref.intf)) {
                console.log("Transaction is aborted", txn_ref);
                try {
                    this.page.port.postMessage({
                        txn_id: txn_ref.page_txn_id,
                        success: false,
                        error: "abort",
                    });
                } catch (e) {
                    // It's okay if this fails. This usually happens when cleaning up after a closed page.
                }

                this.transactions.delete(txn_ref);
                let global_txn = usb_txns.get(txn_ref.global_txn_id);
                global_txn.alive = false;
            }
        }
    }

    // Close this device, using the specified transaction ID to talk to the native stub.
    // We need to do this on page close as well as on explicit close
    close(global_txn_id) {
        this.abort_transactions(-2);

        // Try to release each interface
        for (let iface in this.claimed_interfaces) {
            if (this.claimed_interfaces[iface]) {
                console.log("Releasing interface on close...", this.sid, iface);
                // We have to use the global scope for these requests,
                // since we're already in the middle of a request.
                let this_txn_id = usb_global_txn++;
                let new_txn_id = `0-${this_txn_id}`;

                usb_txns.set(new_txn_id, new PageTransaction((res) => {
                    // No errors are reported if closing fails
                    if (res.type === "RequestError") {
                        console.warn("Interface release failed!", res);
                    } else {
                        console.log("Released interface on close", this.sid, iface);
                        this.global_usb_dev.interfaces_claimed[iface] = null;
                        this.claimed_interfaces[iface] = false;
                    }
                }));
                nativeport.postMessage({
                    type: "ReleaseInterface",
                    sid: this.sid,
                    txn_id: new_txn_id,
                    value: +iface,  // XXX JS is silly
                });
            }
        }

        this.opened = false;
        this.global_usb_dev.opened--;
        if (this.global_usb_dev.opened === 0) {
            // We actually have to send a request to the stub now
            // This is not queued on the per-page device, and it cannot be aborted
            usb_txns.set(global_txn_id, new PageTransaction((res) => {
                // No errors are reported if closing fails
                if (res.type === "RequestError") {
                    console.warn("Close operation failed!", res);
                } else {
                    console.log("Device closed", this.sid);
                }
            }));
            nativeport.postMessage({
                type: "CloseDevice",
                sid: this.sid,
                txn_id: global_txn_id,
            });
        }
    }

    get clean_up_usb_device_for_page() {
        let {
            webusb_landing_page: _1,
            opened: _2,
            page_devices: _3,
            interfaces_claimed: _4,
            ep_to_idx: _5,
            ...ret
        } = this.global_usb_dev;
        return ret;
    }
}

// Actual per-page state. Pages are referenced by a numeric ID,
// and this state contains the reply messaging port as well as
// the "authoritative" copy of the page's opened devices (USBDevice objects)
class PerPageState {
    page_id;
    port;
    constructor(page_id, port) {
        this.page_id = page_id;
        this.port = port;
    }

    // Permission storage
    // {vid, pid, sn} => {dev_handles...}
    #allowed_devices = new Map();
    find_allowed_device_slot(global_usb_dev, make_hole=true) {
        let found;
        for (let [ids, handles] of this.#allowed_devices) {
            if (ids.vid === global_usb_dev.idVendor
                && ids.pid === global_usb_dev.idProduct
                && ids.sn === global_usb_dev.serial)
            {
                found = [ids, handles];
                break;
            }
        }

        if (make_hole && found === undefined) {
            let ids = {
                vid: global_usb_dev.idVendor,
                pid: global_usb_dev.idProduct,
                sn: global_usb_dev.serial,
            };
            let handles = new Set();
            this.#allowed_devices.set(ids, handles);
            found = [ids, handles];
        }

        return found;
    }
    forget_device(dev_handle) {
        for (let handles of this.#allowed_devices.values()) {
            handles.delete(dev_handle);
        }
    }

    // Map from device handle (numeric ID) to authoritative state
    opened_devices = new Map();
    // "Reverse" map from session ID to device handle
    // This is used to make sure we don't open duplicate devices
    #sid_to_handle = new Map();
    #next_device_id = 0;
    open_device(sid) {
        let existing_handle = this.#sid_to_handle.get(sid);
        if (existing_handle !== undefined)
            return [existing_handle, this.opened_devices.get(existing_handle)];

        let global_usb_dev = usb_devices.get(sid);
        if (global_usb_dev === undefined) {
            return [undefined, undefined];
        }

        let this_device_handle = this.#next_device_id++;
        let page_usb_dev = new PerPageUSBDevice(this_device_handle, this, sid, global_usb_dev);
        this.opened_devices.set(this_device_handle, page_usb_dev);
        this.#sid_to_handle.set(sid, this_device_handle);
        global_usb_dev.page_devices.add(page_usb_dev);

        // Add this device to the permission storage
        let [_, allowed_devices_slot] = this.find_allowed_device_slot(global_usb_dev);
        allowed_devices_slot.add(this_device_handle);

        return [this_device_handle, page_usb_dev];
    }

    // Force-invalidate a page's device, probably because it was unplugged
    invalidate_device(page_usb_dev) {
        this.#sid_to_handle.delete(page_usb_dev.sid);
        this.opened_devices.delete(page_usb_dev.dev_handle);
        page_usb_dev.global_usb_dev.page_devices.delete(page_usb_dev);

        // Remove this device from the permission storage,
        // and also remove the key entirely if there's no SN and no device
        let allowed_slot = this.find_allowed_device_slot(page_usb_dev.global_usb_dev, false);
        if (allowed_slot !== undefined) {
            let [allowed_ids, allowed_handles] = allowed_slot;
            // We *only* want to wipe out the allowed permissions if
            // the client *hasn't* already called forget()
            // This is the only way to hang on to devices with no SN,
            // and this is what the spec says:
            // > Search for an element allowedDevice in storage.allowedDevices
            // > where device is in allowedDevice@[[devices]],
            // > if no such element exists, abort these steps.
            if (allowed_handles.has(page_usb_dev.dev_handle)) {
                allowed_handles.delete(page_usb_dev.dev_handle);
                if (allowed_ids.sn === null && allowed_handles.size === 0) {
                    this.#allowed_devices.delete(allowed_ids);
                }

                // Apparently we don't get disconnect notifications on forgotten devices
                try {
                    this.port.postMessage({
                        event: "unplug",
                        dev_handle: page_usb_dev.dev_handle,
                    });
                } catch (e) {
                    // Ignore notification failures
                }
            }
        }

        // NOTE: We don't abort any transactions. We're not told to do that.
        // Also, the stub will (should?) return them to us as failed.
    }

    // Loop through *all* pages, see if it makes sense to inject the
    static inject_new_device(sid, global_usb_dev) {
        for (let page of PerPageState.#pages.values()) {
            let maybe_allowed = page.find_allowed_device_slot(global_usb_dev, false);
            if (maybe_allowed === undefined) continue;

            console.log("Injecting allowed device", sid, page.page_id);
            let this_device_handle = page.#next_device_id++;
            let page_usb_dev = new PerPageUSBDevice(this_device_handle, page, sid, global_usb_dev);
            page.opened_devices.set(this_device_handle, page_usb_dev);
            page.#sid_to_handle.set(sid, this_device_handle);
            global_usb_dev.page_devices.add(page_usb_dev);

            // Add this device to the permission storage
            maybe_allowed[1].add(this_device_handle);

            // Try to send event
            try {
                page.port.postMessage({
                    event: "plug",
                    dev_handle: this_device_handle,
                    dev_data: page_usb_dev.clean_up_usb_device_for_page,
                });
            } catch (e) {
                // Ignore notification failures
            }
        }
    }

    static #next_page_id = 1;
    static #pages = new Map();
    static new_page(port) {
        let this_page_id = PerPageState.#next_page_id++;
        let state = new PerPageState(this_page_id, port);
        console.log("New page opened!", this_page_id, port.sender);
        PerPageState.#pages.set(this_page_id, state);

        return [this_page_id, state];
    }
    static delete_page(page_id) {
        console.log("Page closed...", page_id);

        // Close all devices
        let page = PerPageState.#pages.get(page_id);
        for (let page_usb_dev of page.opened_devices.values()) {
            let global_usb_dev = usb_devices.get(page_usb_dev.sid);
            if (global_usb_dev === undefined) continue;

            if (page_usb_dev.opened) {
                let this_txn_id = usb_global_txn++;
                let txn_id = `0-${this_txn_id}`;
                page_usb_dev.close(txn_id);
            }

            // Since we are _closing_ closing, we not only have to close
            // but we also need to invalidate the references from global->page usb object
            global_usb_dev.page_devices.delete(page_usb_dev);
        }

        PerPageState.#pages.delete(page_id);
    }
    static debug_pages() {
        let ret = new Array();
        for (let [page_id, state] of PerPageState.#pages) {
            let handles = new Array();
            for (let [handle_id, usb_dev] of state.opened_devices) {
                handles.push([handle_id, usb_dev.sid, usb_dev.opened, usb_dev.transactions]);
            }
            ret.push({
                page_id,
                url: state.port.sender.url,
                allowed_devices: state.#allowed_devices,
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
    }
    if (filt.classCode !== null && dev.bDeviceClass !== filt.classCode) return false;
    if (filt.subclassCode !== null && dev.bDeviceSubClass !== filt.subclassCode) return false;
    if (filt.protocolCode !== null && dev.bDeviceProtocol !== filt.protocolCode) return false;
    return true;
}

class DebugPage {
    static {
        browser.runtime.onMessage.addListener((m, sender, response) => {
            if (sender.id === EXTENSION_ID && sender.url.startsWith("moz-extension://")) {
                if (sender.url.endsWith("/debug-page/debug.html")) {
                    if (m === "list_devices") {
                        let devices = new Array();
                        for (let [sid, usb_dev] of usb_devices) {
                            let {page_devices, ...rest} = usb_dev;
                            page_devices = Array.from(page_devices, (x) => [x.page.page_id, x.dev_handle]);
                            devices.push([sid, {page_devices, ...rest}]);
                        }
                        response(devices);
                    } else if (m === "list_pages") {
                        response(PerPageState.debug_pages());
                    } else if (m === "list_txns") {
                        let txns = new Array();
                        for (let txn_id of usb_txns.keys()) {
                            txns.push(txn_id);
                        }
                        response(txns);
                    } else {
                        console.warn("Debug page sent bad request!", m)
                    }
                }
            }
        });
    }
}

// Handle requests from pages
browser.runtime.onConnect.addListener((p) => {
    // Create and stash the per-page state
    let [this_page_id, this_page] = PerPageState.new_page(p);

    function get_usb_device(m, check_open=false, check_config=true) {
        let page_usb_dev = this_page.opened_devices.get(m.dev_handle);
        if (page_usb_dev === undefined) {
            p.postMessage({
                txn_id: m.txn_id,
                success: false,
                error: "not_found",
            });
            return;
        }

        if (check_open) {
            if (!page_usb_dev.opened) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "not_open",
                });
                return;
            }
            if (check_config) {
                if (page_usb_dev.global_usb_dev.current_config == 0) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: false,
                        error: "not_configured",
                    });
                    return;
                }
            }
        }

        return page_usb_dev;
    }

    function map_native_error(txn_id, m) {
        if (m.type === "RequestError") {
            let mapped_error;
            if (m.error === "DeviceNotFound")
                mapped_error = "not_found";
            else if (m.error === "Stall")
                mapped_error = "stall";
            else if (m.error === "InvalidState")
                mapped_error = "not_open";
            else if (m.error === "InvalidNumber")
                mapped_error = "invalid_value";
            else if (m.error === "AlreadyClaimed")
                mapped_error = "already_claimed";

            p.postMessage({
                txn_id,
                success: false,
                error: mapped_error,
            });
            return true;
        }

        return false;
    }

    p.onDisconnect.addListener((_p) => {
        PerPageState.delete_page(this_page_id);
    });

    p.onMessage.addListener(async (m) => {
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

            let selected_sid = await UserPermissionDialog.request_user_permissions(possible_devices);

            // If there wasn't a selection made, or if it's invalid (unplugged?), bail out
            if (selected_sid === null) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                });
                return;
            }
            let [dev_handle, page_usb_dev] = this_page.open_device(selected_sid);
            if (dev_handle === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                });
                return;
            }

            // Otherwise, we should be good to go!
            p.postMessage({
                txn_id: m.txn_id,
                success: true,
                dev_handle,
                dev_data: page_usb_dev.clean_up_usb_device_for_page,
            });
        } else if (m.type === "open") {
            let page_usb_dev = get_usb_device(m);
            if (page_usb_dev === undefined) return;

            // Already open _locally_?
            if (page_usb_dev.opened) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: true,
                });
                return;
            }

            // Already open globally, but *not* locally?
            if (page_usb_dev.global_usb_dev.opened > 0) {
                page_usb_dev.global_usb_dev.opened++;
                page_usb_dev.opened = true;
                p.postMessage({
                    txn_id: m.txn_id,
                    success: true,
                });
                return;
            }

            // We actually have to send a request to the stub now
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, -1, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    // The open was (finally) successful
                    // NOTE: We *can* race and send redundant opens to the stub
                    page_usb_dev.global_usb_dev.opened++;
                    page_usb_dev.opened = true;
                    console.log("Device successfully opened", page_usb_dev.sid);
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage({
                type: "OpenDevice",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
            });
        } else if (m.type === "close") {
            let page_usb_dev = get_usb_device(m);
            if (page_usb_dev === undefined) return;

            // Already closed _locally_?
            if (!page_usb_dev.opened) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: true,
                });
                return;
            }

            // Do the actual close operation, including aborting transactions
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.close(global_txn_id);

            // No errors are reported if closing fails
            p.postMessage({
                txn_id: m.txn_id,
                success: true,
            });
        } else if (m.type === "forget") {
            this_page.forget_device(m.dev_handle);
            p.postMessage({
                txn_id: m.txn_id,
                success: true,
            });
        } else if (m.type === "reset") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;
            page_usb_dev.abort_transactions(-2);

            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, -1, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage({
                type: "ResetDevice",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
            });
        } else if (m.type === "set_config") {
            // NOTE: Don't check open state here
            // The spec mandates a particular order
            let page_usb_dev = get_usb_device(m);
            if (page_usb_dev === undefined) return;

            // Validate the desired configuration
            let conf = m.configurationValue & 0xff;
            let found_conf;
            for (let conf_desc of page_usb_dev.global_usb_dev.configs) {
                if (conf_desc.bConfigurationValue === conf) {
                    found_conf = conf_desc;
                    break;
                }
            }
            if (found_conf === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }

            // *Now* we can check if it's open
            if (!page_usb_dev.opened) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "not_open",
                });
                return;
            }

            page_usb_dev.abort_transactions(-1);

            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, -1, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    console.log("Device configuration changed", page_usb_dev.sid, conf);
                    page_usb_dev.global_usb_dev.current_config = conf;
                    let ifaces_claimed = new Array();
                    for (let iface of found_conf.interfaces) {
                        // XXX: What happens if/when the OS does something _weird_ with alt settings?
                        iface.current_alt_setting = 0;
                        ifaces_claimed[iface.bInterfaceNumber] = null;
                    }
                    page_usb_dev.global_usb_dev.interfaces_claimed = ifaces_claimed;
                    page_usb_dev.claimed_interfaces = new Array();
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage({
                type: "SetConfiguration",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                value: conf,
            });
        } else if (m.type === "claim_interface") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let iface = m.interfaceNumber & 0xff;

            // Validate the desired interface by checking the global device
            // (which uses null vs undefined to distinguish)
            let iface_already_claimed = page_usb_dev.global_usb_dev.interfaces_claimed[iface];
            if (iface_already_claimed === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }

            // Already claimed by us?
            if (page_usb_dev.claimed_interfaces[iface]) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: true,
                });
                return;
            }

            // Already claimed by another page?
            if (iface_already_claimed !== null) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "already_claimed",
                });
                return;
            }

            // Ok, we can finally try to claim the interface
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, iface, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    console.log("Interface successfully claimed", page_usb_dev.sid, iface);
                    page_usb_dev.global_usb_dev.interfaces_claimed[iface] = this_page_id;
                    page_usb_dev.claimed_interfaces[iface] = true;
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage({
                type: "ClaimInterface",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                value: iface,
            });
        } else if (m.type === "release_interface") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let iface = m.interfaceNumber & 0xff;

            // Validate the desired interface by checking the global device
            // (which uses null vs undefined to distinguish)
            let iface_already_claimed = page_usb_dev.global_usb_dev.interfaces_claimed[iface];
            if (iface_already_claimed === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }

            // Not claimed by us?
            if (!page_usb_dev.claimed_interfaces[iface]) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: true,
                });
                return;
            }

            // Ok, we can finally try to release the interface
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, iface, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    console.log("Interface successfully released", page_usb_dev.sid, iface);
                    page_usb_dev.global_usb_dev.interfaces_claimed[iface] = null;
                    page_usb_dev.claimed_interfaces[iface] = false;
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage({
                type: "ReleaseInterface",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                value: iface,
            });
        } else if (m.type === "set_alt_interface") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let iface = m.interfaceNumber & 0xff;
            let alt = m.alternateSetting & 0xff;

            // Validate the desired interface by checking the global device
            // (which uses null vs undefined to distinguish)
            let iface_already_claimed = page_usb_dev.global_usb_dev.interfaces_claimed[iface];
            if (iface_already_claimed === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }

            // Not claimed by us?
            if (!page_usb_dev.claimed_interfaces[iface]) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "not_open",
                });
                return;
            }

            // Make sure this alt setting exists
            let found_iface_desc;
            let found_alt = false;
            for (let conf_desc of page_usb_dev.global_usb_dev.configs) {
                if (conf_desc.bConfigurationValue === page_usb_dev.global_usb_dev.current_config) {
                    for (let iface_desc of conf_desc.interfaces) {
                        if (iface_desc.bInterfaceNumber === iface) {
                            for (let alt_desc of iface_desc.alts) {
                                if (alt_desc.bAlternateSetting === alt) {
                                    found_iface_desc = iface_desc;
                                    found_alt = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if (!found_alt) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }

            // Abort all transfers
            page_usb_dev.abort_transactions(iface);

            // Ok, we can finally try to change the alt setting
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, iface, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    console.log("Interface alt setting changed", page_usb_dev.sid, iface, alt);
                    found_iface_desc.current_alt_setting = alt;
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage({
                type: "SetAltInterface",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                iface,
                alt,
            });
        } else if (m.type === "ctrl_xfer") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let req = 0;
            let txn_iface = -1;

            if (m.length !== undefined)
                req |= 0x80;    // device-to-host

            if (m.setup.requestType === "standard") {}
            else if (m.setup.requestType === "class") {
                req |= 1 << 5;
            } else if (m.setup.requestType === "vendor") {
                req |= 2 << 5;
            } else {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                });
                return;
            }

            if (m.setup.recipient === "device") {}
            else if (m.setup.recipient === "interface") {
                let iface = m.setup.index & 0xff;
                if (!page_usb_dev.claimed_interfaces[iface]) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: false,
                        error: "not_open",
                    });
                    return;
                }
                req |= 1;
                txn_iface = iface;
            } else if (m.setup.recipient === "endpoint") {
                let ep = m.setup.index & 0xffff;

                // Check interface
                let iface_ep = page_usb_dev.global_usb_dev.ep_to_idx.get(ep);
                if (iface_ep === undefined) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: false,
                        error: "invalid_value",
                    });
                    return;
                }
                let iface = iface_ep.iface;
                if (!page_usb_dev.claimed_interfaces[iface]) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: false,
                        error: "not_open",
                    });
                    return;
                }

                req |= 2;
                txn_iface = iface;
            } else if (m.setup.recipient === "other") {
                req |= 3;
            }

            // Prepare the request
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            let req_obj = {
                type: "ControlTransfer",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                request_type: req,
                request: m.setup.request & 0xff,
                value: m.setup.value & 0xffff,
                index: m.setup.index & 0xffff,
            }
            if (req & 0x80) {
                // device to host
                req_obj.length = m.length & 0xffff;
            } else {
                // host to device
                let bytes = new Uint8Array(m.data);
                req_obj.data = bytes.toBase64({ alphabet: "base64url", omitPadding: true });
            }

            // Send the request
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, txn_iface, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                        babble: res.babble,
                        data: res.data,
                        bytes_written: res.bytes_written,
                    });
                }
            });
            nativeport.postMessage(req_obj);
        } else if (m.type === "data_xfer") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let ep = m.endpointNumber & 0xff;

            // Check interface
            let iface_ep = page_usb_dev.global_usb_dev.ep_to_idx.get(ep);
            if (iface_ep === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }
            let {iface, ep_obj} = iface_ep;
            if (!page_usb_dev.claimed_interfaces[iface]) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "not_open",
                });
                return;
            }

            let ep_type = ep_obj.bmAttributes & 3;
            if (ep_type !== 2 && ep_type !== 3) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "bad_ep_type",
                });
                return;
            }

            // Prepare the request
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            let req_obj = {
                type: "DataTransfer",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                ep,
            }
            if (ep & 0x80) {
                // device to host
                req_obj.length = m.length & 0xffffffff;
            } else {
                // host to device
                let bytes = new Uint8Array(m.data);
                req_obj.data = bytes.toBase64({ alphabet: "base64url", omitPadding: true });
            }

            // Send the request
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, iface, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                        babble: res.babble,
                        data: res.data,
                        bytes_written: res.bytes_written,
                    });
                }
            });
            nativeport.postMessage(req_obj);
        } else if (m.type === "clear_halt") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let ep = m.endpointNumber & 0xff;

            // Check interface
            let iface_ep = page_usb_dev.global_usb_dev.ep_to_idx.get(ep);
            if (iface_ep === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }
            let iface = iface_ep.iface;
            if (!page_usb_dev.claimed_interfaces[iface]) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "not_open",
                });
                return;
            }

            // Prepare the request
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            let req_obj = {
                type: "ClearHalt",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                ep,
            }

            // Send the request
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, iface, (res) => {
                if (!map_native_error(m.txn_id, res)) {
                    p.postMessage({
                        txn_id: m.txn_id,
                        success: true,
                    });
                }
            });
            nativeport.postMessage(req_obj);
        } else if (m.type === "isoc_xfer") {
            let page_usb_dev = get_usb_device(m, true);
            if (page_usb_dev === undefined) return;

            let ep = m.endpointNumber & 0xff;
            let packetLengths = Array.from(m.packetLengths, (x) => x & 0xffffffff);

            // Check interface
            let iface_ep = page_usb_dev.global_usb_dev.ep_to_idx.get(ep);
            if (iface_ep === undefined) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "invalid_value",
                });
                return;
            }
            let {iface, ep_obj} = iface_ep;
            if (!page_usb_dev.claimed_interfaces[iface]) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "not_open",
                });
                return;
            }

            let ep_type = ep_obj.bmAttributes & 3;
            if (ep_type !== 1) {
                p.postMessage({
                    txn_id: m.txn_id,
                    success: false,
                    error: "bad_ep_type",
                });
                return;
            }

            // Prepare the request
            let global_txn_id = `${this_page_id}-${m.txn_id}`;
            let req_obj = {
                type: "IsocTransfer",
                sid: page_usb_dev.sid,
                txn_id: global_txn_id,
                ep,
                pkt_len: packetLengths,
            }
            if (!(ep & 0x80)) {
                // host to device
                let bytes = new Uint8Array(m.data);
                req_obj.data = bytes.toBase64({ alphabet: "base64url", omitPadding: true });
            }

            // Send the request
            page_usb_dev.queue_transaction(global_txn_id, m.txn_id, iface, (res) => {
                console.log("isoc cb", res);

                // if (!map_native_error(m.txn_id, res)) {
                //     p.postMessage({
                //         txn_id: m.txn_id,
                //         success: true,
                //         babble: res.babble,
                //         data: res.data,
                //         bytes_written: res.bytes_written,
                //     });
                // }
            });
            nativeport.postMessage(req_obj);
        } else {
            console.warn("Unknown request from a page", m, p.sender.url);
            p.postMessage({
                txn_id: m.txn_id,
                success: false,
            });
        }
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
        let ep_to_idx = new Map();
        let iface_claimed = new Array();
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

                if (cfg.bConfigurationValue === m.current_config)
                    iface_claimed[intf.bInterfaceNumber] = null;

                // Interface (alternate) name string
                let intf_name = null;
                if (intf.iInterface !== 0) {
                    intf_name = await get_string_desc(intf.iInterface);
                }

                // Endpoints
                let eps = new Array();
                for (let ep of intf.endpoints) {
                    ep_to_idx.set(ep.bEndpointAddress, {
                        iface: intf.bInterfaceNumber,
                        ep_obj: ep,
                    });
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
                else if (url_desc.data[2] == 0xff) {}
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

        let global_usb_dev = {
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

            // The following settings are for internal use only
            // and are hidden from content pages
            webusb_landing_page,
            opened: 0,
            interfaces_claimed: iface_claimed,
            ep_to_idx,
            page_devices: new Set(),
        };
        usb_devices.set(sid, global_usb_dev);

        // See if any pages are allowed to access this device.
        // If so, inject a new device into them
        PerPageState.inject_new_device(sid, global_usb_dev);
    } else if (m.type === "UnplugDevice") {
        let sid = m.sid;
        let device = usb_devices.get(sid);
        if (device === undefined) {
            console.warn("Unplugging unknown device??", sid);
            return;
        }

        for (let page_dev of device.page_devices) {
            page_dev.page.invalidate_device(page_dev);
        }

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

        if (txn instanceof PageTransaction) {
            if (txn.alive) {
                m.data = data;
                txn.callback(m);
            } else {
                console.log("Completing a dead transaction", txn_id);
            }
        } else {
            let [resolve, _reject] = txn;
            resolve({
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

        if (txn instanceof PageTransaction) {
            if (txn.alive) {
                txn.callback(m);
            } else {
                console.log("Completing a dead transaction", txn_id);
            }
        } else {
            let [_resolve, reject] = txn;
            reject(m.error);
        }
    } else {
        console.log("Unexpected reply from native stub!", m);
    }
})
