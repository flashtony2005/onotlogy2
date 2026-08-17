//! Projection（Phase 5）—— World Event → Kernel World State 的确定性投影层。
//!
//! 契约：docs/03_架构设计/projection-v1.md（P1–P7）。
//!
//! 核心命题：
//! - **Projection 是 Event Log 的纯函数**（P1）：同一事件流 → 同一 Kernel 关系元组集。
//! - **幂等**（P2）：同 id 事件只应用一次（applied 水位）；重放/重复调用状态不变。
//! - **确定性**（P4）：expire 时刻 = 事件时间戳，不用墙钟；事件微秒轴经
//!   `normalize_ts` 纯除法归一化到 Kernel 秒轴（同一事件永远映射同一秒）。
//! - **Kernel 不接受不可追溯的 World Mutation**（P7）：投影世界只由事件重建；
//!   裸写（storage.create 不带事件）的对象在投影世界里不存在。
//!
//! 写入路径（P5/P6，见契约）：对象写与事件写同事务；Effect 提交在提交点即转
//! world 事件落库——本层只负责「事件 → Kernel」，不产生事件（P2 反向：投影不写日志）。

use std::collections::{HashMap, HashSet};

use crate::kernel::relation::Relation;
use crate::kernel::{ContextId, Kernel, KernelError};
use crate::storage::{LibSQLStorage, TraceQuery};
use crate::trace::{TraceEvent, TraceKind};

/// 投影错误（Kernel 规则错误 + 存储读取错误）。
#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("投影内核错误: {0}")]
    Kernel(#[from] KernelError),
    #[error("投影存储错误: {0}")]
    Storage(String),
}

/// 世界状态快照元组（比较/测试用；忽略 RelationId 以支持跨 Kernel 比较）。
pub type StateTuple = (String, String, String, i64, Option<i64>);

/// 确定性投影器：世界事件 → Kernel 关系。
///
/// `applied` 是已应用事件 id 的水位（P2 幂等）；领域规则经 `with_domain_rule`
/// 注册（Metadata 驱动：event_type → predicate，事件载荷 `{"to": value}`）。
pub struct Projection {
    ctx: ContextId,
    domain_rules: HashMap<String, String>,
    applied: HashSet<String>,
}

impl Projection {
    pub fn new(ctx: ContextId) -> Self {
        Self {
            ctx,
            domain_rules: HashMap::new(),
            applied: HashSet::new(),
        }
    }

    /// 注册领域规则：`event_type` → `predicate`（事件 refs=[subject]，input={"to": value}）。
    pub fn with_domain_rule(
        mut self,
        event_type: impl Into<String>,
        predicate: impl Into<String>,
    ) -> Self {
        self.domain_rules
            .insert(event_type.into(), predicate.into());
        self
    }

    /// 已应用事件数（水位）。
    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    /// 世界状态快照（按元组排序；P1 重放比较的依据）。
    pub fn state_snapshot(&self, kernel: &Kernel) -> Vec<StateTuple> {
        let mut v: Vec<StateTuple> = kernel
            .relations()
            .filter(|r| r.context == self.ctx)
            .map(|r| {
                (
                    r.source.clone(),
                    r.predicate.clone(),
                    r.target.clone(),
                    r.valid_from,
                    r.valid_until,
                )
            })
            .collect();
        v.sort();
        v
    }

