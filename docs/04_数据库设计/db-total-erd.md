# 数据库 ER 图说明
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-04-001 |
| 所属模块 | WorkBuddy-DB |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
呈现全局 ER 图与实体关系说明。

## 2. 业务/技术背景
依赖 module-split.md 与 table-design.md。

## 3. 核心规则 & 约束（重点）
- [强制] 必须含全局 ER 图（优先用内联 Mermaid `erDiagram` 渲染，零二进制依赖；如需位图放 /docs/assets/ 并相对路径引用）。
- [强制] 标注实体间基数关系（1:1 / 1:N / M:N）。

## 4. 详细设计/内容
### 4.1 ER 总图
```mermaid
erDiagram
    WB_ONTOLOGY_OBJECT ||--o{ WB_RELATION_EDGE : "source/target"
    WB_ONTOLOGY_OBJECT ||--o{ WB_TRACE_EVENT : "object_refs"
    WB_USER_ACCOUNT ||--o{ WB_TRACE_EVENT : "触发运行(可选)"

    WB_ONTOLOGY_OBJECT {
        TEXT id PK "语义对象ID 如 customer_001"
        TEXT type "对象类型 如 Customer"
        TEXT data "对象数据 JSON"
        INTEGER version "乐观锁 自增"
        TEXT create_time
        TEXT update_time
        INTEGER is_delete
    }
    WB_RELATION_EDGE {
        TEXT id PK "语义边ID 如 e_001"
        TEXT source FK "源对象ID"
        TEXT relation "关系名 如 purchased"
        TEXT target FK "目标对象ID"
        TEXT data "附载 JSON"
        TEXT create_time
        TEXT update_time
        INTEGER is_delete
    }
    WB_TRACE_EVENT {
        TEXT id PK "语义轨迹ID 如 tr_abc123"
        TEXT kind "world/cognitive"
        TEXT run_id "认知运行ID"
        TEXT agent_id "Agent ID"
        TEXT event_type "事件类型"
        TEXT object_refs "对象ID数组 JSON"
        TEXT reasoning "推理过程"
        TEXT input "输入载荷"
        TEXT output "输出载荷"
        REAL confidence "0~1"
        TEXT timestamp "业务时间"
        TEXT create_time
        TEXT update_time
        INTEGER is_delete
    }
    WB_USER_ACCOUNT {
        INTEGER id PK "自增"
        TEXT username "用户名"
        TEXT create_time
        TEXT update_time
        INTEGER is_delete
    }
```

### 4.2 实体关系说明
| 实体 A | 关系 | 实体 B | 说明 |
|--------|------|--------|------|
| wb_ontology_object | 1:N | wb_relation_edge | 一个对象可作 source / target 出现在多条边，构成世界图 |
| wb_ontology_object | 1:N | wb_trace_event | 对象生命周期事件经 `object_refs` 绑定，可解释性根基 |
| wb_user_account | 1:N | wb_trace_event | 触发 Agent 运行的用户（可选，经 `run_id` 关联） |


## 5. 风险点 & 注意事项
- ER 图与表结构不一致需立即同步。

## 6. 关联文档
- [../04_数据库设计/table-design.md](../04_数据库设计/table-design.md)
- [../04_数据库设计/data-dict.md](../04_数据库设计/data-dict.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 4.1 改为内联 Mermaid erDiagram（去不存在的 db-erd.png 绝对路径）；4.2 填充实体关系表 |

