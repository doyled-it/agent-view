use rusqlite::params;
use rusqlite::Result as SqlResult;

use super::Storage;

impl Storage {
    pub fn set_meta(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> SqlResult<Option<String>> {
        let result = self.conn.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );
        match result {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;

    #[test]
    fn test_metadata_crud() {
        let (storage, _dir) = test_storage();
        storage.set_meta("test_key", "test_value").unwrap();
        let val = storage.get_meta("test_key").unwrap();
        assert_eq!(val, Some("test_value".to_string()));

        let missing = storage.get_meta("nonexistent").unwrap();
        assert_eq!(missing, None);
    }
}
