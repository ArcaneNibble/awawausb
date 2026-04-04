use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ResponseMessage {
    NewDevice { sid: String },
    UnplugDevice { sid: String },
}
