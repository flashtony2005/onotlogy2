# 公司业务包 v1

## 目标

业务包是产品发布时的业务语义单元，不是另一套数据库。它把：

1. `ontology.odl`：公司业务概念、属性、动作、状态约束；
2. `bootstrap.json`：公司级设置、组织、岗位、默认客户等级和演示/初始事实；
3. `scenarios/`：业务验收场景；

装配成一个可校验、可安装、可审计的目录。

当前示例：`domain-packs/crm-retention/`。

## 安装流程

```text
domain-pack validate
  → 读取 manifest
  → 解析 ODL
  → 校验 bootstrap 的类型、字段和关系引用

domain-pack install
  → 创建 project（id = manifest.id）
  → 通过 WorldCommand 写入对象/关系
  → 每次写入带 project_id，并产生 ObjectCreated / RelationLinked 事件
  → 重复安装只跳过内容完全相同的对象/关系
```

服务启动时可以直接加载业务包：

```bash
oaip --db company.db serve --domain-pack domain-packs/crm-retention
```

`--domain-pack` 与 `--odl` 互斥。业务包优先于空白 ODL 入口，服务启动后运行时使用业务包的 ODL。

## manifest 与 Skill 关联

业务包可以在 `manifest.json` 中声明所需的操作 Skill：

```json
{
  "id": "lottery-analysis",
  "name": "彩票本体分析业务包",
  "version": "1.0.0",
  "ontology": "ontology.odl",
  "bootstrap": "bootstrap.json",
  "skills": ["business-pack-operations"]
}
```

`skills` 只是声明式关联。宿主从 Skill Registry 解析 Skill 状态，业务包页面据此展示关联关系；业务包不会通过前端分支绑定彩票代码，也不会把 Skill 实现复制进业务包。

## 网页管理 API

认证后的 Operator 及以上角色可以查看本地业务包：

```text
GET /api/v1/domain-packs
```

返回每个目录的发现、校验和安装状态，包括 `valid`、`installed`、`status`、`skill_ids`、本体类型数量以及 Bootstrap 对象/关系数量。无效业务包会作为 `invalid` 项返回并保留错误信息，不会阻断其它业务包展示。

只有 Developer 可以安装业务包：

```text
POST /api/v1/domain-packs/:id/install
```

安装复用 `DomainPack::install`，通过现有项目、本体、Bootstrap 和 `WorldCommand` 流程执行，并返回新增/跳过的对象与关系数量。重复安装是幂等的。当前网页端支持发现、校验状态查看和安装，不宣称支持热卸载、版本迁移或运行时插件热加载。

## 分层边界

| 层 | 内容 | 当前状态 |
|---|---|---|
| 本体定义 | Company、Customer、Contract、RiskSignal、RetentionTask、Approval 等概念 | 已支持 |
| 公司设置/初始事实 | 部门、员工、审批阈值、默认客户 | 已支持，作为对象/关系导入 |
| 对象动作 | `Customer.refund`、`Customer.retain`、任务生命周期 | 已由现有 ODL Action Engine 支持 |
| 派生事实 | 高流失风险、预警、自动任务 | 下一步实现 `derivations` 执行器 |
| dsh 执行 | 客户研究、报告生成、外部系统调用 | 由 Worker/Fabric 调用，不写进业务本体 |

## 版本策略

同一个 `manifest.id` 的业务包如果 ODL 内容发生变化，安装会拒绝覆盖，并提示使用新的业务包 id 发布新版本。这样可以避免在已有业务世界上静默改变概念语义；正式版本升级时再增加显式 migration/release 流程。

## 相关操作 Skill

业务包的实际操作流程（校验、安装、启动、彩票同步、分析、回测、排错和停用）统一记录在：

```text
skills/business-pack-operations/SKILL.md
```

架构说明和验收标准见：

```text
docs/03_架构设计/business-pack-operations-v1.md
```


## Phase 2.4：业务包运行能力

安装后的业务包可以通过宿主 HTTP API 管理运行状态，但这不等于已经启动后台调度器。运行配置作为 `DomainPackRuntime` 世界对象写入对应项目，并通过 `WorldCommand` 产生审计事件：

```text
id: domain-pack-runtime:<pack_id>
type: DomainPackRuntime
project_id: <pack_id>
```

配置字段包括：

- `enabled`：业务包启用/停用状态；
- `source_path`：CSV 数据源路径；
- `watch_enabled`、`poll_interval_seconds`：文件监测配置；
- `auto_sync`、`auto_analyze`、`auto_backtest`、`auto_review`：自动任务开关；
- `strategy_ids`：允许自动任务使用的策略代码，策略家族之间禁止混合评分。

API：

```text
GET  /api/v1/domain-packs/:id/runtime
PUT  /api/v1/domain-packs/:id/runtime
POST /api/v1/domain-packs/:id/enable
POST /api/v1/domain-packs/:id/disable
GET  /api/v1/domain-packs/:id/operations
GET  /api/v1/domain-packs/:id/strategies
```

Operator 可以读取运行配置，Developer 才能修改配置或启停业务包。未安装的业务包只返回默认配置，不能保存运行配置。

### Plugin 关联

业务包不直接绑定宿主插件实现。关联链为：

```text
manifest.skills → Skill capabilities → Plugin manifest capabilities
```

业务包列表因此可以展示匹配的 Plugin 和 capability。当前宿主的 `PluginCatalog` 仍在启动时静态装配，不支持插件热加载或热卸载。

### 策略注册表

策略注册表读取已安装项目中的 `AnalysisStrategy` 对象，并从同项目的 `AnalysisRun` 回测记录汇总指标。返回策略版本、参数、训练窗口、回测次数、平均命中、随机基线和 `publishable` 状态。没有回测证据的策略不能标记为可发布；`statistics`、`heuristic`、`metaphysics` 只做独立评估。

Phase 2.4 只完成配置、展示和审计持久化；文件监测、任务队列、自动分析和 Pi Agent 调度属于后续 Phase 2.5，不在本阶段伪装成已完成能力。
