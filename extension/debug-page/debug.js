let port = browser.runtime.connect();

let list_devices_ul = document.getElementById("list_of_devices");

function make_row(desc, val) {
    let tr = document.createElement('tr');
    let td1 = document.createElement('td');
    td1.innerText = desc;
    tr.appendChild(td1);
    let td2 = document.createElement('td');
    td2.innerText = val;
    tr.appendChild(td2);
    return tr;
}

function make_row_elem(desc, elem) {
    let tr = document.createElement('tr');
    let td1 = document.createElement('td');
    td1.innerText = desc;
    tr.appendChild(td1);
    let td2 = document.createElement('td');
    td2.appendChild(elem);
    tr.appendChild(td2);
    return tr;
}

function to_hex(x, width) {
    let ret = x.toString(16);
    while (ret.length < width)
        ret = "0" + ret;
    return ret;
}

function do_iface_alt_setting(iface, elem) {
    elem.appendChild(make_row("bAlternateSetting", `0x${to_hex(iface.bAlternateSetting, 2)}`));
    elem.appendChild(make_row("Interface name", iface.intf_name));
    elem.appendChild(make_row("bInterfaceClass", `0x${to_hex(iface.bInterfaceClass, 2)}`));
    elem.appendChild(make_row("bInterfaceSubClass", `0x${to_hex(iface.bInterfaceSubClass, 2)}`));
    elem.appendChild(make_row("bInterfaceProtocol", `0x${to_hex(iface.bInterfaceProtocol, 2)}`));

    let eps_ul = document.createElement('ul');
    elem.appendChild(make_row_elem("Endpoints", eps_ul));
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

port.onMessage.addListener((m) => {
    if (m.type === "list_devices") {
        list_devices_ul.replaceChildren();

        for (let [sid, dev_info] of m.devices) {
            let table = document.createElement('table');
            let tbody = document.createElement('tbody');
            table.appendChild(tbody);
            list_devices_ul.appendChild(table);

            tbody.appendChild(make_row("Session ID", sid));
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
            tbody.appendChild(make_row_elem("All configurations", all_configs_ul));
            for (let conf of dev_info.configs) {
                let configs_li = document.createElement('li');
                all_configs_ul.appendChild(configs_li);
                let configs_table = document.createElement('table');
                configs_li.appendChild(configs_table);
                let configs_tbody = document.createElement('tbody');
                configs_table.appendChild(configs_tbody);

                configs_tbody.appendChild(make_row("bConfigurationValue", `0x${to_hex(conf.bConfigurationValue, 2)}`));
                configs_tbody.appendChild(make_row("Config name", conf.config_name));

                let all_ifaces_ul = document.createElement('ul');
                configs_tbody.appendChild(make_row_elem("Interfaces", all_ifaces_ul));
                for (let iface of conf.interfaces) {
                    let iface_li = document.createElement('li');
                    all_ifaces_ul.appendChild(iface_li);
                    let iface_table = document.createElement('table');
                    iface_li.appendChild(iface_table);
                    let iface_tbody = document.createElement('tbody');
                    iface_table.appendChild(iface_tbody);

                    iface_tbody.appendChild(make_row("bInterfaceNumber", `0x${to_hex(iface.bInterfaceNumber, 2)}`));
                    iface_tbody.appendChild(make_row("Current alt setting", `0x${to_hex(iface.current_alt_setting, 2)}`));

                    if (iface.alts.length == 1) {
                        do_iface_alt_setting(iface.alts[0], iface_tbody);
                    } else {
                        let all_iface_alts_ul = document.createElement('ul');
                        iface_tbody.appendChild(make_row_elem("Alt settings", all_iface_alts_ul));
                        for (let iface_alt of iface.alts) {

                            let iface_alt_li = document.createElement('li');
                            all_iface_alts_ul.appendChild(iface_alt_li);
                            let iface_alt_table = document.createElement('table');
                            iface_alt_li.appendChild(iface_alt_table);
                            let iface_alt_tbody = document.createElement('tbody');
                            iface_alt_table.appendChild(iface_alt_tbody);
                            do_iface_alt_setting(iface_alt, iface_alt_tbody);
                        }
                    }
                }

            }

        }
    }
});

document.getElementById("list_devices").addEventListener('click', () => {
    port.postMessage("list_devices");
});
