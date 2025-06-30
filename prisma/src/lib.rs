use prisma_rust_schema;
use serde::{Deserialize, Serialize};

prisma_rust_schema::import_types!(
    schema_path = "./schema.prisma",
    derive = [Clone, Debug, Serialize, Deserialize],
    include = [
        "EnvExam",
        "EnvExamAttempt",
        "EnvQuestionSetAttempt",
        "EnvMultipleChoiceQuestionAttempt",
        "EnvQuestionSet",
        "EnvMultipleChoiceQuestion",
        "EnvAudio",
        "EnvQuestionType",
        "EnvConfig",
        "EnvQuestionSetConfig",
        "EnvTagConfig",
        "EnvAnswer",
        "EnvExamModeration",
        "EnvExamModerationStatus",
    ]
);
