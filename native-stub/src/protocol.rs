//! JSON-based protocol for talking with the browser extension

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[non_exhaustive]
pub enum Errors {
    /// The device was not found
    ///
    /// This can happen either when the device ID is totally bogus,
    /// or when surprise removal happens.
    DeviceNotFound,
    /// A request was too big
    ///
    /// For example, a control transfer of more than 0xffff bytes,
    /// an isoc transfer of too many packets, etc.
    ///
    /// We do not attempt to hide platform-specific quirks.
    RequestTooBig,
    /// Device returned STALL condition
    Stall,
    /// Some kind of error occurred, but we are not required to be any more specific
    TransferError,
}

#[allow(non_snake_case)]
#[derive(Serialize)]
pub struct DeviceInterface {
    pub bInterfaceNumber: u8,
    pub bAlternateSetting: u8,
    pub bInterfaceClass: u8,
    pub bInterfaceSubClass: u8,
    pub bInterfaceProtocol: u8,
    pub iInterface: u8,
}

#[allow(non_snake_case)]
#[derive(Serialize)]
pub struct DeviceConfiguration {
    pub bConfigurationValue: u8,
    pub iConfiguration: u8,

    pub interfaces: Vec<DeviceInterface>,
}

#[allow(non_snake_case)]
#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ResponseMessage {
    EchoResponse {
        msg: String,
    },
    NewDevice {
        sid: String,

        bcdUSB: u16,
        bDeviceClass: u8,
        bDeviceSubClass: u8,
        bDeviceProtocol: u8,
        idVendor: u16,
        idProduct: u16,
        bcdDevice: u16,
        manufacturer: Option<String>,
        product: Option<String>,
        serial: Option<String>,

        current_config: u8,
        configs: Vec<DeviceConfiguration>,
    },
    UnplugDevice {
        sid: String,
    },
    RequestError {
        txn_id: String,
        error: Errors,
    },
    RequestComplete {
        txn_id: String,
        babble: bool,
        data: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum RequestMessage {
    EchoTest {
        msg: String,
    },
    ControlTransfer {
        sid: String,
        txn_id: String,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: Option<String>,
        length: Option<u16>,
        _timeout_internal: Option<u64>,
    },
}
