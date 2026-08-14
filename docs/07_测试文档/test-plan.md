# 测试计划（TDD 总纲）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-07-001 |
| 所属模块 | OpenAIP-OS-Test |
| 版本 | V2.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
确立 **Test-Driven Development 为本项目强制工程方法论**：测试不是写完代码后的验证，而是写代码前的**边界定义**。本文定义 TDD 节奏、测试分层、工程闭环与版本测试门禁，是整个项目的质量与边界总纲。

## 2. 业务/技术背景
- 依赖 `02_需求设计/requirement-all.md`（REQ）、`02_需求设计/acceptance-criteria.md`（验收）、`09_迭代与版本/sprint-plan.md`（Roadmap）。
- 与 `09_迭代与版本/task-spec-template.md`（Task 定义）、`07_测试文档/test-case.md`（测试契约）配套。

## 3. 核心规则 & 约束（重点）
- [强制] **测试先行**：先写失败的测试（定义边界），再写实现；禁止"先实现后补测试"。
- [强制] **测试即边界**：一条测试 = 一个可执行契约；没有测试覆盖的能力 = 没有边界的能力 = 不允许合入。
- [强制] **版本测试门禁**：一个版本"完成"的唯一判定 = 该版本所有 Test 全绿；任何 Test 红，版本不可发布。
- [强制] 每个 Task 必须能追溯到 `Task → Test → REQ → 版本` 完整链路。
- [建议] 测试分层比例：单元 > 集成 > e2e（金字塔）。

## 4. 详细设计/内容

### 4.1 核心命题：测试即边界
```
方向看 Roadmap，边界看 Test，实现看 Task。
```
- Roadmap 回答"往哪走"，Test 回答"怎样算完成"，Task 回答"怎么做"。
- 测试先行把一个能力的**输入、输出、边界、异常**在写实现前就锁死，防止范围蔓延与理解偏差。

### 4.2 TDD 节奏：Red → Green → Refactor
1. **Red**：先写一个失败的测试，定义"要做什么、边界在哪"（此时功能不存在，测试必红）。
2. **Green**：写最小实现让测试通过，不多写一行。
3. **Refactor**：在测试守护下重构，边界不变。

### 4.3 测试金字塔（分层）
| 层 | 比例 | 覆盖对象 | 示例 |
|----|------|----------|------|
| 单元测试 | 多 | 纯函数 / 单方法 | Compiler 解析、IR 生成、Codegen、ObjectEngine 单方法 |
| 集成测试 | 中 | 模块协作 | Runtime+Storage 读写、Event Bus 分发、wb_trace_event 落库 |
| e2e 测试 | 少 | 端到端闭环 | `oaip compile→run→object list`、`oaip agent run` 全链路 |

### 4.4 工程闭环（可追溯）
```
Roadmap (sprint-plan)   定方向    版本线 V0.1~V0.5
   ↓
Requirement (REQ)        定需求    做什么
   ↓
Test Contract (test-case) 定边界   怎样算完成（先行，可执行契约）
   ↓
Task Spec (task-spec)     定实现    能力/边界/定义/方法
   ↓
Code
```
任何一行代码都能回溯到一个 Task、一条 Test、一个 REQ、一个版本。

### 4.5 版本测试门禁
| 版本 | 完成判定（全部全绿） | 测试契约 |
|------|----------------------|----------|
| V0.1 | TC-001 ~ TC-008 | Object Kernel + 观察基建 |
| V0.2 | V0.1 全绿 + 新增 Relation/领域 Event 用例 | World Model |
| V0.3（MVP） | V0.2 全绿 + TC-009 ~ TC-011 | Agent 闭环 |
| V0.4 / V0.5 | 前序全绿 + 本版新增用例 | Action & Policy / 自治 |

### 4.6 准入 / 准出标准
| 标准 | 说明 |
|------|------|
| 准入（开始一个 Task） | Task Spec 四要素（能力/边界/定义/方法）填写干净 + 对应 Test 已写成 Red 状态 |
| 准出（完成一个 Task） | 对应 Test 全绿 + 代码评审通过 + 无 `object_refs` 缺失等可解释性违规 |

## 5. 风险点 & 注意事项
- "先实现后补测试"会使测试沦为形式，边界失控，评审须卡 Red 先行。
- 只写 e2e 不写单元会导致反馈慢、定位难，须保持金字塔比例。
- 准出标准缺失会使上线质量主观化；版本门禁不可因赶工放宽。

## 6. 关联文档
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/acceptance-criteria.md](../02_需求设计/acceptance-criteria.md)
- [../07_测试文档/test-case.md](../07_测试文档/test-case.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V2.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V2.1 | lushaohui | 2026-08-11 | 修正 4.3 残留裸表名 trace_event → wb_trace_event（与 sql-script.md 权威表名一致）|
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../09_迭代与版本/task-spec-template.md](../09_迭代与版本/task-spec-template.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V2.0 | lushaohui | 2026-08-11 | 重写为 TDD 总纲：测试即边界、Red-Green-Refactor、测试金字塔、工程闭环、版本测试门禁 |
