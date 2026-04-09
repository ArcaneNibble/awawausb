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
    /// Attempted to do something while the device is not properly opened or claimed
    InvalidState,
    /// A number (interface, endpoint) wasn't valid
    InvalidNumber,
    /// An interface is already claimed
    AlreadyClaimed,
}

#[derive(Debug, Serialize)]
pub enum IsocPacketState {
    Ok,
    Babble,
    Error,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize)]
pub struct DeviceEndpoint {
    pub bEndpointAddress: u8,
    pub bmAttributes: u8,
    pub wMaxPacketSize: u16,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize)]
pub struct DeviceInterface {
    pub bInterfaceNumber: u8,
    pub bAlternateSetting: u8,
    pub bInterfaceClass: u8,
    pub bInterfaceSubClass: u8,
    pub bInterfaceProtocol: u8,
    pub iInterface: u8,

    pub current_alt_setting: u8,
    pub endpoints: Vec<DeviceEndpoint>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Serialize)]
pub struct DeviceConfiguration {
    pub bConfigurationValue: u8,
    pub iConfiguration: u8,

    pub interfaces: Vec<DeviceInterface>,
}

#[allow(non_snake_case)]
#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ResponseMessage {
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
        bytes_written: u64,
    },
    RequestComplete {
        txn_id: String,
        babble: bool,
        data: Option<String>,
        bytes_written: u64,
    },
    IsocRequestComplete {
        txn_id: String,
        data: Option<String>,
        pkt_status: Vec<IsocPacketState>,
        pkt_len: Vec<u32>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum RequestMessage {
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
    DataTransfer {
        sid: String,
        txn_id: String,
        ep: u8,
        data: Option<String>,
        length: Option<u32>,
    },
    ClearHalt {
        sid: String,
        txn_id: String,
        ep: u8,
    },
    IsocTransfer {
        sid: String,
        txn_id: String,
        ep: u8,
        data: Option<String>,
        pkt_len: Vec<u32>,
    },

    OpenDevice {
        sid: String,
        txn_id: String,
    },
    CloseDevice {
        sid: String,
        txn_id: String,
    },
    ResetDevice {
        sid: String,
        txn_id: String,
    },
    SetConfiguration {
        sid: String,
        txn_id: String,
        value: u8,
    },

    ClaimInterface {
        sid: String,
        txn_id: String,
        value: u8,
    },
    ReleaseInterface {
        sid: String,
        txn_id: String,
        value: u8,
    },
    SetAltInterface {
        sid: String,
        txn_id: String,
        iface: u8,
        alt: u8,
    },
}
impl RequestMessage {
    pub fn txn_id(&self) -> &str {
        match self {
            RequestMessage::ControlTransfer { txn_id, .. } => txn_id,
            RequestMessage::DataTransfer { txn_id, .. } => txn_id,
            RequestMessage::ClearHalt { txn_id, .. } => txn_id,
            RequestMessage::IsocTransfer { txn_id, .. } => txn_id,
            RequestMessage::OpenDevice { txn_id, .. } => txn_id,
            RequestMessage::CloseDevice { txn_id, .. } => txn_id,
            RequestMessage::ResetDevice { txn_id, .. } => txn_id,
            RequestMessage::SetConfiguration { txn_id, .. } => txn_id,
            RequestMessage::ClaimInterface { txn_id, .. } => txn_id,
            RequestMessage::ReleaseInterface { txn_id, .. } => txn_id,
            RequestMessage::SetAltInterface { txn_id, .. } => txn_id,
        }
    }
}
