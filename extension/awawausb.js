let nativeport = browser.runtime.connectNative("awawausb_native_stub");

nativeport.onMessage.addListener((m) => {
    console.log("reply from native", m);
    if (m.type == "NewDevice") {
        nativeport.postMessage({
            "type": "ControlTransfer",
            "sid": m.sid,
            "txn_id": "deadbeef",
            "request_type": 0xc0,
            "request": 'E'.charCodeAt(0),
            "value": 0,
            "index": 0,
            "length": 4,
        });
    }
})

browser.runtime.onConnect.addListener((p) => {
    p.onMessage.addListener((m) => {
        console.log("test from bkg", m);
        nativeport.postMessage({
            "type": "EchoTest",
            "msg": m.toString(),
        });
    });
});

console.log("bkg script!", nativeport);
