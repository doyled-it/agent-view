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

    /// Renumber every group to a dense, unique sort_order based on current
    /// ordering, breaking ties by path so the result is deterministic.
    pub fn renumber_groups(&self) -> SqlResult<()> {
        self.conn.execute(
            "WITH ordered AS (
                SELECT path,
                       ROW_NUMBER() OVER (ORDER BY sort_order, path) - 1 AS new_order
                FROM groups
            )
            UPDATE groups
               SET sort_order = (SELECT new_order FROM ordered WHERE ordered.path = groups.path)",
            [],
        )?;
        Ok(())
    }

    /// Swap the sort_order of two groups by path. Renumbers first so ties in
    /// sort_order can't reduce the swap to a no-op.
    pub fn swap_group_order(&self, path_a: &str, path_b: &str) -> SqlResult<()> {
        self.renumber_groups()?;
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
    fn test_swap_group_order_breaks_ties() {
        // Two groups sharing the same sort_order would previously make swap a
        // no-op since both ends got the same value back. Renumbering first
        // ensures the swap actually moves them.
        let (storage, _dir) = test_storage();
        let a = Group {
            path: "alpha".to_string(),
            name: "Alpha".to_string(),
            expanded: true,
            order: 2,
            default_path: String::new(),
        };
        let b = Group {
            path: "bravo".to_string(),
            name: "Bravo".to_string(),
            expanded: true,
            order: 2,
            default_path: String::new(),
        };
        storage.save_group(&a).unwrap();
        storage.save_group(&b).unwrap();

        storage.swap_group_order("alpha", "bravo").unwrap();

        let groups = storage.load_groups().unwrap();
        assert_eq!(groups.len(), 2);
        assert_ne!(
            groups[0].order, groups[1].order,
            "orders should be unique after swap"
        );
        assert_eq!(groups[0].path, "bravo");
        assert_eq!(groups[1].path, "alpha");
    }

    #[test]
    fn test_renumber_groups_makes_orders_dense_and_unique() {
        let (storage, _dir) = test_storage();
        for (path, order) in [("a", 5), ("b", 5), ("c", 10), ("d", 0)] {
            storage
                .save_group(&Group {
                    path: path.to_string(),
                    name: path.to_uppercase(),
                    expanded: true,
                    order,
                    default_path: String::new(),
                })
                .unwrap();
        }

        storage.renumber_groups().unwrap();

        let groups = storage.load_groups().unwrap();
        let orders: Vec<i32> = groups.iter().map(|g| g.order).collect();
        assert_eq!(orders, vec![0, 1, 2, 3]);
        // Tie between "a" and "b" at order=5 broken by path.
        let paths: Vec<&str> = groups.iter().map(|g| g.path.as_str()).collect();
        assert_eq!(paths, vec!["d", "a", "b", "c"]);
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
