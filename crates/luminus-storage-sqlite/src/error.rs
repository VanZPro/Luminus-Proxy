use luminus_storage::StorageError;

pub(crate) fn map_sqlite_error(error: rusqlite::Error) -> StorageError {
    match error {
        rusqlite::Error::SqliteFailure(_, _) => StorageError::Unavailable,
        rusqlite::Error::InvalidColumnType(_, _, _)
        | rusqlite::Error::IntegralValueOutOfRange(_, _) => StorageError::CorruptData,
        rusqlite::Error::QueryReturnedNoRows => StorageError::CorruptData,
        _ => StorageError::Internal,
    }
}

pub(crate) fn map_join_error(_: tokio::task::JoinError) -> StorageError {
    StorageError::Internal
}
