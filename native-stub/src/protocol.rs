use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[non_exhaustive]
pub enum Errors {
    DeviceNotFound,
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ResponseMessage {
    EchoResponse { msg: String },
    NewDevice { sid: String },
    UnplugDevice { sid: String },
    RequestError { txn_id: String, error: Errors },
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
