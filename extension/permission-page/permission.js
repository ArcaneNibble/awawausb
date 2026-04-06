let params = new URLSearchParams(window.location.search);
let possible_devices = params.getAll("sid");

setTimeout(async function() {
    // Get all of the information we actually need for showing the user
    let device_info = await browser.runtime.sendMessage({
        type: "get_devices",
        devices: possible_devices,
    });

    // In the rare event that a device has been surprise-unplugged here,
    // filter out any devices which are missing.
    device_info = device_info.filter((x) => x !== undefined);
    if (device_info.length === 0) {
        // Causes a flash of jank, but oh well
        window.close();
    }
    console.log(device_info);
});

// browser.runtime.sendMessage({
//     type: "finished",
//     req: +params.get("req"),
//     result: 12345,
// });
