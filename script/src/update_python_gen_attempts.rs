pub async fn _update_python_gen_attempts(client: Client) {
    let attempt_collection =
        get_collection::<prisma::ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt")
            .await;

    // Get all attempts for generation
    // Update with correct answer
    let res = attempt_collection
        .update_many(
            doc! {"generatedExamId": ObjectId::from_str("69398ad739d7be3806660016").unwrap()},
            vec![doc! {
            "$set": {
                "questionSets": {
                    "$concatArrays": [
                        "$questionSets", 
                        [{
                            "id": ObjectId::from_str("68e66c12f77abced35427f5a").unwrap(),
                            "questions": [
                                {
                                    "id": ObjectId::from_str("68e66c12f77abced35427f5b").unwrap(),
                                    "answers": [
                                        ObjectId::from_str("68e66c3ef77abced35427f5d").unwrap()
                                    ],
                                    // Adds 1000ms (1 second) to the record's startTime
                                    "submissionTime": { "$add": ["$startTime", 1000] }
                                }
                            ]
                        }]
                    ]
                }
            }
        }],
        )
        .await;

    println!("{res:?}");
}
