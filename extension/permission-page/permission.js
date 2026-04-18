let params = new URLSearchParams(window.location.search);
let possible_devices = params.getAll("sid");

let dev_list = document.getElementById("dev_list");
let dev_info = document.getElementById("dev_info");
let submitbtn = document.getElementById("submitbtn");

let chosen_sid;

function to_hex(x, width) {
    let ret = x.toString(16);
    while (ret.length < width)
        ret = "0" + ret;
    return ret;
}

function prepare_device_label(dev) {
    let ret = "";

    if (dev.manufacturer !== null)
        ret += dev.manufacturer;
    else
        ret += `VID 0x${to_hex(dev.idVendor, 4)}`

    ret += " ";
    if (dev.product !== null)
        ret += dev.product;
    else
        ret += `PID 0x${to_hex(dev.idProduct, 4)}`

    ret += " (";
    if (dev.serial !== null)
        ret += `SN: ${dev.serial}, `;

    ret += `VID 0x${to_hex(dev.idVendor, 4)}, `
    ret += `PID 0x${to_hex(dev.idProduct, 4)}`

    ret += ")";
    return ret;
}

setTimeout(async function() {
    // Get all of the information we actually need for showing the user
    let device_info = await browser.runtime.sendMessage({
        type: "get_devices",
        devices: possible_devices,
    });

    // In the rare event that a device has been surprise-unplugged here,
    // filter out any devices which are missing.
    device_info = device_info
        .map((x, i) => ({
            sid: possible_devices[i],
            dev: x,
        }))
        .filter((x) => x.dev !== undefined);
    if (device_info.length === 0) {
        // Causes a flash of jank, but oh well
        window.close();
    }

    let device_labels = device_info.map((x) => prepare_device_label(x.dev));

    if (device_info.length === 1) {
        // If there is only one device, there is nothing to choose from
        chosen_sid = device_info[0].sid;
        dev_info.innerText = device_labels[0];
        dev_info.style = "";
    } else {
        // Otherwise populate the list
        for (let dev_i in device_info) {
            let option = document.createElement('option');
            option.value = device_info[dev_i].sid;
            option.innerText = device_labels[dev_i];
            dev_list.appendChild(option);
        }
        dev_list.size = device_info.length;
        dev_list.selectedIndex = -1;
        dev_list.style = "";
    }
});

submitbtn.addEventListener('click', () => {
    if (chosen_sid !== undefined) {
        browser.runtime.sendMessage({
            type: "finished",
            req: +params.get("req"),
            result: chosen_sid,
        });
        window.close();
    } else if (dev_list.selectedIndex !== -1 && dev_list.value !== "") {
        browser.runtime.sendMessage({
            type: "finished",
            req: +params.get("req"),
            result: dev_list.value,
        });
        window.close();
    }
})

// FIXME: Is there a better way to handle this?????
let all_elems = document.getElementsByTagName("*");
for (let elem of all_elems) {
    if (elem.dataset.i18n !== undefined) {
        elem.innerHTML = browser.i18n.getMessage(elem.dataset.i18n);
    }
}