    /// 应用一条世界事件（P2：同 id 只应用一次；未知类型 = 无副作用但计入水位）。
    pub fn apply_event(
        &mut self,
        kernel: &mut Kernel,
        ev: &TraceEvent,
    ) -> Result<(), ProjectionError> {
        if self.applied.contains(&ev.id) {
            return Ok(());
        }
        let ts = normalize_ts(ev);
        match ev.event_type.as_str() {
            "ObjectCreated" => self.apply_created(kernel, ev, ts)?,
            "ObjectUpdated" => self.apply_updated(kernel, ev, ts)?,
            "ObjectDeleted" => self.apply_deleted(kernel, ev, ts)?,
            "RelationLinked" => {
                if let (Some(s), Some(t)) = (ev.object_refs.first(), ev.object_refs.get(1)) {
                    let edge = ev
                        .input
                        .as_ref()
                        .and_then(|v| v.get("edge"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("related");
                    kernel.relate(Relation::new(self.ctx, s.clone(), edge, t.clone(), ts))?;
                }
            }
            "RelationUnlinked" => self.expire_matching(kernel, ev, ts)?,
            ty => {
                // 领域规则（注册制）：(refs[0], predicate, input.to)
                if let Some(pred) = self.domain_rules.get(ty)
                    && let Some(s) = ev.object_refs.first()
                {
                    let to = ev
                        .input
                        .as_ref()
                        .and_then(|v| v.get("to"))
                        .map(stringify)
                        .unwrap_or_default();
                    kernel.relate(Relation::new(self.ctx, s.clone(), pred.clone(), to, ts))?;
                }
            }
        }
        self.applied.insert(ev.id.clone());
        Ok(())
    }

    /// 全量投影一批事件（replay 语义：新 Projection 实例 = 全新 Kernel 重建）。
    /// 同一实例重复调用：已应用事件跳过（P2）。
    pub fn project_events(
        &mut self,
        kernel: &mut Kernel,
        events: &[TraceEvent],
    ) -> Result<usize, ProjectionError> {
        let mut n = 0;
        for ev in events {
            if self.applied.contains(&ev.id) {
                continue;
            }
            self.apply_event(kernel, ev)?;
            n += 1;
        }
        Ok(n)
    }

    /// 从 Storage 事件日志增量投影（P5.5 策略：per-run 全量；watermark 由 applied 承担，
    /// 跨 run 常驻 Kernel + 查询水位优化属后续里程碑）。诚实边界：TraceQuery limit 1000。
    pub async fn project_from_storage(
        &mut self,
        kernel: &mut Kernel,
        storage: &LibSQLStorage,
    ) -> Result<usize, ProjectionError> {
        self.project_from_storage_for(kernel, storage, None).await
    }

    /// 按项目过滤投影（Phase 8：世界是项目范围的——只投影该项目的事件；
    /// None = 全量/全局世界，兼容 P5.4 per-run 策略）。
    pub async fn project_from_storage_for(
        &mut self,
        kernel: &mut Kernel,
        storage: &LibSQLStorage,
        project_id: Option<String>,
    ) -> Result<usize, ProjectionError> {
        let events = storage
            .list_events(TraceQuery {
                project_id,
                limit: 1000,
                ..Default::default()
            })
            .await
            .map_err(|e| ProjectionError::Storage(e.to_string()))?;
        let mut world: Vec<TraceEvent> = events
            .into_iter()
            .filter(|e| {
                // World 事件全量投影；fabric.* 通信事件（Cognitive）经领域规则
                // 进入世界状态（通信黑匣子 → World State，facade-v1.md §3）
                e.kind == TraceKind::World
                    || (e.kind == TraceKind::Cognitive && e.event_type.starts_with("fabric."))
            })
            .collect();
        // 确定性序：(timestamp, id)（P4）
        world.sort_by(|a, b| {
            (a.timestamp.clone(), a.id.clone()).cmp(&(b.timestamp.clone(), b.id.clone()))
        });
        self.project_events(kernel, &world)
    }

    // ── 规则实现（P5 契约第 3 节）──────────────────────────────────────

    /// ObjectCreated：relate (id,"type",T) + 每字段 (id, field, value)。
    fn apply_created(
        &mut self,
        kernel: &mut Kernel,
        ev: &TraceEvent,
        ts: i64,
    ) -> Result<(), ProjectionError> {
        let Some(id) = ev.object_refs.first().cloned() else {
            return Ok(());
        };
        // 兼容：手动 emit / 历史坏事件可能缺 input——无法还原对象，跳过
        //（计入水位，世界不含该实体；新事件由契约保证带 input）
        let Some(input) = ev.input.as_ref() else {
            return Ok(());
        };
        let ty = input
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("Object");
        kernel.relate(Relation::new(self.ctx, id.clone(), "type", ty, ts))?;
        if let Some(data) = input.get("data").and_then(|v| v.as_object()) {
            for (k, v) in data {
                kernel.relate(Relation::new(self.ctx, id.clone(), k, stringify(v), ts))?;
            }
        }
        Ok(())
    }

    /// ObjectUpdated：expire 该 id 除 type 外的 current 关系（at=事件时刻），再 relate data。
    fn apply_updated(
        &mut self,
        kernel: &mut Kernel,
        ev: &TraceEvent,
        ts: i64,
    ) -> Result<(), ProjectionError> {
        let Some(id) = ev.object_refs.first().cloned() else {
            return Ok(());
        };
        let input = ev
            .input
            .as_ref()
            .ok_or_else(|| ProjectionError::Storage("ObjectUpdated 缺 input".into()))?;
        // 契约：data 不含 type 时保持原 type 关系（expire 除 type 外的 current 关系）
        self.expire_where(
            kernel,
            |r| (r.source == id || r.target == id) && r.predicate != "type",
            ts,
        )?;
        if let Some(data) = input.get("data").and_then(|v| v.as_object()) {
            for (k, v) in data {
                kernel.relate(Relation::new(self.ctx, id.clone(), k, stringify(v), ts))?;
            }
        }
        Ok(())
    }

    /// ObjectDeleted：expire 该 id 全部 current 关系（source 与 target 两侧）。
    fn apply_deleted(
        &mut self,
        kernel: &mut Kernel,
        ev: &TraceEvent,
        ts: i64,
    ) -> Result<(), ProjectionError> {
        let Some(id) = ev.object_refs.first().cloned() else {
            return Ok(());
        };
        self.expire_where(kernel, |r| r.source == id || r.target == id, ts)?;
        Ok(())
    }

    /// RelationUnlinked：expire 匹配 (refs[0], edge, refs[1]) 的 current 边。
    fn expire_matching(
        &mut self,
        kernel: &mut Kernel,
        ev: &TraceEvent,
        ts: i64,
    ) -> Result<(), ProjectionError> {
        let (Some(s), Some(t)) = (ev.object_refs.first(), ev.object_refs.get(1)) else {
            return Ok(());
        };
        let edge = ev
            .input
            .as_ref()
            .and_then(|v| v.get("edge"))
            .and_then(|v| v.as_str())
            .unwrap_or("related");
        self.expire_where(
            kernel,
            |r| r.source == *s && r.predicate == edge && r.target == *t,
            ts,
        )?;
        Ok(())
    }

    /// 在事件时刻 expire 匹配谓词的 current 关系（P4：时刻来自事件，不用墙钟）。
    fn expire_where<F: Fn(&crate::kernel::relation::Relation) -> bool>(
        &mut self,
        kernel: &mut Kernel,
        f: F,
        ts: i64,
    ) -> Result<(), ProjectionError> {
        let ids: Vec<_> = kernel
            .relations()
            .filter(|r| r.context == self.ctx)
            .filter(|r| r.valid_until.is_none() || *r.valid_until.as_ref().unwrap() > ts)
            .filter(|r| f(r))
            .map(|r| r.id)
            .collect();
        for id in ids {
            kernel.expire_relation(id, ts)?;
        }
        Ok(())
    }
}

/// 字段值 stringify（契约已知限制：标量文本化，数值语义由 EntityView 还原）。
fn stringify(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// 事件时刻 → Kernel 时钟轴（P4 确定性）。
///
/// Trace 时间轴是微秒（oaip-trace::now_ts，G1），Kernel 时钟是秒（now_secs）。
/// 投影把事件时刻 floor 归一化到 Kernel 秒轴：纯除法、无墙钟参与——同一事件
/// 永远映射同一秒（重放确定性）；`valid_from` 永不落入未来（observe 的
/// `is_current(now)` 恒成立）。已知限制：同秒内事件序坍缩（项目粒度可接受，
/// 语义重建由事件序保证——watermark/常驻 Kernel 里程碑再收紧）。
fn normalize_ts(ev: &TraceEvent) -> i64 {
    ev.timestamp.parse::<i64>().unwrap_or(0) / 1_000_000
}
