# 建表 SQL 与初始化脚本
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-04-004 |
| 所属模块 | WorkBuddy-DB |
| 版本 | V1.4 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
存放所有 DDL 与初始化 DML 脚本，作为库结构变更的唯一定义来源。

## 2. 业务/技术背景
由 table-design.md 衍生。

## 3. 核心规则 & 约束（重点）
- [强制] 所有表结构变更必须走此脚本，禁止直接在线改库。
- [强制] 脚本按版本分段，可重复执行（幂等）或带版本标记。
- [建议] 初始化数据区分「环境无关基础数据」与「环境相关种子数据」。

## 4. 详细设计/内容
### 4.1 建表脚本（V1.3.0）
```sql
-- 系统账户（内部账号，数值自增主键）
CREATE TABLE wb_user_account (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,  -- 主键
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  username    TEXT NOT NULL  -- 用户名
);

CREATE INDEX idx_create_time ON wb_user_account (create_time);

-- 本体对象实例（Everything is Object：每类业务对象统一存此表，禁止按业务建独立表；V0.1 引入）
CREATE TABLE wb_ontology_object (
  id          TEXT PRIMARY KEY,  -- 语义对象ID（如 customer_001）
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  type        TEXT NOT NULL,  -- 对象类型（对应 ODL object 名，如 Customer）
  data        TEXT NOT NULL,  -- 对象数据 JSON（由 ObjectDef 驱动，禁止 match 硬编码）
  version     INTEGER NOT NULL DEFAULT 1  -- 乐观锁版本号，update 自增
);

CREATE INDEX idx_obj_type   ON wb_ontology_object (type);
CREATE INDEX idx_obj_create ON wb_ontology_object (create_time);

-- 关系边（世界连接，图查询基础；V0.2 引入）
CREATE TABLE wb_relation_edge (
  id          TEXT PRIMARY KEY,  -- 语义边ID（如 e_001）
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  source      TEXT NOT NULL,  -- 源对象ID（FK → wb_ontology_object.id）
  relation    TEXT NOT NULL,  -- 关系名（如 purchased / owns）
  target      TEXT NOT NULL,  -- 目标对象ID（FK → wb_ontology_object.id）
  data        TEXT  -- 关系附载 JSON（可选）
);

CREATE INDEX idx_edge_source ON wb_relation_edge (source);
CREATE INDEX idx_edge_target ON wb_relation_edge (target);
CREATE INDEX idx_edge_rel    ON wb_relation_edge (relation);

-- 统一事件轨迹：world 生命周期事件 + cognitive 认知事件（见 agent-observation-design.md 4.3 / 4.7）
CREATE TABLE wb_trace_event (
  id          TEXT PRIMARY KEY,  -- 语义轨迹ID（如 tr_abc123），与 API DTO.id 直接对应
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 记录创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 记录更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  kind        TEXT NOT NULL,  -- 事件大类：world | cognitive（枚举见 data-dict）
  run_id      TEXT,  -- 认知运行ID（kind=cognitive 时必填）
  agent_id    TEXT,  -- Agent ID（kind=cognitive 时必填）
  event_type  TEXT NOT NULL,  -- 事件类型（枚举见 data-dict）
  object_refs TEXT,  -- 绑定的 Ontology 对象引用，JSON 数组，可解释性根基；反查经 wb_ontology_object.id
  reasoning   TEXT,  -- 推理过程（kind=cognitive 专有）
  input       TEXT,  -- 输入载荷 JSON
  output      TEXT,  -- 输出载荷 JSON
  confidence  REAL,  -- 置信度 0~1（kind=cognitive 专有）
  timestamp   TEXT NOT NULL  -- 事件发生时间（业务时间）
);

CREATE INDEX idx_trace_run    ON wb_trace_event (run_id);
CREATE INDEX idx_trace_agent  ON wb_trace_event (agent_id);
CREATE INDEX idx_trace_type   ON wb_trace_event (event_type);
CREATE INDEX idx_trace_time   ON wb_trace_event (timestamp);
CREATE INDEX idx_create_time  ON wb_trace_event (create_time);
```

### 4.2 初始化脚本
```sql
-- 环境无关基础数据
```


## 5. 风险点 & 注意事项
- 脚本不可幂等会在重复执行时失败，需评估。

## 6. 关联文档
- [../04_数据库设计/table-design.md](../04_数据库设计/table-design.md)
- [../08_部署运维/deploy-steps.md](../08_部署运维/deploy-steps.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 新增 wb_trace_event 建表 SQL（统一 TraceEvent 模型，kind 区分 world/cognitive） |
| V1.2 | lushaohui | 2026-08-11 | DDL 方言统一为 Turso/SQLite（INTEGER/TEXT/REAL + AUTOINCREMENT，列注释改 -- 行注释） |
| V1.3 | lushaohui | 2026-08-11 | 新增核心表 wb_ontology_object / wb_relation_edge（SQLite 方言，TEXT 语义ID）；wb_trace_event.id 改为 TEXT 与 DTO 对齐；补齐 idx_create_time 索引 |
| V1.4 | lushaohui | 2026-08-11 | codegen(canonical_schema) 与 LibSQLStorage 字段/索引对齐本权威 DDL：object 表列 type、edge 表 source/relation/target；索引补全 idx_obj_create / idx_edge_source / idx_edge_target / idx_edge_rel（与文档一致），移除代码侧多余的 idx_rel_* |

