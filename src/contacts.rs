use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use tracing::{debug, warn};

/// Maps phone numbers to contact display names.
#[derive(Debug, Clone)]
pub struct ContactStore {
    /// Normalized phone number → display name
    contacts: HashMap<String, String>,
}

impl ContactStore {
    /// Load contacts from the kdeconnect vCard sync directory.
    pub fn load() -> Result<Self> {
        let vcard_dir = Self::vcard_dir();
        if !vcard_dir.exists() {
            debug!("vCard directory does not exist: {:?}", vcard_dir);
            return Ok(Self {
                contacts: HashMap::new(),
            });
        }
        Self::load_from_dir(&vcard_dir)
    }

    /// Load contacts from a specific directory (useful for testing).
    ///
    /// Recurses into subdirectories, since KDE Connect stores vCards in
    /// per-device subdirectories under `~/.local/share/kpeoplevcard/`.
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let mut contacts = HashMap::new();
        Self::load_vcards_recursive(dir, &mut contacts);
        debug!("Loaded {} contacts from {:?}", contacts.len(), dir);
        Ok(Self { contacts })
    }

    fn load_vcards_recursive(dir: &Path, contacts: &mut HashMap<String, String>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to read directory {:?}: {}", dir, e);
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::load_vcards_recursive(&path, contacts);
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "vcf" || ext == "vcard" {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        parse_vcard_contacts(&content, contacts);
                    }
                    Err(e) => {
                        warn!("Failed to read vCard {:?}: {}", path, e);
                    }
                }
            }
        }
    }

    /// Look up a display name for a phone number.
    ///
    /// Tries an exact normalized match first, then falls back to suffix
    /// matching (last 10 digits) to handle country-code mismatches — e.g.
    /// vCard stores `+15551234567` but kdeconnect sends `5551234567`.
    pub fn lookup(&self, phone: &str) -> Option<&str> {
        let normalized = normalize_phone(phone);

        // Exact match
        if let Some(name) = self.contacts.get(&normalized) {
            return Some(name.as_str());
        }

        // Suffix match: compare last 10 digits (covers US/CA numbers without
        // country code and most international numbers).
        let suffix = digit_suffix(&normalized, 10);
        if suffix.len() >= 7 {
            for (stored, name) in &self.contacts {
                if digit_suffix(stored, 10) == suffix {
                    debug!(
                        "Contact suffix match: '{}' matched stored '{}' -> '{}'",
                        phone, stored, name
                    );
                    return Some(name.as_str());
                }
            }
        }

        debug!(
            "No contact found for '{}' (normalized: '{}', {} contacts loaded)",
            phone,
            normalized,
            self.contacts.len()
        );
        None
    }

    /// Get display name or fall back to the phone number.
    pub fn display_name(&self, phone: &str) -> String {
        self.lookup(phone)
            .unwrap_or(phone)
            .to_string()
    }

    pub fn len(&self) -> usize {
        self.contacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    fn vcard_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("kpeoplevcard")
    }
}

/// Parse vCard file content and insert name/phone mappings.
fn parse_vcard_contacts(content: &str, contacts: &mut HashMap<String, String>) {
    let mut current_name: Option<String> = None;
    let mut current_phones: Vec<String> = Vec::new();
    let mut in_vcard = false;

    for line in content.lines() {
        let line = line.trim();

        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            in_vcard = true;
            current_name = None;
            current_phones.clear();
            continue;
        }

        if line.eq_ignore_ascii_case("END:VCARD") {
            if let Some(ref name) = current_name {
                for phone in &current_phones {
                    let normalized = normalize_phone(phone);
                    if !normalized.is_empty() {
                        contacts.insert(normalized, name.clone());
                    }
                }
            }
            in_vcard = false;
            continue;
        }

        if !in_vcard {
            continue;
        }

        // FN (formatted name) — preferred over N
        if let Some(value) = line.strip_prefix("FN:") {
            let name = value.trim();
            if !name.is_empty() {
                current_name = Some(name.to_string());
            }
        } else if line.starts_with("FN;") {
            // FN with parameters like FN;CHARSET=UTF-8:Name
            if let Some(value) = line.split(':').nth(1) {
                let name = value.trim();
                if !name.is_empty() {
                    current_name = Some(name.to_string());
                }
            }
        }

        // TEL lines: TEL:+1234, TEL;TYPE=CELL:+1234, etc.
        if line.starts_with("TEL") {
            if let Some(value) = line.split(':').last() {
                let phone = value.trim();
                if !phone.is_empty() {
                    current_phones.push(phone.to_string());
                }
            }
        }
    }
}

