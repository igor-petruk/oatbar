pub mod i3bar {
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Header {
        pub version: i32,
    }

    impl Default for Header {
        fn default() -> Self {
            Self { version: 1 }
        }
    }

    #[derive(Clone, Default, Debug, Serialize, Deserialize)]
    pub struct Block {
        pub full_text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub instance: Option<String>,
        #[serde(flatten)]
        pub other: BTreeMap<String, serde_json::Value>,
    }
}
