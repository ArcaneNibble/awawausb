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

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ResponseMessage {
    EchoResponse {
        msg: String,
    },
    NewDevice {
        sid: String,
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
