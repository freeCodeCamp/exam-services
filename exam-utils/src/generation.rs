use crate::error::Error;

/// Given an exam, use config to create generations.
///
/// Can fail if config is invalid, or if algorithm gets stuck.
pub fn try_generate<E>(_exam: E) -> Result<prisma::ExamEnvironmentGeneratedExam, Error>
where
    E: Into<prisma::ExamEnvironmentExam>,
{
    unimplemented!()
}

/// Given a generation, validate it for basic properties:
/// 1) No duplicates
pub fn validate_generation(generation: &prisma::ExamEnvironmentGeneratedExam) -> Result<(), Error> {
    let question_sets = &generation.question_sets;

    let mut qs_ids = vec![];
    let mut q_ids = vec![];
    let mut a_ids = vec![];
    for qs in question_sets {
        if qs_ids.contains(&qs.id) {
            return Err(Error::Generation(format!(
                "question set id {} duplicate of question set id",
                qs.id
            )));
        }
        if q_ids.contains(&qs.id) {
            return Err(Error::Generation(format!(
                "question set id {} duplicate of question id",
                qs.id
            )));
        }
        if a_ids.contains(&qs.id) {
            return Err(Error::Generation(format!(
                "question set id {} duplicate of answer id",
                qs.id
            )));
        }
        qs_ids.push(qs.id);

        for q in &qs.questions {
            if qs_ids.contains(&q.id) {
                return Err(Error::Generation(format!(
                    "question id {} duplicate of question set id",
                    q.id
                )));
            }
            if q_ids.contains(&q.id) {
                return Err(Error::Generation(format!(
                    "question id {} duplicate of question id",
                    q.id
                )));
            }
            if a_ids.contains(&q.id) {
                return Err(Error::Generation(format!(
                    "question id {} duplicate of answer id",
                    q.id
                )));
            }
            q_ids.push(q.id);

            for a in &q.answers {
                if qs_ids.contains(&a) {
                    return Err(Error::Generation(format!(
                        "answer id {} duplicate of question set id",
                        a
                    )));
                }
                if q_ids.contains(&a) {
                    return Err(Error::Generation(format!(
                        "answer id {} duplicate of question id",
                        a
                    )));
                }
                if a_ids.contains(&a) {
                    return Err(Error::Generation(format!(
                        "answer id {} duplicate of answer id",
                        a
                    )));
                }

                a_ids.push(*a);
            }
        }
    }

    Ok(())
}
