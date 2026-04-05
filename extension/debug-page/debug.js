let port = browser.runtime.connect();
console.log("debug page content port", port);

port.onMessage.addListener((m) => {
    if (m.type === "list_devices") {
        console.log(m.devices);
    }
});