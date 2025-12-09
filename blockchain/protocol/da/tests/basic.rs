use da::{DAProvider, DASampler, InMemoryDA};

#[tokio::test]
async fn submit_and_sample_blob() {
    let da = InMemoryDA::new();
    let blob = da.submit_blob("l1", b"hello").await.unwrap();
    let proof = da.prove_blob_availability(&blob.id).await.unwrap();
    assert_eq!(proof.blob_id, blob.id);
    assert!(proof.commitment.total_shards >= proof.samples.len());
    // verify sampler validates merkle paths
    assert!(da.sample(&blob.id, 2).await.unwrap());
}
