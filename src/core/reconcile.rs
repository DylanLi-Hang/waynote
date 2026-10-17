use std::collections::{HashMap, HashSet};

/// What the app already believes is on disk (after its own loads/saves).
pub struct Known {
    pub id: String,
    pub path: String,
    pub hash: String,
}

/// A file found during a fresh rescan of the notes dir.
pub struct Scanned {
    pub id: String,
    pub path: String,
    pub hash: String,
}

#[derive(PartialEq, Debug)]
pub enum Change {
    Added(String),
    Modified(String),
    Removed(String),
    Renamed { id: String, new_path: String },
}

/// Reconcile a rescan against the latest loaded or saved state.
pub fn reconcile(
    known: &[Known],
    scanned: &[Scanned],
) -> Vec<Change> {
    let known_by_id: HashMap<&str, &Known> =
        known.iter().map(|k| (k.id.as_str(), k)).collect();

    let mut changes = Vec::new();

    for s in scanned {
        match known_by_id.get(s.id.as_str()) {
            None => changes.push(Change::Added(s.id.clone())),
            Some(k) => classify_known(k, s, &mut changes),
        }
    }

    append_removals(known, scanned, &mut changes);
    changes
}

fn classify_known(k: &Known, s: &Scanned, changes: &mut Vec<Change>) {
    if k.hash == s.hash && k.path == s.path {
        return;
    }
    if k.hash == s.hash {
        changes.push(Change::Renamed {
            id: s.id.clone(),
            new_path: s.path.clone(),
        });
    } else {
        changes.push(Change::Modified(s.id.clone()));
    }
}

fn append_removals(known: &[Known], scanned: &[Scanned], changes: &mut Vec<Change>) {
    let scanned_ids: HashSet<&str> = scanned.iter().map(|s| s.id.as_str()).collect();
    for k in known {
        if !scanned_ids.contains(k.id.as_str()) {
            changes.push(Change::Removed(k.id.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(id: &str, path: &str, hash: &str) -> Known {
        Known { id: id.to_string(), path: path.to_string(), hash: hash.to_string() }
    }

    fn scanned(id: &str, path: &str, hash: &str) -> Scanned {
        Scanned { id: id.to_string(), path: path.to_string(), hash: hash.to_string() }
    }

    fn sorted(mut changes: Vec<Change>) -> Vec<Change> {
        changes.sort_by_key(|c| match c {
            Change::Added(id) => format!("added:{id}"),
            Change::Modified(id) => format!("modified:{id}"),
            Change::Removed(id) => format!("removed:{id}"),
            Change::Renamed { id, .. } => format!("renamed:{id}"),
        });
        changes
    }

    #[test]
    fn external_new_file_produces_added() {
        let result = reconcile(&[], &[scanned("id1", "id1-note.md", "hash1")]);
        assert_eq!(result, vec![Change::Added("id1".to_string())]);
    }

    #[test]
    fn external_edit_produces_modified() {
        let result = reconcile(
            &[known("id1", "id1-note.md", "hash1")],
            &[scanned("id1", "id1-note.md", "hash2")],
        );
        assert_eq!(result, vec![Change::Modified("id1".to_string())]);
    }

    #[test]
    fn deletion_produces_removed() {
        let result = reconcile(&[known("id1", "id1-note.md", "hash1")], &[]);
        assert_eq!(result, vec![Change::Removed("id1".to_string())]);
    }

    #[test]
    fn atomic_rename_produces_renamed_not_modified() {
        let result = reconcile(
            &[known("id1", "id1-old-slug.md", "hash1")],
            &[scanned("id1", "id1-new-slug.md", "hash1")],
        );
        assert_eq!(
            result,
            vec![Change::Renamed { id: "id1".to_string(), new_path: "id1-new-slug.md".to_string() }]
        );
    }

    #[test]
    fn unchanged_file_produces_no_event() {
        let result = reconcile(
            &[known("id1", "id1-note.md", "hash1")],
            &[scanned("id1", "id1-note.md", "hash1")],
        );
        assert!(result.is_empty());
    }

    #[test]
    fn mixed_syncthing_burst_produces_add_modify_remove() {
        let known_state = vec![
            known("id-existing", "id-existing.md", "hash-old"),
            known("id-deleted", "id-deleted.md", "hash-del"),
        ];
        let scan = vec![
            scanned("id-new", "id-new.md", "hash-new"),
            scanned("id-existing", "id-existing.md", "hash-changed"),
        ];
        let result = sorted(reconcile(&known_state, &scan));
        assert_eq!(result, vec![
            Change::Added("id-new".to_string()),
            Change::Modified("id-existing".to_string()),
            Change::Removed("id-deleted".to_string()),
        ]);
    }

}
