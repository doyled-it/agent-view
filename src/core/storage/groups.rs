use rusqlite::params;
use rusqlite::Result as SqlResult;

use super::Storage;
use crate::types::Group;

impl Storage {
    /// Load all groups ordered by sort_order
    pub fn load_groups(&self) -> SqlResult<Vec<Group>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, name, expanded, sort_order, default_path
             FROM groups ORDER BY sort_order",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(Group {
                path: row.get(0)?,
                name: row.get(1)?,
                expanded: row.get::<_, i32>(2)? == 1,
                order: row.get(3)?,
                default_path: row.get(4)?,
            })
        })?;

        rows.collect()
    }

    /// Save a group (insert or replace)
    pub fn save_group(&self, group: &Group) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO groups (path, name, expanded, sort_order, default_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                group.path,
                group.name,
                group.expanded as i32,
                group.order,
                group.default_path,
            ],
        )?;
        Ok(())
    }

    /// Delete a group by path
    pub fn delete_group(&self, path: &str) -> SqlResult<()> {
        self.conn
            .execute("DELETE FROM groups WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Swap the sort_order of two groups by path
    pub fn swap_group_order(&self, path_a: &str, path_b: &str) -> SqlResult<()> {
        let order_a: i32 = self.conn.query_row(
            "SELECT sort_order FROM groups WHERE path = ?1",
            params![path_a],
            |row| row.get(0),
        )?;
        let order_b: i32 = self.conn.query_row(
            "SELECT sort_order FROM groups WHERE path = ?1",
            params![path_b],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "UPDATE groups SET sort_order = ?1 WHERE path = ?2",
            params![order_b, path_a],
        )?;
        self.conn.execute(
            "UPDATE groups SET sort_order = ?1 WHERE path = ?2",
            params![order_a, path_b],
        )?;
        Ok(())
    }

    /// Toggle the expanded state of a group
    pub fn toggle_group_expanded(&self, path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE groups SET expanded = CASE WHEN expanded = 1 THEN 0 ELSE 1 END WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use crate::types::Group;

    #[test]
    fn test_save_and_load_groups() {
        let (storage, _dir) = test_storage();
        let group = Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&group).unwrap();

        let groups = storage.load_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Work");
        assert!(groups[0].expanded);
    }

    #[test]
    fn test_delete_group() {
        let (storage, _dir) = test_storage();
        let group = Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&group).unwrap();
        storage.delete_group("work").unwrap();
        let groups = storage.load_groups().unwrap();
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_swap_group_order() {
        let (storage, _dir) = test_storage();
        let g1 = Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 0,
            default_path: String::new(),
        };
        let g2 = Group {
            path: "personal".to_string(),
            name: "Personal".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&g1).unwrap();
        storage.save_group(&g2).unwrap();

        storage.swap_group_order("work", "personal").unwrap();

        let groups = storage.load_groups().unwrap();
        assert_eq!(groups[0].path, "personal");
        assert_eq!(groups[1].path, "work");
    }

    #[test]
    fn test_toggle_group_expanded() {
        let (storage, _dir) = test_storage();
        let group = Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&group).unwrap();
        storage.toggle_group_expanded("work").unwrap();
        let groups = storage.load_groups().unwrap();
        assert!(!groups[0].expanded);
    }
}
