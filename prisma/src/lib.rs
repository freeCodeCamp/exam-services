use bson::Bson;
use prisma_rust_schema;
use serde::{Deserialize, Serialize};

prisma_rust_schema::import_types!(
    schema_path =
        "https://raw.githubusercontent.com/freeCodeCamp/freeCodeCamp/main/api/prisma/schema.prisma",
    derive = [Clone, Debug, Serialize, Deserialize, PartialEq],
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

impl Default for ExamEnvironmentExam {
    fn default() -> Self {
        Self {
            id: Default::default(),
            question_sets: Default::default(),
            config: Default::default(),
            prerequisites: Default::default(),
            deprecated: Default::default(),
            version: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentExamAttempt {
    fn default() -> Self {
        Self {
            id: Default::default(),
            user_id: Default::default(),
            exam_id: Default::default(),
            generated_exam_id: Default::default(),
            question_sets: Default::default(),
            start_time_in_m_s: Default::default(),
            version: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentQuestionSetAttempt {
    fn default() -> Self {
        Self {
            id: Default::default(),
            questions: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentMultipleChoiceQuestionAttempt {
    fn default() -> Self {
        Self {
            id: Default::default(),
            answers: Default::default(),
            submission_time_in_m_s: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentQuestionSet {
    fn default() -> Self {
        Self {
            id: Default::default(),
            _type: Default::default(),
            context: Default::default(),
            questions: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentMultipleChoiceQuestion {
    fn default() -> Self {
        Self {
            id: Default::default(),
            text: Default::default(),
            tags: Default::default(),
            audio: Default::default(),
            answers: Default::default(),
            deprecated: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentAnswer {
    fn default() -> Self {
        Self {
            id: Default::default(),
            is_correct: Default::default(),
            text: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentAudio {
    fn default() -> Self {
        Self {
            captions: Default::default(),
            url: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentConfig {
    fn default() -> Self {
        Self {
            name: Default::default(),
            note: Default::default(),
            tags: Default::default(),
            total_time_in_m_s: Default::default(),
            question_sets: Default::default(),
            retake_time_in_m_s: Default::default(),
            passing_percent: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentQuestionSetConfig {
    fn default() -> Self {
        Self {
            _type: Default::default(),
            number_of_set: Default::default(),
            number_of_questions: Default::default(),
            number_of_correct_answers: Default::default(),
            number_of_incorrect_answers: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentTagConfig {
    fn default() -> Self {
        Self {
            group: Default::default(),
            number_of_questions: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentExamModeration {
    fn default() -> Self {
        Self {
            id: Default::default(),
            status: Default::default(),
            exam_attempt_id: Default::default(),
            feedback: Default::default(),
            moderation_date: Default::default(),
            moderator_id: Default::default(),
            submission_date: bson::DateTime::now(),
            version: Default::default(),
        }
    }
}

impl Default for ExamEnvironmentExamModerationStatus {
    fn default() -> Self {
        ExamEnvironmentExamModerationStatus::Pending
    }
}

impl Default for ExamEnvironmentQuestionType {
    fn default() -> Self {
        Self::MultipleChoice
    }
}

impl ToString for ExamEnvironmentExamModerationStatus {
    fn to_string(&self) -> String {
        match self {
            ExamEnvironmentExamModerationStatus::Approved => "Approved",
            ExamEnvironmentExamModerationStatus::Denied => "Denied",
            ExamEnvironmentExamModerationStatus::Pending => "Pending",
        }
        .to_string()
    }
}

impl From<ExamEnvironmentExamModerationStatus> for Bson {
    fn from(value: ExamEnvironmentExamModerationStatus) -> Self {
        Bson::String(value.to_string())
    }
}
