use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ResponseMessage {
    EchoResponse { msg: String },
    NewDevice { sid: String },
    UnplugDevice { sid: String },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum RequestMessage {
    EchoTest { msg: String },
}
