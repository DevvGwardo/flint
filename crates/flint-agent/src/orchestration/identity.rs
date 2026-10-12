use super::{EngineError, EngineErrorCode};
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! identity {
    ($name:ident, $validate:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self, EngineError> {
                if ($validate)(value) {
                    Ok(Self(value.into()))
                } else {
                    Err(EngineError::new(
                        EngineErrorCode::InvalidCommand,
                        concat!("Invalid ", stringify!($name), "."),
                    ))
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl std::str::FromStr for $name {
            type Err = EngineError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
identity!(SessionId, |s: &str| {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
});
identity!(ChildId, |s: &str| s.strip_prefix("agent-").is_some_and(
    |n| n
        .parse::<u64>()
        .is_ok_and(|id| id > 0 && n == id.to_string())
));
fn uuid_valid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|id| {
        id.get_version_num() == 4
            && id.get_variant() == uuid::Variant::RFC4122
            && id.to_string() == s
    })
}
identity!(CommandId, uuid_valid);
identity!(RunId, uuid_valid);
impl CommandId {
    pub fn fresh() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}
