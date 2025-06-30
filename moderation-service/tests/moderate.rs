use futures_util::TryStreamExt;
use moderation_service::{
    db::{self, update_moderation_collection},
    prisma::{self, EnvExam, EnvExamAttempt, EnvExamModeration},
};
use mongo_drop::MongoDrop;
use mongodb::bson::{doc, oid::ObjectId};

/// Add exam data, add attempt data, call function, check if moderation record is created
/// Call again, ensure no more records are created
/// Add new attempt
/// Call again, ensure new record is created
#[tokio::test]
async fn moderation_record_is_created() {
    dotenvy::dotenv().ok();
    let mongo_uri = std::env::var("MONGOHQ_URL").unwrap();
    let client = db::client(&mongo_uri).await.unwrap();

    let _guard = MongoDrop::new(&client.database("freecodecamp"))
        .await
        .unwrap();

    let moderation_collection =
        db::get_collection::<EnvExamModeration>(&client, "EnvExamModeration").await;
    let attempt_collection = db::get_collection::<EnvExamAttempt>(&client, "EnvExamAttempt").await;
    let exam_collection = db::get_collection::<EnvExam>(&client, "EnvExam").await;

    let exam_1 = EnvExam {
        id: ObjectId::new(),
        config: prisma::EnvExamConfig {
            total_time_in_ms: 1000,
        },
        ..Default
    };
    let exam_2 = EnvExam {
        id: ObjectId::new(),
        config: prisma::EnvExamConfig {
            total_time_in_ms: 2000,
        },
        ..Default
    };
    exam_collection
        .insert_many([&exam_1, &exam_2])
        .await
        .unwrap();

    let attempt_1 = EnvExamAttempt {
        id: ObjectId::new(),
        start_time_in_m_s: 0,
        exam_id: exam_1.id,
        ..Default
    };
    let attempt_2 = EnvExamAttempt {
        id: ObjectId::new(),
        start_time_in_m_s: 0,
        exam_id: exam_2.id,
        ..Default
    };
    attempt_collection
        .insert_many([&attempt_1, &attempt_2])
        .await
        .unwrap();

    let test_start_date = mongodb::bson::DateTime::now();

    let _ = update_moderation_collection().await.unwrap();

    let moderation_records: Vec<EnvExamModeration> = moderation_collection
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
    assert_eq!(record_1.approved, false);
    assert_eq!(record_2.approved, false);
    assert_eq!(record_1.feedback, None);
    assert_eq!(record_2.feedback, None);
    assert_eq!(record_1.moderation_date, None);
    assert_eq!(record_2.moderation_date, None);
    // Submission date should be greater than `test_start_date`
    assert!(record_1.submission_date.timestamp_millis() > test_start_date.timestamp_millis());
    assert!(record_2.submission_date.timestamp_millis() > test_start_date.timestamp_millis());

    let _ = update_moderation_collection().await.unwrap();
    let moderation_records_without_change: Vec<EnvExamModeration> = moderation_collection
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

    let attempt_3 = EnvExamAttempt {
        id: ObjectId::new(),
        start_time_in_m_s: 0,
        exam_id: exam_1.id,
        ..Default
    };
    attempt_collection.insert_one(&attempt_3).await.unwrap();

    let test_start_date = mongodb::bson::DateTime::now();

    let _ = update_moderation_collection().await.unwrap();
    let moderation_record: EnvExamModeration = moderation_collection
        .find_one(doc! {
            "examAttemptId": attempt_3.id
        })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(moderation_record.exam_attempt_id, attempt_3.id);
    assert_eq!(moderation_record.approved, false);
    assert_eq!(moderation_record.feedback, None);
    assert_eq!(moderation_record.moderation_date, None);
    // Submission date should be greater than `test_start_date`
    assert!(
        moderation_record.submission_date.timestamp_millis() > test_start_date.timestamp_millis()
    );
}
