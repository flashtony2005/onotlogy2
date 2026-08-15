//! RawSkill —— 第三方能力资产的原始快照（P1：Raw 不修改）。

use std::hash::{Hash, Hasher};

/// 来源格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Markdown,
    Txt,
    Json,
    Yaml,
    Prompt,
    Mcp,
    Workflow,
}

/// 第三方资产只读快照。
///
/// **Raw 不修改**：任何 AI 转换都是派生资产；本结构不提供修改 API。
#[derive(Debug, Clone)]
pub struct RawSkill {
    pub id: String,
    pub name: String,
    pub source_type: SourceType,
    pub source_uri: Option<String>,
    /// 内容指纹（确定性：同内容同 hash，不同内容不同 hash）。
    pub content_hash: String,
    /// 原始内容（快照，永不变更）。
    pub content: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub version: Option<String>,
}

impl RawSkill {
    /// 导入：从内容生成只读快照（hash 由内容派生）。
    pub fn import(name: &str, content: &str) -> Self {
        Self {
            id: name.to_string(),
            name: name.to_string(),
            source_type: SourceType::Markdown,
            source_uri: None,
            content_hash: Self::content_hash_of(content),
            content: content.to_string(),
            author: None,
            license: None,
            version: None,
        }
    }

    /// 内容指纹（FNV 1a 64——确定性、无外部依赖）。
    pub fn content_hash_of(content: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}
