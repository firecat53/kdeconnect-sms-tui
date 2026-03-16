use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub reachable: bool,
    pub paired: bool,
}

impl Device {
    pub fn is_available(&self) -> bool {
        self.reachable && self.paired
    }
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.is_available() {
            "available"
        } else if self.paired {
            "paired (unreachable)"
        } else {
            "not paired"
        };
        write!(f, "{} [{}]", self.name, status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_available() {
        let dev = Device {
            id: "abc123".into(),
            name: "My Phone".into(),
            reachable: true,
            paired: true,
        };
        assert!(dev.is_available());
    }

    #[test]
    fn test_device_unreachable() {
        let dev = Device {
            id: "abc123".into(),
            name: "My Phone".into(),
            reachable: false,
            paired: true,
        };
        assert!(!dev.is_available());
    }

    #[test]
    fn test_device_display() {
        let dev = Device {
            id: "abc123".into(),
            name: "Pixel".into(),
            reachable: true,
            paired: true,
        };
        assert_eq!(dev.to_string(), "Pixel [available]");
    }
}
