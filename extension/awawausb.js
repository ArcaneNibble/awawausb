let nativeport = browser.runtime.connectNative("awawausb_native_stub");

nativeport.onMessage.addListener((m) => {
    console.log("reply from native", m);
})

browser.runtime.onConnect.addListener((p) => {
    p.onMessage.addListener((m) => {
        console.log("test from bkg", m);
        nativeport.postMessage(m);
    });
});

console.log("bkg script!", nativeport);
