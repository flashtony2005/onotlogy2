//! Lineage —— Skill 版本与来源（可追溯：source/origin/parent/derived_from）。

/// 谱系（Lineage Graph 的节点属性）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Lineage {
    pub parent: Option<String>,
    pub derived_from: Option<String>,
    pub version: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub origin: Option<String>,
}

impl Lineage {
    pub fn new(version: &str) -> Self {
        Self {
            parent: None,
            derived_from: None,
            version: version.to_string(),
            author: None,
            license: None,
            origin: None,
        }
    }
}
