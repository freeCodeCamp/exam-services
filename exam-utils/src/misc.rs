use mongodb::bson::oid::ObjectId;
use prisma;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::trace;

use crate::error::Error;

/// Calculates the attempt score, and compares score >= pass_score
pub fn check_attempt_pass(
    exam: &prisma::ExamEnvironmentExam,
    generated_exam: &prisma::ExamEnvironmentGeneratedExam,
    attempt: &prisma::ExamEnvironmentExamAttempt,
) -> bool {
    let passing_percent = exam.config.passing_percent;
    if let Ok(score) = calculate_score(exam, generated_exam, attempt) {
        return score >= passing_percent;
    }
    false
}

pub fn calculate_score(
    exam: &prisma::ExamEnvironmentExam,
    generated_exam: &prisma::ExamEnvironmentGeneratedExam,
    attempt: &prisma::ExamEnvironmentExamAttempt,
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
                let correct_exam_answer_ids: Vec<ObjectId> = exam_question
                    .answers
                    .iter()
                    .filter_map(|a| if a.is_correct { Some(a.id) } else { None })
                    .collect();
                let selected_answer_ids = attempt_question.answers.clone();
                assert!(correct_exam_answer_ids.contains(selected_answer_ids.get(0).unwrap()));
                correct_questions += 1;
            }
        }
    }

    Ok((correct_questions as f64 / total_questions as f64) * 100.0)
}

pub fn compare_answers(
    exam_answers: &[prisma::ExamEnvironmentAnswer],
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

    let answers_equal = correct_generated_answers
        .iter()
        .all(|&correct_answer| attempt_answers.contains(correct_answer));

    answers_equal && correct_generated_answers.len() == attempt_answers.len()
}

