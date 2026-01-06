pub async fn _validate_all_generations(client: Client) {
    let generation_collection = get_collection::<prisma::ExamEnvironmentGeneratedExam>(
        &client,
        "ExamEnvironmentGeneratedExam",
    )
    .await;

    let generation_count = generation_collection
        .count_documents(doc! {})
        .await
        .expect("unable to get generation count");

    // Get generations
    let mut generation_cursor = generation_collection
        .find(doc! {})
        .batch_size(20)
        .await
        .unwrap();

    let pb = ProgressBar::new(generation_count);

    while let Some(generation) = generation_cursor.next().await {
        let generation = generation.expect("unable to deserialize generation");
        match exam_utils::generation::validate_generation(&generation) {
            Ok(_) => {}
            Err(e) => {
                println!("Invalid Generation: {}", generation.id);
                eprintln!("{e:?}");
            }
        }
        pb.inc(1)
    }

    pb.finish_with_message("Generations checked");
}
