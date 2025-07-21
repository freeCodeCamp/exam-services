use bson::Bson;
use prisma_rust_schema;
use serde::{Deserialize, Serialize};

prisma_rust_schema::import_types!(
    schema_path = "https://raw.githubusercontent.com/ShaunSHamilton/freeCodeCamp/274738aa3184e79eda84da4218ac19a8183a1682/api/prisma/schema.prisma",
    derive = [Clone, Debug, Serialize, Deserialize],
    include = [
        "ExamEnvironmentExam",
        "ExamEnvironmentExamAttempt",
        "ExamEnvironmentQuestionSetAttempt",
        "ExamEnvironmentMultipleChoiceQuestionAttempt",
        "ExamEnvironmentQuestionSet",
        "ExamEnvironmentMultipleChoiceQuestion",
        "ExamEnvironmentAudio",
        "ExamEnvironmentQuestionType",
        "ExamEnvironmentConfig",
        "ExamEnvironmentQuestionSetConfig",
        "ExamEnvironmentTagConfig",
        "ExamEnvironmentAnswer",
        "ExamEnvironmentExamModeration",
        "ExamEnvironmentExamModerationStatus",
    ]
);

impl ToString for ExamEnvironmentExamModerationStatus {
    fn to_string(&self) -> String {
        match self {
            ExamEnvironmentExamModerationStatus::Approved => "approved",
            ExamEnvironmentExamModerationStatus::Denied => "denied",
            ExamEnvironmentExamModerationStatus::Pending => "pending",
        }
        .to_string()
    }
}

impl From<ExamEnvironmentExamModerationStatus> for Bson {
    fn from(value: ExamEnvironmentExamModerationStatus) -> Self {
        Bson::String(value.to_string())
    }
}
