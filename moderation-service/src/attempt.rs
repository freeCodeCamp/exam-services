use mongodb::bson::oid::ObjectId;
use prisma::{
    ExamEnvironmentAnswer, ExamEnvironmentExam, ExamEnvironmentExamAttempt,
    ExamEnvironmentGeneratedExam,
};

/// Calculates the attempt score, and compares score >= pass_score
pub fn check_attempt_pass(
    exam: &ExamEnvironmentExam,
    generated_exam: &ExamEnvironmentGeneratedExam,
    attempt: &ExamEnvironmentExamAttempt,
) -> bool {
    let passing_percent = exam.config.passing_percent;
    if let Ok(score) = calculate_score(exam, generated_exam, attempt) {
        return score >= passing_percent;
    }
    false
}

pub fn calculate_score(
    exam: &ExamEnvironmentExam,
    generated_exam: &ExamEnvironmentGeneratedExam,
    attempt: &ExamEnvironmentExamAttempt,
) -> Result<f64, String> {
    let attempt_question_sets = &attempt.question_sets;
    let generated_question_sets = &generated_exam.question_sets;

    let total_questions: usize = generated_question_sets
        .iter()
        .map(|qs| qs.questions.len())
        .sum();
    let mut correct_questions: usize = 0;

    for attempt_question_set in attempt_question_sets {
        let exam_question_set = exam
            .question_sets
            .iter()
            .find(|qs| &qs.id == &attempt_question_set.id)
            .ok_or_else(|| {
                format!(
                    "Attempt question set {} must exist in exam {}",
                    attempt_question_set.id, exam.id
                )
            })?;

        let generated_question_set = generated_question_sets
            .iter()
            .find(|qs| &qs.id == &attempt_question_set.id)
            .ok_or_else(|| {
                format!(
                    "Generated question set {} must exist in generated exam {}",
                    attempt_question_set.id, generated_exam.id
                )
            })?;

        for attempt_question in &attempt_question_set.questions {
            let exam_question = exam_question_set
                .questions
                .iter()
                .find(|q| &q.id == &attempt_question.id)
                .ok_or_else(|| {
                    format!(
                        "Attempt question {} must exist in exam {}",
                        attempt_question.id, exam.id
                    )
                })?;

            let generated_question = generated_question_set
                .questions
                .iter()
                .find(|q| &q.id == &attempt_question.id)
                .ok_or_else(|| {
                    format!(
                        "Generated question {} must exist in generated exam {}",
                        attempt_question.id, generated_exam.id
                    )
                })?;

            if compare_answers(
                &exam_question.answers,
                &generated_question.answers,
                &attempt_question.answers,
            ) {
                correct_questions += 1;
            }
        }
    }

    Ok((correct_questions as f64 / total_questions as f64) * 100.0)
}

pub fn compare_answers(
    exam_answers: &[ExamEnvironmentAnswer],
    generated_answers: &[ObjectId],
    attempt_answers: &[ObjectId],
) -> bool {
    let correct_generated_answers: Vec<&ObjectId> = generated_answers
        .iter()
        .filter(|gen_ans| {
            exam_answers
                .iter()
                .any(|exam_ans| exam_ans.is_correct && &exam_ans.id == *gen_ans)
        })
        .collect();

    correct_generated_answers
        .iter()
        .all(|&correct_answer| attempt_answers.contains(correct_answer))
        && correct_generated_answers.len() == attempt_answers.len()
}
