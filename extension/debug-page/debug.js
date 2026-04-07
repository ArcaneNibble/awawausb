let list_devices_ul = document.getElementById("list_of_devices");
let list_pages_ol = document.getElementById("list_of_pages");
let list_txns_ol = document.getElementById("list_of_txns");

function make_row(...items) {
    let tr = document.createElement('tr');
    for (let item of items) {
        let td = document.createElement('td');
        if (item instanceof HTMLElement)
            td.appendChild(item);
        else
            td.innerText = item
        tr.appendChild(td);
    }
    return tr;
}

function to_hex(x, width) {
    let ret = x.toString(16);
    while (ret.length < width)
        ret = "0" + ret;
    return ret;
}

function do_iface_alt_setting(iface, elem) {
    elem.appendChild(make_row("Interface name", iface.intf_name));
    elem.appendChild(make_row("bInterfaceClass", `0x${to_hex(iface.bInterfaceClass, 2)}`));
    elem.appendChild(make_row("bInterfaceSubClass", `0x${to_hex(iface.bInterfaceSubClass, 2)}`));
    elem.appendChild(make_row("bInterfaceProtocol", `0x${to_hex(iface.bInterfaceProtocol, 2)}`));

    let eps_ul = document.createElement('ul');
    elem.appendChild(make_row("Endpoints", eps_ul));
    for (let ep of iface.endpoints) {
        let ep_li = document.createElement('li');
        eps_ul.appendChild(ep_li);
        let ep_table = document.createElement('table');
        ep_li.appendChild(ep_table);
        let ep_tbody = document.createElement('tbody');
        ep_table.appendChild(ep_tbody);

        ep_tbody.appendChild(make_row("bEndpointAddress", `0x${to_hex(ep.bEndpointAddress, 2)}`));
        ep_tbody.appendChild(make_row("bmAttributes", `0x${to_hex(ep.bmAttributes, 2)}`));
        ep_tbody.appendChild(make_row("wMaxPacketSize", `0x${to_hex(ep.wMaxPacketSize, 4)}`));
    }
}

document.getElementById("list_devices").addEventListener('click', async () => {
    let devices = await browser.runtime.sendMessage("list_devices");

    list_devices_ul.replaceChildren();

    for (let [sid, dev_info] of devices) {
        let table = document.createElement('table');
        let tbody = document.createElement('tbody');
        table.appendChild(tbody);

        let details = document.createElement('details');
        let summary = document.createElement('summary');
        summary.innerText = `Session ID: ${sid}`;
        details.appendChild(summary);
        details.appendChild(table);

        list_devices_ul.appendChild(details);

        if (dev_info.webusb_landing_page !== undefined) {
            let link = document.createElement('a');
            link.href = dev_info.webusb_landing_page;
            link.innerText = dev_info.webusb_landing_page;
            tbody.appendChild(make_row("WebUSB landing page", link));
        }
        tbody.appendChild(make_row("Open count", `${dev_info.opened}`));
        let page_device_refs_ul = document.createElement('ul');
        for (let [page_id, dev_handle] of dev_info.page_devices) {
            let li = document.createElement('li');
            li.innerText = `Page ${page_id} handle ${dev_handle}`;
            page_device_refs_ul.appendChild(li);
        }
        tbody.appendChild(make_row("Page references", page_device_refs_ul));

        tbody.appendChild(make_row("Vendor ID", `0x${to_hex(dev_info.idVendor, 4)}`));
        tbody.appendChild(make_row("Product ID", `0x${to_hex(dev_info.idProduct, 4)}`));
        tbody.appendChild(make_row("Manufacturer name", dev_info.manufacturer));
        tbody.appendChild(make_row("Product name", dev_info.product));
        tbody.appendChild(make_row("Serial number", dev_info.serial));
        tbody.appendChild(make_row("bcdUSB", `0x${to_hex(dev_info.bcdUSB, 4)}`));
        tbody.appendChild(make_row("bcdDevice", `0x${to_hex(dev_info.bcdDevice, 4)}`));
        tbody.appendChild(make_row("bDeviceClass", `0x${to_hex(dev_info.bDeviceClass, 2)}`));
        tbody.appendChild(make_row("bDeviceSubClass", `0x${to_hex(dev_info.bDeviceSubClass, 2)}`));
        tbody.appendChild(make_row("bDeviceProtocol", `0x${to_hex(dev_info.bDeviceProtocol, 2)}`));
        tbody.appendChild(make_row("Current configuration", `0x${to_hex(dev_info.current_config, 2)}`));


        let all_configs_ul = document.createElement('ul');
        tbody.appendChild(make_row("All configurations", all_configs_ul));
        for (let conf of dev_info.configs) {
            let configs_li = document.createElement('li');
            all_configs_ul.appendChild(configs_li);

            let configs_table = document.createElement('table');
            let configs_tbody = document.createElement('tbody');
            configs_table.appendChild(configs_tbody);

            let configs_details = document.createElement('details');
            let configs_summary = document.createElement('summary');
            configs_summary.innerText = `bConfigurationValue: 0x${to_hex(conf.bConfigurationValue, 2)}`;
            configs_details.appendChild(configs_summary);
            configs_details.appendChild(configs_table);
            configs_li.appendChild(configs_details);

            configs_tbody.appendChild(make_row("Config name", conf.config_name));

            let all_ifaces_ul = document.createElement('ul');
            configs_tbody.appendChild(make_row("Interfaces", all_ifaces_ul));
            for (let iface of conf.interfaces) {
                let iface_li = document.createElement('li');
                all_ifaces_ul.appendChild(iface_li);

                let iface_table = document.createElement('table');
                let iface_tbody = document.createElement('tbody');
                iface_table.appendChild(iface_tbody);

                let iface_details = document.createElement('details');
                let iface_summary = document.createElement('summary');
                iface_summary.innerText = `bInterfaceNumber: 0x${to_hex(iface.bInterfaceNumber, 2)}`;
                iface_details.appendChild(iface_summary);
                iface_details.appendChild(iface_table);
                iface_li.appendChild(iface_details);

                if (conf.bConfigurationValue === dev_info.current_config) {
                    iface_tbody.appendChild(make_row("Current alt setting", `0x${to_hex(iface.current_alt_setting, 2)}`));
                    iface_tbody.appendChild(make_row("Claimed by page", `${dev_info.interfaces_claimed[iface.bInterfaceNumber]}`));
                }

                if (iface.alts.length == 1) {
                    do_iface_alt_setting(iface.alts[0], iface_tbody);
                } else {
                    let all_iface_alts_ul = document.createElement('ul');
                    iface_tbody.appendChild(make_row("All alt settings", all_iface_alts_ul));
                    for (let iface_alt of iface.alts) {

                        let iface_alt_li = document.createElement('li');
                        all_iface_alts_ul.appendChild(iface_alt_li);

                        let iface_alt_table = document.createElement('table');
                        let iface_alt_tbody = document.createElement('tbody');
                        iface_alt_table.appendChild(iface_alt_tbody);

                        let iface_alt_details = document.createElement('details');
                        let iface_alt_summary = document.createElement('summary');
                        iface_alt_summary.innerText = `bAlternateSetting: 0x${to_hex(iface_alt.bAlternateSetting, 2)}`;
                        iface_alt_details.appendChild(iface_alt_summary);
                        iface_alt_details.appendChild(iface_alt_table);
                        iface_alt_li.appendChild(iface_alt_details);

                        do_iface_alt_setting(iface_alt, iface_alt_tbody);
                    }
                }
            }
        }
    }
});

