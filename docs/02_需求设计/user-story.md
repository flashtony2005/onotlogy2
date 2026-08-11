# 用户故事与场景
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-02-002 |
| 所属模块 | WorkBuddy-Requirement |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
以用户视角描述使用场景，补全需求的行为上下文。

## 2. 业务/技术背景
依赖 requirement-all.md 提供需求清单。

## 3. 核心规则 & 约束（重点）
- [强制] 用户故事采用 As a / I want / So that 三段式。
- [建议] 每个故事附 1-2 个主流程 + 异常流程场景。

## 4. 详细设计/内容
### 4.1 用户故事模板
> 作为 **<角色>**，我希望 **<能力>**，以便 **<价值>**。

### 4.2 场景示例

**US-1｜开发者定义并查询一个对象（V0.1）**
> 作为 **后端开发者**，我希望 **用 ODL 定义 Customer 并查询其实例**，以便 **让对象拥有生命而不写任何业务 CRUD 代码**。

- 前置条件：已安装 `oaip-cli`，空 Turso 库。
- 主流程：
  1. 编写 `customer.odl`（`object Customer: properties: name/level`）。
  2. `oaip compile customer.odl` → 生成 `Customer.rs` / `schema.sql` / `ontology.json`。
  3. `oaip run` 启动内核；`oaip object create Customer '{...}'` 写入 `customer_001`。
  4. `oaip object list Customer` 返回 `001 Apple VIP / 002 Tesla GOLD`。
- 异常流程：ODL 语法错误 → 编译报 `OdlError`（TC-001-b），不生成代码。
- 验收要点：REQ-001~004 通过；`oaip event list` 可见 `Customer_001 created`（world 事件，TC-004）。

**US-2｜运营观察 Agent 推理并审批动作（V0.3 / MVP）**
> 作为 **客户运营（Operator）**，我希望 **看到 Agent 如何基于客户世界模型得出挽留方案**，以便 **在动作执行前理解并审批**。

- 前置条件：V0.3 已接入 Pi（Cognitive Engine）+ 三层 Observation UI。
- 主流程：
  1. `oaip agent run "reduce churn"` 触发运行 `run_001`。
  2. Operator View 显示 Pipeline：`Goal ✓ → Intent ✓ → OntologyQuery ✓ → Reasoning ▶ → Policy ○ → Action ○`。
  3. Developer View 展示 Reasoning Evidence：`Customer_001 Risk 0.72`，依据 `PaymentLate ×3 / Order ↓30%`。
  4. 动作 `CreateTask` 触发 `PolicyCheck → manager required → ALLOW`，Operator 在 UI 审批通过。
- 异常流程：Pi 未就绪 → 触发即返 WB2004，连接不悬挂。
- 验收要点：REQ-009~011 通过；每步 `object_refs` 非空（WB2006 卡点）；Trace 经 WebSocket 实时推送。

**US-3｜管理者看一眼全局风险（V0.3 / MVP）**
> 作为 **业务负责人（Executive）**，我希望 **在 Observation UI 看到 Mission / Progress / Risk 摘要**，以便 **不陷入推理细节也能掌控 Agent**。

- 前置条件：同 US-2，角色 = Executive。
- 主流程：
  1. 打开 Executive View，显示 `run_001` 的 Mission（降低流失）、Progress 70%、Human Approval Required、Risk 摘要。
  2. `view=executive` 时 `events` 仅返回 Goal/Decision/Risk，隐藏 Reasoning 细节。
- 异常流程：越权（Operator 以下角色访问）→ WB1002 / 角色校验拦截。
- 验收要点：三层视图数据同源（均来自 `wb_trace_event`）；Executive 视野由后端裁剪。


## 5. 风险点 & 注意事项
- 故事粒度过大无法独立验收，需拆分到可交付单元。

## 6. 关联文档
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/acceptance-criteria.md](../02_需求设计/acceptance-criteria.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填充 4.2 三个真实场景（US-1 开发者定义查询对象 / US-2 运营观察审批 / US-3 管理者全局视图），对齐 V0.1/V0.3 |