/// Normalize a phone number for consistent lookup.
/// Strips spaces, dashes, parens, and leading country code variations.
pub fn normalize_phone(phone: &str) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    // Keep the + prefix if present, strip everything else
    digits
}

/// Return the last `n` digits of a phone string (ignoring `+`).
fn digit_suffix(phone: &str, n: usize) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() <= n {
        digits
    } else {
        digits[digits.len() - n..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    const SAMPLE_VCARD: &str = "\
BEGIN:VCARD
VERSION:3.0
FN:Alice Smith
TEL;TYPE=CELL:+15551234567
TEL;TYPE=HOME:+15559876543
END:VCARD
BEGIN:VCARD
VERSION:3.0
FN:Bob Jones
TEL:+442071234567
END:VCARD
";

    #[test]
    fn test_parse_vcard_contacts() {
        let mut contacts = HashMap::new();
        parse_vcard_contacts(SAMPLE_VCARD, &mut contacts);
        assert_eq!(contacts.len(), 3);
        assert_eq!(contacts.get("+15551234567").unwrap(), "Alice Smith");
        assert_eq!(contacts.get("+15559876543").unwrap(), "Alice Smith");
        assert_eq!(contacts.get("+442071234567").unwrap(), "Bob Jones");
    }

    #[test]
    fn test_normalize_phone() {
        assert_eq!(normalize_phone("+1 (555) 123-4567"), "+15551234567");
        assert_eq!(normalize_phone("555-123-4567"), "5551234567");
        assert_eq!(normalize_phone("+44 20 7123 4567"), "+442071234567");
    }

    #[test]
    fn test_load_from_dir() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("contact1.vcf");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(SAMPLE_VCARD.as_bytes()).unwrap();

        let store = ContactStore::load_from_dir(dir.path()).unwrap();
        assert_eq!(store.len(), 3);
        assert_eq!(store.lookup("+15551234567"), Some("Alice Smith"));
        assert_eq!(store.display_name("+15551234567"), "Alice Smith");
        assert_eq!(store.display_name("+19999999999"), "+19999999999");
    }

    #[test]
    fn test_empty_dir() {
        let dir = TempDir::new().unwrap();
        let store = ContactStore::load_from_dir(dir.path()).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn test_vcard_with_params() {
        let vcard = "\
BEGIN:VCARD
VERSION:3.0
FN;CHARSET=UTF-8:Ñoño García
TEL;TYPE=CELL:+34612345678
END:VCARD
";
        let mut contacts = HashMap::new();
        parse_vcard_contacts(vcard, &mut contacts);
        assert_eq!(contacts.get("+34612345678").unwrap(), "Ñoño García");
    }

    #[test]
    fn test_load_from_subdirectories() {
        let dir = TempDir::new().unwrap();
        // Simulate per-device subdirectory structure
        let device_dir = dir.path().join("kdeconnect_abc123");
        fs::create_dir(&device_dir).unwrap();
        let path = device_dir.join("contact1.vcf");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(SAMPLE_VCARD.as_bytes()).unwrap();

        let store = ContactStore::load_from_dir(dir.path()).unwrap();
        assert_eq!(store.len(), 3);
        assert_eq!(store.lookup("+15551234567"), Some("Alice Smith"));
    }

    #[test]
    fn test_lookup_suffix_matching() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("contact1.vcf");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(SAMPLE_VCARD.as_bytes()).unwrap();

        let store = ContactStore::load_from_dir(dir.path()).unwrap();

        // Exact match
        assert_eq!(store.lookup("+15551234567"), Some("Alice Smith"));

        // Missing country code — should still match via suffix
        assert_eq!(store.lookup("5551234567"), Some("Alice Smith"));

        // With formatting — should still match
        assert_eq!(store.lookup("(555) 123-4567"), Some("Alice Smith"));

        // UK number without + prefix
        assert_eq!(store.lookup("442071234567"), Some("Bob Jones"));

        // No match at all
        assert_eq!(store.lookup("9999999999"), None);
    }

    #[test]
    fn test_digit_suffix() {
        assert_eq!(digit_suffix("+15551234567", 10), "5551234567");
        assert_eq!(digit_suffix("5551234567", 10), "5551234567");
        assert_eq!(digit_suffix("+442071234567", 10), "2071234567");
        assert_eq!(digit_suffix("123", 10), "123");
    }

    #[test]
    fn test_vcard_no_phone() {
        let vcard = "\
BEGIN:VCARD
VERSION:3.0
FN:No Phone Person
EMAIL:nophone@example.com
END:VCARD
";
        let mut contacts = HashMap::new();
        parse_vcard_contacts(vcard, &mut contacts);
        assert!(contacts.is_empty());
    }
}
