//! 事实引用（EventRef）—— Kernel 事件轨的最小寻址（M4：从 evaluation 下沉，
//! 消除 kernel → evaluation 的上层依赖；`EventRef::of` 在组合层补 impl）。

/// 事件引用（seq + 类型）：证据链/投影的最小事实锚点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventRef {
    pub seq: u64,
    pub event_type: String,
}

impl EventRef {
    pub fn new(seq: u64, event_type: impl Into<String>) -> Self {
        Self {
            seq,
            event_type: event_type.into(),
        }
    }
}