/// Validate Exam Config:
/// - `config.name` is not empty
/// - `config.passing_percent` is between 0 and 100
/// - `config.tags` is solvable
/// - `config.question_sets` is solvable
/// - `question_sets.questions.text` is not empty
/// - `question_sets.questions.answers` has at least one correct answer
/// - `question_sets.questions.answers.text` is not empty
///
/// A "solvable" config means that there are enough sets, questions, and answers to satisfy the constraints
pub fn validate_config(exam: &prisma::ExamEnvironmentExam) -> Result<(), String> {
    let config = &exam.config;
    let question_sets = &exam.question_sets;

    if config.name.is_empty() {
        return Err("Config name is empty".into());
    }

    if config.passing_percent < 0.0 || config.passing_percent > 100.0 {
        return Err("Config passing percent must be between 0.0 and 100.0".into());
    }

    // For each tag config, generate a map of (tag config, number of questions satisfying tag)
    // If any tag config `number_of_questions` > available questions with that tag, return error
    for tag_config in &config.tags {
        let mut available_questions = 0;
        for question_set in question_sets {
            for question in &question_set.questions {
                let group = &tag_config.group;
                // if `question.tags` includes all of `group`, then it satisfies the tag config
                if group.iter().all(|tag| question.tags.contains(tag)) {
                    available_questions += 1;
                }
            }
        }
        if available_questions < tag_config.number_of_questions as usize {
            return Err(format!(
                "Not enough questions for tag config: {:?}. Available: {}, Required: {}",
                tag_config, available_questions, tag_config.number_of_questions
            ));
        }
    }

    // For each question set config, ensure there are enough question sets of that type
    for qs_config in &config.question_sets {
        let available_question_sets = question_sets
            .iter()
            .filter(|qs| qs._type == qs_config._type)
            .count();
        if available_question_sets < qs_config.number_of_set as usize {
            return Err(format!(
                "Not enough question sets for question set config: {:?}. Available: {}, Required: {}",
                qs_config, available_question_sets, qs_config.number_of_set
            ));
        }
    }

    // For each `config.question_sets.number_of_questions`, ensure there are enough questions in the question sets of that type
    // Tally the total number of questions for a given type
    // Also, ensure for each question_set config, there exists a question set of that type with enough questions
    for qs_config in &config.question_sets {
        let mut total_questions = 0;
        let mut has_enough_in_single_set = false;
        for question_set in question_sets
            .iter()
            .filter(|qs| qs._type == qs_config._type)
        {
            let num_questions_in_set = question_set.questions.len();
            total_questions += num_questions_in_set;
            if num_questions_in_set >= qs_config.number_of_questions as usize {
                has_enough_in_single_set = true;
            }
        }
        if total_questions
            < qs_config.number_of_set as usize * qs_config.number_of_questions as usize
        {
            return Err(format!(
                "Not enough questions overall for question set config: {:?}. Available: {}, Required: {}",
                qs_config,
                total_questions,
                qs_config.number_of_set * qs_config.number_of_questions
            ));
        }
        if !has_enough_in_single_set {
            return Err(format!(
                "No single question set has enough questions for question set config: {:?}",
                qs_config
            ));
        }
    }

    // For each `config.question_sets.number_of_correct_answers` and `number_of_incorrect_answers`, ensure there are enough answers in the question sets of that type
    for qs_config in &config.question_sets {
        for question_set in question_sets
            .iter()
            .filter(|qs| qs._type == qs_config._type)
        {
            for question in &question_set.questions {
                let num_correct_answers = question.answers.iter().filter(|a| a.is_correct).count();
                let num_incorrect_answers =
                    question.answers.iter().filter(|a| !a.is_correct).count();
                if num_correct_answers < qs_config.number_of_correct_answers as usize {
                    return Err(format!(
                        "Not enough correct answers for question {:?} in question set {:?}. Available: {}, Required: {}",
                        question.id,
                        question_set.id,
                        num_correct_answers,
                        qs_config.number_of_correct_answers
                    ));
                }
                if num_incorrect_answers < qs_config.number_of_incorrect_answers as usize {
                    return Err(format!(
                        "Not enough incorrect answers for question {:?} in question set {:?}. Available: {}, Required: {}",
                        question.id,
                        question_set.id,
                        num_incorrect_answers,
                        qs_config.number_of_incorrect_answers
                    ));
                }
            }
        }
    }

    for qs in question_sets {
        for question in &qs.questions {
            if question.text.trim().is_empty() {
                return Err(format!("Question {:?} has empty text", question.id));
            }
            let has_correct_answer = question.answers.iter().any(|a| a.is_correct);
            if !has_correct_answer {
                return Err(format!("Question {:?} has no correct answers", question.id));
            }
            for answer in &question.answers {
                if answer.text.trim().is_empty() {
                    return Err(format!(
                        "Answer {:?} in question {:?} has empty text",
                        answer.id, question.id
                    ));
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExamInput {
    pub id: ObjectId,
    #[serde(rename = "questionSets")]
    pub question_sets: Vec<prisma::ExamEnvironmentQuestionSet>,
    pub config: prisma::ExamEnvironmentConfig,
}

#[derive(Debug, Clone)]
struct QuestionSetConfigWithQuestions {
    config: prisma::ExamEnvironmentQuestionSetConfig,
    question_sets: Vec<prisma::ExamEnvironmentQuestionSet>,
}

const TIMEOUT_IN_MS: u64 = 5_000;

/// Generates an exam for the user, based on the exam configuration.
pub fn generate_exam(exam: ExamInput) -> Result<prisma::ExamEnvironmentGeneratedExam, Error> {
    let start_time = Instant::now();
    let timeout = Duration::from_millis(TIMEOUT_IN_MS);

    let mut rng = rand::rng();

    // Shuffle question sets and their questions/answers
    let mut shuffled_question_sets: Vec<prisma::ExamEnvironmentQuestionSet> = exam
        .question_sets
        .into_iter()
        .map(|mut qs| {
            let mut shuffled_questions: Vec<prisma::ExamEnvironmentMultipleChoiceQuestion> = qs
                .questions
                .into_iter()
                .filter(|q| !q.deprecated)
                .map(|mut q| {
                    q.answers.shuffle(&mut rng);
                    q
                })
                .collect();
            shuffled_questions.shuffle(&mut rng);
            qs.questions = shuffled_questions;
            qs
        })
        .collect();
    shuffled_question_sets.shuffle(&mut rng);

    if exam.config.question_sets.is_empty() {
        return Err(Error::Generation(format!(
            "{}: Invalid exam config - no question sets config.",
            exam.id
        )));
    }

    // Convert question set config by type: [[all question sets of type], [another type], ...]
    let mut type_converted_question_sets_config: Vec<
        Vec<prisma::ExamEnvironmentQuestionSetConfig>,
    > = Vec::new();
    for config in exam.config.question_sets.iter() {
        if let Some(type_group) = type_converted_question_sets_config
            .iter_mut()
            .find(|group| group.first().map(|c| &c._type) == Some(&config._type))
        {
            type_group.push(config.clone());
        } else {
            type_converted_question_sets_config.push(vec![config.clone()]);
        }
    }

    // Sort each type group randomly (heuristic for retry)
    for group in type_converted_question_sets_config.iter_mut() {
        group.shuffle(&mut rng);
    }

    let sorted_question_sets_config: Vec<prisma::ExamEnvironmentQuestionSetConfig> =
        type_converted_question_sets_config
            .into_iter()
            .flatten()
            .collect();

    // Move all questions from set that are used to fulfill tag config.
    let mut question_sets_config_with_questions: Vec<QuestionSetConfigWithQuestions> =
        sorted_question_sets_config
            .into_iter()
            .map(|config| QuestionSetConfigWithQuestions {
                config,
                question_sets: Vec::new(),
            })
            .collect();

    // Sort tag config by number of tags in descending order.
    let mut sorted_tag_config = exam.config.tags.clone();
    sorted_tag_config.sort_by(|a, b| b.group.len().cmp(&a.group.len()));

    // Main allocation loop
    'question_sets_config_loop: for qsc_with_qs in question_sets_config_with_questions.iter_mut() {
        'sorted_tag_config_loop: for tag_config in sorted_tag_config.iter_mut() {
            // Collect questions to remove (question_set_id, question_id)
            let mut questions_to_remove: Vec<(ObjectId, ObjectId)> = Vec::new();

            for question_set in shuffled_question_sets
                .iter_mut()
                .filter(|sqs| sqs._type == qsc_with_qs.config._type)
            {
                // If questionSet does not have enough questions for config, do not consider.
                if qsc_with_qs.config.number_of_questions > question_set.questions.len() as i64 {
                    trace!(
                        number_of_questions = question_set.questions.len(),
                        "skipping. not enough questions in question set"
                    );
                    continue;
                }
                // If tagConfig is finished, skip.
                if tag_config.number_of_questions == 0 {
                    trace!(?tag_config.group, "skipping. tag config fulfilled");
                    continue 'sorted_tag_config_loop;
                }
                // If questionSetConfig has been fulfilled, skip.
                if is_question_set_config_fulfilled(qsc_with_qs) {
                    trace!(?qsc_with_qs, "skipping. question set config fulfilled");
                    continue 'question_sets_config_loop;
                }

                // Store question_set id and metadata before mutable borrow
                let question_set_id = question_set.id;
                let question_set_type = question_set._type.clone();
                let question_set_context = question_set.context.clone();

                // Find question with at least all tags in the set.
                let questions: Vec<&mut prisma::ExamEnvironmentMultipleChoiceQuestion> =
                    question_set
                        .questions
                        .iter_mut()
                        .filter(|q| tag_config.group.iter().all(|t| q.tags.contains(t)))
                        .collect();

                // trace!(
                //     number_of_questions = questions.len(),
                //     ?tag_config,
                //     "questions fulfilling tags"
                // );

                for question in questions {
                    // Does question fulfill criteria for questionSetConfig:
                    let number_of_correct_answers =
                        question.answers.iter().filter(|a| a.is_correct).count() as i64;
                    let number_of_incorrect_answers =
                        question.answers.iter().filter(|a| !a.is_correct).count() as i64;

                    if qsc_with_qs.config.number_of_correct_answers <= number_of_correct_answers
                        && qsc_with_qs.config.number_of_incorrect_answers
                            <= number_of_incorrect_answers
                    {
                        if is_question_set_config_fulfilled(qsc_with_qs) {
                            continue 'question_sets_config_loop;
                        }

                        // Push questionSet if it does not exist. Otherwise, just push question
                        let qscqs = qsc_with_qs
                            .question_sets
                            .iter_mut()
                            .find(|qs| qs.id == question_set_id);

                        let question_with_correct_number_of_answers =
                            get_question_with_random_answers(question, &qsc_with_qs.config)?;

                        if let Some(existing_qs) = qscqs {
                            if existing_qs.questions.len()
                                == qsc_with_qs.config.number_of_questions as usize
                            {
                                break;
                            }
                            existing_qs
                                .questions
                                .push(question_with_correct_number_of_answers);
                        } else {
                            if qsc_with_qs.question_sets.len()
                                == qsc_with_qs.config.number_of_set as usize
                            {
                                break;
                            }
                            // Create new question set from stored metadata
                            let new_question_set = prisma::ExamEnvironmentQuestionSet {
                                id: question_set_id,
                                _type: question_set_type.clone(),
                                context: question_set_context.clone(),
                                questions: vec![question_with_correct_number_of_answers],
                            };
                            qsc_with_qs.question_sets.push(new_question_set);
                        }

                        // Mark question for removal
                        questions_to_remove.push((question_set_id, question.id));

                        tag_config.number_of_questions -= 1;
                    }
                }
            }

            // Remove marked questions after iteration
            for (qs_id, q_id) in questions_to_remove {
                if let Some(qs) = shuffled_question_sets.iter_mut().find(|qs| qs.id == qs_id) {
                    qs.questions.retain(|q| q.id != q_id);
                }
            }
        }

        // Add questions to questionSetsConfigWithQuestions until fulfilled.
        while !is_question_set_config_fulfilled(qsc_with_qs) {
            if start_time.elapsed() > timeout {
                return Err(Error::Generation(format!(
                    "Unable to generate exam within {}ms",
                    TIMEOUT_IN_MS
                )));
            }

            // Ensure all questionSets ARE FULL
            if (qsc_with_qs.config.number_of_set as usize) > qsc_with_qs.question_sets.len() {
                // Find a question set with enough questions and answers to fulfill config
                let question_set = shuffled_question_sets
                    .iter()
                    .find(|qs| {
                        if qs._type == qsc_with_qs.config._type && qs.questions.len() >= qsc_with_qs.config.number_of_questions as usize
                            {
                                let questions: Vec<&prisma::ExamEnvironmentMultipleChoiceQuestion> = qs
                                    .questions
                                    .iter()
                                    .filter(|q| {
                                        let number_of_correct_answers =
                                            q.answers.iter().filter(|a| a.is_correct).count()
                                                as i64;
                                        let number_of_incorrect_answers =
                                            q.answers.iter().filter(|a| !a.is_correct).count()
                                                as i64;
                                        number_of_correct_answers
                                            >= qsc_with_qs.config.number_of_correct_answers
                                            && number_of_incorrect_answers
                                                >= qsc_with_qs.config.number_of_incorrect_answers
                                    })
                                    .collect();

                                return questions.len()
                                    >= qsc_with_qs.config.number_of_questions as usize;
                        }

                        false
                    })
                    .cloned()
                    .ok_or_else(|| {
                        Error::Generation(format!(
                            "Invalid Exam Configuration for {}. Not enough questions for question type {:?}.",
                            exam.id, qsc_with_qs.config._type
                        ))
                    })?;

                trace!("question set found to fulfill");

                // Remove questionSet from shuffledQuestionSets
                let question_set_id = question_set.id;
                shuffled_question_sets.retain(|qs| qs.id != question_set_id);

                // Find question with enough answers to fulfill config
                let questions: Vec<prisma::ExamEnvironmentMultipleChoiceQuestion> = question_set
                    .questions
                    .iter()
                    .filter(|q| {
                        let number_of_correct_answers =
                            q.answers.iter().filter(|a| a.is_correct).count() as i64;
                        let number_of_incorrect_answers =
                            q.answers.iter().filter(|a| !a.is_correct).count() as i64;
                        number_of_correct_answers >= qsc_with_qs.config.number_of_correct_answers
                            && number_of_incorrect_answers
                                >= qsc_with_qs.config.number_of_incorrect_answers
                    })
                    .cloned()
                    .collect();

                let num_to_add = qsc_with_qs.config.number_of_questions as usize;
                let questions_with_correct_answers: Result<
                    Vec<prisma::ExamEnvironmentMultipleChoiceQuestion>,
                    Error,
                > = questions
                    .iter()
                    .take(num_to_add)
                    .map(|q| get_question_with_random_answers(q, &qsc_with_qs.config))
                    .collect();

                let mut question_set_with_correct_number_of_answers = question_set.clone();
                question_set_with_correct_number_of_answers.questions =
                    questions_with_correct_answers?;

                trace!(
                    number_of_questions =
                        question_set_with_correct_number_of_answers.questions.len(),
                    "pushing question set to question set with config"
                );
                qsc_with_qs
                    .question_sets
                    .push(question_set_with_correct_number_of_answers);
            }

            // Ensure all existing questionSets have correct number of questions
            for question_set in qsc_with_qs.question_sets.iter_mut() {
                if (question_set.questions.len() as i64) < qsc_with_qs.config.number_of_questions {
                    let questions: Vec<prisma::ExamEnvironmentMultipleChoiceQuestion> =
                        shuffled_question_sets
                            .iter()
                            .find(|qs| qs.id == question_set.id)
                            .ok_or_else(|| {
                                Error::Generation(
                                    format!(
                                    "Invalid Exam Configuration for {}. Not enough questions for question type {:?}.",
                                    exam.id, qsc_with_qs.config._type
                                ))
                            })?
                            .questions
                            .iter()
                            .filter(|q| !question_set.questions.iter().any(|qsq| qsq.id == q.id))
                            .cloned()
                            .collect();

                    let questions_with_enough_answers: Vec<
                        prisma::ExamEnvironmentMultipleChoiceQuestion,
                    > = questions
                        .into_iter()
                        .filter(|q| {
                            let number_of_correct_answers =
                                q.answers.iter().filter(|a| a.is_correct).count() as i64;
                            let number_of_incorrect_answers =
                                q.answers.iter().filter(|a| !a.is_correct).count() as i64;
                            number_of_correct_answers
                                >= qsc_with_qs.config.number_of_correct_answers
                                && number_of_incorrect_answers
                                    >= qsc_with_qs.config.number_of_incorrect_answers
                        })
                        .collect();

                    // Push as many questions as needed to fulfill questionSetConfig
                    let num_to_add = (qsc_with_qs.config.number_of_questions as usize)
                        - question_set.questions.len();
                    let questions_to_add: Vec<prisma::ExamEnvironmentMultipleChoiceQuestion> =
                        questions_with_enough_answers
                            .into_iter()
                            .take(num_to_add)
                            .collect();

                    let questions_with_correct_answers: Result<
                        Vec<prisma::ExamEnvironmentMultipleChoiceQuestion>,
                        Error,
                    > = questions_to_add
                        .iter()
                        .map(|q| get_question_with_random_answers(q, &qsc_with_qs.config))
                        .collect();

                    question_set
                        .questions
                        .extend(questions_with_correct_answers?);

                    // Remove questions from shuffledQuestionSets
                    for q in questions_to_add.iter() {
                        if let Some(qs) = shuffled_question_sets
                            .iter_mut()
                            .find(|qs| qs.id == question_set.id)
                        {
                            qs.questions.retain(|qs_q| qs_q.id != q.id);
                        }
                    }
                }
            }
        }
    }

    trace!("finished question set config");

    // Update tag config for cases where one question fulfills multiple tag configs
    for qsc_with_qs in question_sets_config_with_questions.iter() {
        for question_set in qsc_with_qs.question_sets.iter() {
            for question in question_set.questions.iter() {
                for tag_config in sorted_tag_config.iter_mut() {
                    if tag_config.number_of_questions <= 0 {
                        continue;
                    }
                    if tag_config.group.iter().all(|t| question.tags.contains(t)) {
                        tag_config.number_of_questions -= 1;
                    }
                }
            }
        }
    }

    trace!("finished updating tag config post question set config");

    // Verify all tag configs are fulfilled
    for tag_config in sorted_tag_config.iter() {
        if tag_config.number_of_questions > 0 {
            return Err(Error::Generation(format!(
                "Invalid Exam Configuration for exam \"{}\". Not enough questions for tag group \"{}\".",
                exam.id,
                tag_config.group.join(",")
            )));
        }
    }

    trace!("all tag configs are fulfilled");

    // Build the final generated exam structure
    let question_sets: Vec<prisma::ExamEnvironmentGeneratedQuestionSet> =
        question_sets_config_with_questions
            .into_iter()
            .flat_map(|qsc| {
                qsc.question_sets.into_iter().map(|qs| {
                    let questions: Vec<prisma::ExamEnvironmentGeneratedMultipleChoiceQuestion> = qs
                        .questions
                        .into_iter()
                        .map(|q| {
                            let answers: Vec<ObjectId> =
                                q.answers.into_iter().map(|a| a.id).collect();
                            prisma::ExamEnvironmentGeneratedMultipleChoiceQuestion {
                                id: q.id,
                                answers,
                            }
                        })
                        .collect();
                    prisma::ExamEnvironmentGeneratedQuestionSet {
                        id: qs.id,
                        questions,
                    }
                })
            })
            .collect();

    Ok(prisma::ExamEnvironmentGeneratedExam {
        id: ObjectId::new(),
        exam_id: exam.id,
        question_sets,
        deprecated: false,
        version: 1,
    })
}

/// Returns the configs that are not fulfilled
fn is_question_set_config_fulfilled(qsc_with_qs: &QuestionSetConfigWithQuestions) -> bool {
    let enough_of_set =
        qsc_with_qs.config.number_of_set as usize == qsc_with_qs.question_sets.len();

    if !enough_of_set {
        trace!(
            expected = qsc_with_qs.config.number_of_set,
            actual = qsc_with_qs.question_sets.len(),
            "not enough of set"
        );
    }

    let enough_questions = qsc_with_qs.question_sets.iter().all(|qs| {
        let num_questions = qs.questions.len();
        let expected_num_questions = qsc_with_qs.config.number_of_questions as usize;
        if num_questions != expected_num_questions {
            trace!(
                expected = expected_num_questions,
                actual = num_questions,
                "number of questions required in config does not match number of questions"
            );
        }

        num_questions == expected_num_questions
    });

    enough_of_set && enough_questions
}

/// Gets random answers for a question.
fn get_question_with_random_answers(
    question: &prisma::ExamEnvironmentMultipleChoiceQuestion,
    question_set_config: &prisma::ExamEnvironmentQuestionSetConfig,
) -> Result<prisma::ExamEnvironmentMultipleChoiceQuestion, Error> {
    let mut rng = rand::rng();
    let mut random_answers = question.answers.clone();
    random_answers.shuffle(&mut rng);

    let incorrect_answers: Vec<prisma::ExamEnvironmentAnswer> = random_answers
        .iter()
        .filter(|a| !a.is_correct)
        .take(question_set_config.number_of_incorrect_answers as usize)
        .cloned()
        .collect();

    let correct_answers: Vec<prisma::ExamEnvironmentAnswer> = random_answers
        .iter()
        .filter(|a| a.is_correct)
        .take(question_set_config.number_of_correct_answers as usize)
        .cloned()
        .collect();

    if incorrect_answers.is_empty() || correct_answers.is_empty() {
        return Err(Error::Generation(format!(
            "Question {} does not have enough correct/incorrect answers.",
            question.id
        )));
    }

    let mut answers = incorrect_answers;
    answers.extend(correct_answers);

    let mut result = question.clone();
    result.answers = answers;
    Ok(result)
}
