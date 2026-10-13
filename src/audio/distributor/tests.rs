use super::*;

#[tokio::test]
async fn test_distributor_basic() {
    let distributor = AudioDistributor::new();

    // Subscribe to receive frames
    let mut rx1 = distributor.subscribe_pcm();
    let mut rx2 = distributor.subscribe_pcm();

    // Broadcast some test frames
    let test_frames = vec![[1.0, 2.0], [3.0, 4.0]];
    let count = distributor.broadcast_pcm(test_frames.clone());

    assert_eq!(count, 2);

    // Both receivers should get the same data
    let received1 = rx1.recv().await.unwrap();
    let received2 = rx2.recv().await.unwrap();

    assert_eq!(received1, test_frames);
    assert_eq!(received2, test_frames);
}

#[tokio::test]
async fn test_distributor_no_receivers() {
    let distributor = AudioDistributor::new();

    // Broadcast with no receivers should not panic
    let test_frames = vec![[1.0, 2.0]];
    let count = distributor.broadcast_pcm(test_frames);

    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_opus_distribution() {
    let distributor = AudioDistributor::new();

    let mut rx = distributor.subscribe_opus();

    let test_packet = vec![1, 2, 3, 4];
    let count = distributor.broadcast_opus(test_packet.clone());

    assert_eq!(count, 1);

    let received = rx.recv().await.unwrap();
    assert_eq!(received, test_packet);
}
