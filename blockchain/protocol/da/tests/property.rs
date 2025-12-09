use da::{verify_da_proof, DAProvider, DASampler, InMemoryDA};
use proptest::prelude::*;
use tokio::runtime::Runtime;

proptest! {
    #[test]
    fn sampling_proofs_verify_for_random_blobs(blob in prop::collection::vec(any::<u8>(), 1..4096)) {
        let rt = Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let da = InMemoryDA::new();
            let blob = da.submit_blob("domain-A", &blob).await.unwrap();
            let proof = da.prove_blob_availability(&blob.id).await.unwrap();

            prop_assert!(verify_da_proof(&proof));
            prop_assert!(proof.samples.len() <= proof.commitment.total_shards);
            let sampled = da.sample(&blob.id, proof.samples.len()).await.unwrap();
            prop_assert!(sampled);
        });
    }
}

#[tokio::test]
async fn tampered_commitments_are_rejected() {
    let da = InMemoryDA::new();
    let blob = da.submit_blob("domain-B", b"resilient payload").await.unwrap();

    let mut proof = da.prove_blob_availability(&blob.id).await.unwrap();
    proof.commitment.root = [9u8; 32];
    assert!(!verify_da_proof(&proof));

    let mut proof2 = da.prove_blob_availability(&blob.id).await.unwrap();
    if let Some(sample) = proof2.samples.get_mut(0) {
        sample.merkle_path.push([7u8; 32]);
    }
    assert!(!verify_da_proof(&proof2));
}
