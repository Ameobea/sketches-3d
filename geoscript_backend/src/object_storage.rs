use std::sync::{Arc, OnceLock};

use object_store::{aws::AmazonS3Builder, path::Path, ObjectStore, PutPayload};

static OBJECT_STORE: OnceLock<Arc<dyn ObjectStore>> = OnceLock::new();

fn get_object_store() -> &'static Arc<dyn ObjectStore> {
  OBJECT_STORE.get_or_init(|| {
    let settings = &crate::server::settings().object_storage;
    let object_store = AmazonS3Builder::new()
      .with_access_key_id(&settings.access_key)
      .with_secret_access_key(&settings.secret_access_key)
      .with_endpoint(&settings.endpoint)
      .with_region("auto")
      .with_bucket_name(&settings.bucket_name)
      .build()
      .map(|s3| Arc::new(s3) as Arc<dyn ObjectStore + 'static>)
      .expect("Failed to build object store");
    object_store
  })
}

/// Uploads an object to cloud storage. Returns the public URL of the object.
pub async fn upload_object(
  key: &str,
  data: impl Into<PutPayload>,
) -> Result<String, object_store::Error> {
  let store = get_object_store();
  let path = Path::from(key);
  store.put(&path, data.into()).await?;
  let settings = &crate::server::settings().object_storage;

  let url = format!("{}/{key}", settings.public_endpoint);

  info!("Uploaded object to {url}");
  Ok(url)
}
