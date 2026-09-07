#![cfg(feature = "serde")]

use ferric_rules_runtime::{
    Engine, RunLimit, SerializationError, SerializationFormat, SnapshotFileError,
};

#[test]
fn bounded_snapshot_file_restores_behavior_and_preserves_error_families() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.cbor");
    let engine =
        Engine::with_rules("(deffacts seed (ready)) (defrule choose (ready) => (assert (chosen)))")
            .unwrap();
    std::fs::write(
        &path,
        engine.serialize(SerializationFormat::RECOMMENDED).unwrap(),
    )
    .unwrap();
    let mut restored =
        Engine::deserialize_from_file(&path, SerializationFormat::RECOMMENDED).unwrap();
    assert_eq!(restored.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(restored.find_facts("chosen").unwrap().len(), 1);

    std::fs::write(&path, b"corrupt").unwrap();
    assert!(matches!(
        Engine::deserialize_from_file(&path, SerializationFormat::RECOMMENDED),
        Err(SnapshotFileError::Serialization(
            SerializationError::LegacySnapshot
        ))
    ));
    // A sparse 1 GiB file must be stopped by the byte limit before decoding.
    std::fs::File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    assert!(matches!(
        Engine::deserialize_from_file(&path, SerializationFormat::RECOMMENDED),
        Err(SnapshotFileError::Serialization(
            SerializationError::LimitExceeded(_)
        ))
    ));
    std::fs::remove_file(&path).unwrap();
    assert!(
        matches!(Engine::deserialize_from_file(&path, SerializationFormat::RECOMMENDED), Err(SnapshotFileError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
}