document.getElementById("list_pages").addEventListener('click', async () => {
    let pages = await browser.runtime.sendMessage("list_pages");

    list_pages_ol.replaceChildren();

    let sorted_pages = pages.toSorted((a, b) => a[0] - b[0]);
    for (let page_ent of sorted_pages) {
        let li = document.createElement('li');
        li.value = page_ent.page_id;
        li.innerText = page_ent.url;
        list_pages_ol.appendChild(li);

        // Device handle table
        {
            let table = document.createElement('table');
            let thead = document.createElement('thead');
            thead.appendChild(make_row(
                "Page device handle",
                "Session ID",
                "Opened?",
                "Pending transactions",
            ));
            table.appendChild(thead);
            let tbody = document.createElement('tbody');
            for (let [handle_id, sid, opened, transactions] of page_ent.handles) {
                let transactions_ul = document.createElement('ul');
                for (let x of transactions) {
                    let li = document.createElement('li');
                    li.innerText = `${x.global_txn_id} (Interface ${x.intf})`;
                    transactions_ul.appendChild(li);
                };
                tbody.appendChild(make_row(handle_id, sid, opened, transactions_ul));
            }
            table.appendChild(tbody);
            li.appendChild(table);
        }

        // Allowed devices table
        {
            let table = document.createElement('table');
            let thead = document.createElement('thead');
            thead.appendChild(make_row(
                "VID",
                "PID",
                "SN",
                "Device handles",
            ));
            table.appendChild(thead);
            let tbody = document.createElement('tbody');
            for (let [ids, devs] of page_ent.allowed_devices) {
                let handles_ul = document.createElement('ul');
                for (let x of devs) {
                    let li = document.createElement('li');
                    li.innerText = x;
                    handles_ul.appendChild(li);
                }
                tbody.appendChild(make_row(
                    `0x${to_hex(ids.vid, 4)}`,
                    `0x${to_hex(ids.pid, 4)}`,
                    ids.sn,
                    handles_ul
                ));
            }
            table.appendChild(tbody);
            li.appendChild(table);
        }
    }
});

document.getElementById("list_txns").addEventListener('click', async () => {
    let txns = await browser.runtime.sendMessage("list_txns");

    list_of_txns.replaceChildren();

    for (let txn_id of txns) {
        let li = document.createElement('li');
        li.innerText = txn_id;
        list_of_txns.appendChild(li);
    }
});
