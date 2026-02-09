use std::collections::HashMap;

use mongodb::{
    Client, Collection,
    bson::{Document, doc, oid::ObjectId},
    options::ClientOptions,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tracing::{error, instrument};

pub async fn get_collection<'d, T>(client: &Client, collection_name: &str) -> Collection<T>
where
    T: Send + Sync + Deserialize<'d> + Serialize,
{
    let db = client
        .default_database()
        .expect("database needs to be defined in the URI");

    let collection = db.collection::<T>(collection_name);
    collection
}

pub async fn client(uri: &str) -> mongodb::error::Result<Client> {
    let mut client_options = ClientOptions::parse(uri).await?;

    client_options.app_name = Some(env!("CARGO_CRATE_NAME").to_string());

    // Get a handle to the cluster
    let client = Client::with_options(client_options)?;

    // Ping the server to see if you can connect to the cluster
    client
        .default_database()
        .expect("database needs to be defined in the URI")
        // .database("freecodecamp")
        .run_command(doc! {"ping": 1})
        .await?;

    Ok(client)
}

#[instrument(skip_all, fields(collection = collection.name(), query = query.to_string()))]
pub async fn get_from_cache_or_collection<T>(
    collection: &Collection<T>,
    query: Document,
    hash: &mut HashMap<ObjectId, T>,
    id: ObjectId,
) -> Option<T>
where
    T: DeserializeOwned + Send + Sync + Clone,
{
    let item = if let Some(item) = hash.get(&id) {
        item.to_owned()
    } else {
        let item = match collection.find_one(query).await {
            Ok(i) => match i {
                Some(i) => i,
                None => {
                    error!("enoent record");
                    return None;
                }
            },
            Err(e) => {
                error!(error = ?e, "unable to query database");
                return None;
            }
        };

        hash.insert(id, item.clone());
        item
    };

    Some(item)
}
