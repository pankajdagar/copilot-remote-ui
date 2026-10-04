use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEvent {
    Output { bytes: Vec<u8> },
    Closed { message: String },
}
