// #![allow(incomplete_features)]
// #![feature(async_drop)]
use futures_util::TryStreamExt;
use moderation_service::{config::EnvVars, db};
// use mongo_drop::MongoDrop;
use mongodb::bson::{doc, oid::ObjectId};
use prisma;
/// Add exam data, add attempt data, call function, check if moderation record is created
/// Call again, ensure no more records are created
/// Add new attempt
/// Call again, ensure new record is created
/// Wait elapsed `moderation_length_in_s` check all attempts are approved
#[tokio::test]
#[tracing_test::traced_test]
async fn moderation_record_is_created() {
    dotenvy::dotenv().ok();
    let mongo_uri = std::env::var("MONGODB_URI").unwrap();
    let client = db::client(&mongo_uri).await.unwrap();

    // NOTE: Not working on latest nightly
    // let _guard = MongoDrop::new(&client.database("freecodecamp"))
    //     .await
    //     .unwrap();

    let moderation_collection = db::get_collection::<prisma::ExamEnvironmentExamModeration>(
        &client,
        "ExamEnvironmentExamModeration",
    )
    .await;
    let attempt_collection = db::get_collection::<prisma::ExamEnvironmentExamAttempt>(
        &client,
        "ExamEnvironmentExamAttempt",
    )
    .await;
    let exam_collection =
        db::get_collection::<prisma::ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;

    // Create 2 exams
    let exam_1 = prisma::ExamEnvironmentExam {
        id: ObjectId::new(),
        config: prisma::ExamEnvironmentConfig {
            total_time_in_m_s: 1000,
            ..Default::default()
        },
        ..Default::default()
    };
    let exam_2 = prisma::ExamEnvironmentExam {
        id: ObjectId::new(),
        config: prisma::ExamEnvironmentConfig {
            total_time_in_m_s: 2000,
            ..Default::default()
        },
        ..Default::default()
    };
    exam_collection
        .insert_many([&exam_1, &exam_2])
        .await
        .unwrap();

    // Used to expire attempts
    let exam_total_time = exam_1.config.total_time_in_m_s + 1_000;

    // Create 2 attempts
    let attempt_1 = prisma::ExamEnvironmentExamAttempt {
        id: ObjectId::new(),
        start_time_in_m_s: (mongodb::bson::DateTime::now().timestamp_millis() - exam_total_time)
            as f64,
        exam_id: exam_1.id,
        ..Default::default()
    };
    let attempt_2 = prisma::ExamEnvironmentExamAttempt {
        id: ObjectId::new(),
        start_time_in_m_s: (mongodb::bson::DateTime::now().timestamp_millis() - exam_total_time)
            as f64,
        exam_id: exam_2.id,
        ..Default::default()
    };
    attempt_collection
        .insert_many([&attempt_1, &attempt_2])
        .await
        .unwrap();

    let test_start_date = mongodb::bson::DateTime::now();

    let mut env_vars = EnvVars::new();

    // Should create two moderation records
    let _ = db::update_moderation_collection(&env_vars).await.unwrap();

    let moderation_records: Vec<prisma::ExamEnvironmentExamModeration> = moderation_collection
        .find(doc! {})
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    let record_1 = moderation_records
        .iter()
        .find(|r| r.exam_attempt_id == attempt_1.id)
        .unwrap();
    let record_2 = moderation_records
        .iter()
        .find(|r| r.exam_attempt_id == attempt_2.id)
        .unwrap();

    assert!(moderation_records.len() >= 2);
    assert_eq!(record_1.exam_attempt_id, attempt_1.id);
    assert_eq!(record_2.exam_attempt_id, attempt_2.id);
    assert_eq!(
        record_1.status,
        prisma::ExamEnvironmentExamModerationStatus::Pending
    );
    assert_eq!(
        record_2.status,
        prisma::ExamEnvironmentExamModerationStatus::Pending
    );
    assert_eq!(record_1.feedback, None);
    assert_eq!(record_2.feedback, None);
    assert_eq!(record_1.moderation_date, None);
    assert_eq!(record_2.moderation_date, None);
    // Submission date should be greater than `test_start_date`
    assert!(record_1.submission_date.timestamp_millis() > test_start_date.timestamp_millis());
    assert!(record_2.submission_date.timestamp_millis() > test_start_date.timestamp_millis());

    // Should not create any more moderation records
    let _ = db::update_moderation_collection(&env_vars).await.unwrap();
    let moderation_records_without_change: Vec<prisma::ExamEnvironmentExamModeration> =
        moderation_collection
            .find(doc! {})
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

    assert_eq!(
        moderation_records.len(),
        moderation_records_without_change.len()
    );

    // Create 3rd attempt
    let attempt_3 = prisma::ExamEnvironmentExamAttempt {
        id: ObjectId::new(),
        start_time_in_m_s: (mongodb::bson::DateTime::now().timestamp_millis() - exam_total_time)
            as f64,
        exam_id: exam_1.id,
        ..Default::default()
    };
    attempt_collection.insert_one(&attempt_3).await.unwrap();

    let test_start_date = mongodb::bson::DateTime::now();

    // Should add one more moderation record
    let _ = db::update_moderation_collection(&env_vars).await.unwrap();
    let moderation_record: prisma::ExamEnvironmentExamModeration = moderation_collection
        .find_one(doc! {
            "examAttemptId": attempt_3.id
        })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(moderation_record.exam_attempt_id, attempt_3.id);
    assert_eq!(
        moderation_record.status,
        prisma::ExamEnvironmentExamModerationStatus::Pending
    );
    assert_eq!(moderation_record.feedback, None);
    assert_eq!(moderation_record.moderation_date, None);
    // Submission date should be greater than `test_start_date`
    assert!(
        moderation_record.submission_date.timestamp_millis() > test_start_date.timestamp_millis()
    );

    env_vars.moderation_length_in_s = std::time::Duration::from_secs(1);

    // Ensure at least 1 second has passed
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;

    let _ = db::update_moderation_collection(&env_vars).await.unwrap();
    let moderation_records: Vec<prisma::ExamEnvironmentExamModeration> = moderation_collection
        .find(doc! {})
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();

    let record_1 = moderation_records
        .iter()
        .find(|r| r.exam_attempt_id == attempt_1.id)
        .unwrap();
    let record_2 = moderation_records
        .iter()
        .find(|r| r.exam_attempt_id == attempt_2.id)
        .unwrap();
    let record_3 = moderation_records
        .iter()
        .find(|r| r.exam_attempt_id == attempt_3.id)
        .unwrap();

    assert_eq!(
        record_1.status,
        prisma::ExamEnvironmentExamModerationStatus::Approved
    );
    assert_eq!(
        record_2.status,
        prisma::ExamEnvironmentExamModerationStatus::Approved
    );
    assert_eq!(
        record_3.status,
        prisma::ExamEnvironmentExamModerationStatus::Approved
    );

    assert_eq!(record_1.feedback, Some("Auto Approved".to_string()));
    assert_eq!(record_2.feedback, Some("Auto Approved".to_string()));
    assert_eq!(record_3.feedback, Some("Auto Approved".to_string()));
}
