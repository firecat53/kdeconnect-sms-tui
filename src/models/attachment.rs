use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub part_id: i64,
    pub mime_type: String,
    pub unique_identifier: String,
    /// Local cached path once downloaded
    pub cached_path: Option<PathBuf>,
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }

    pub fn is_cached(&self) -> bool {
        self.cached_path.as_ref().is_some_and(|p| p.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image() {
        let att = Attachment {
            part_id: 1,
            mime_type: "image/jpeg".into(),
            unique_identifier: "abc".into(),
            cached_path: None,
        };
        assert!(att.is_image());
    }

    #[test]
    fn test_not_image() {
        let att = Attachment {
            part_id: 1,
            mime_type: "text/plain".into(),
            unique_identifier: "abc".into(),
            cached_path: None,
        };
        assert!(!att.is_image());
    }

    #[test]
    fn test_not_cached() {
        let att = Attachment {
            part_id: 1,
            mime_type: "image/png".into(),
            unique_identifier: "abc".into(),
            cached_path: None,
        };
        assert!(!att.is_cached());
    }
}
