use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    CaptionsOpened,
    QuestionVisit,
    Focus,
    Blur,
    ExamExit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub kind: EventKind,
    pub meta: serde_json::Value,
    pub attempt_id: ObjectId,
}
