use scry_provider_protocol::v1::{ChatResponse, ResponseEvent, chat_response, response_event};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Dispatcher {
    event_id: u64,
    tx: mpsc::Sender<ResponseEvent>,
}

impl Dispatcher {
    pub fn new(event_id: u64, tx: mpsc::Sender<ResponseEvent>) -> Self {
        Self { event_id, tx }
    }

    pub async fn send(&self, payload: response_event::Payload) -> bool {
        self.tx
            .send(ResponseEvent {
                event_id: self.event_id,
                payload: Some(payload),
            })
            .await
            .is_ok()
    }

    pub async fn send_chat_event(&self, event: chat_response::Payload) -> bool {
        self.send(response_event::Payload::ChatResponse(ChatResponse {
            payload: Some(event),
        }))
        .await
    }
}
