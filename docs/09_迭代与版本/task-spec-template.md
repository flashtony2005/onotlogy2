# Task Spec 模板（任务定义规范）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-004 |
| 所属模块 | WorkBuddy-Version |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义一个 Task 的最小完备描述，使任何任务在动工前就把**能力、边界、定义、方法**写干净，杜绝"边做边想、范围蔓延、边界模糊"。Task 是 TDD 工程闭环（Roadmap→REQ→Test→Task→Code）的最小执行单元。

## 2. 业务/技术背景
- 依赖 `07_测试文档/test-plan.md`（TDD 总纲）、`09_迭代与版本/sprint-plan.md`（版本）、`02_需求设计/requirement-all.md`（REQ）、`07_测试文档/test-case.md`（Test）。
- 每个 Task 必须可追溯到 `Task → Test → REQ → 版本`。

## 3. 核心规则 & 约束（重点）
- [强制] 每个 Task 必须填齐四要素：**Capability / Boundary / Definition / Method**，缺一不得动工。
- [强制] Boundary 必须含「不做」清单（显式排除项），防止范围蔓延。
- [强制] 必须关联：TaskID、版本、REQ、Test（TC 编号）。
- [强制] Method 必须声明"先写哪个 Test（Red）"，符合测试先行。
- [建议] 一个 Task 聚焦一个能力，能在一次评审内讲清。

## 4. 详细设计/内容

### 4.1 四要素定义（CBDM）
| 要素 | 回答 | 必须包含 |
|------|------|----------|
| **Capability** 能力 | 交付什么 | 一句话能力，可判定 |
| **Boundary** 边界 | 做 / 不做什么 | 范围清单 + 显式排除项 |
| **Definition** 定义 | 契约是什么 | 接口签名 / 数据结构 / 输入输出 / 错误 |
| **Method** 方法 | 怎么实现 | 技术路径 + 依赖 + TDD 节奏（先写哪个 Test） |

### 4.2 空白模板（复制填写）
```markdown
# Task Spec：<能力名>
| 项 | 值 |
|----|----|
| TaskID | WB-TASK-XXX |
| 版本 | V0.x |
| REQ | REQ-XXX |
| Test | TC-XXX |
| 状态 | 待认领 / 进行中 / 已完成 |

## 1. Capability（能力）
一句话：本 Task 交付的能力。

## 2. Boundary（边界）
### 做
- 
### 不做（显式排除）
- 

## 3. Definition（定义）
- 输入：
- 输出：
- 接口签名：
- 数据结构：
- 错误：

## 4. Method（方法）
- 技术选型：
- 实现步骤：
- 依赖：
- TDD：先写 Test ___（Red），再最小实现转 Green，重构守护边界。

## 5. 完成判定（DoD）
对应 Test 全绿 + 代码评审通过 + 无可解释性违规。
```

### 4.3 示例（已填好）：REQ-001 ODL Parser
```markdown
# Task Spec：ODL Parser（.odl → Ontology IR）
| 项 | 值 |
|----|----|
| TaskID | WB-TASK-001 |
| 版本 | V0.1 |
| REQ | REQ-001 |
| Test | TC-001（含 TC-001-b 异常） |
| 状态 | 待认领 |

## 1. Capability（能力）
把一段 .odl 文本解析为语言无关的 Ontology IR（ObjectDef），供后续 Codegen 使用。

## 2. Boundary（边界）
### 做
- 解析 `object X:` 声明与 `properties` 列表（标量类型：string 等）
- 生成 IR：`ObjectType` + `PropertyDef`
- 非法输入返回结构化错误（含行列号）
### 不做（显式排除）
- 不生成代码（归 REQ-003 Codegen）
- 不持久化（归 REQ-005 Storage）
- 不支持 Relation / Event 语法（V0.2+）
- 不做任何 LLM 解析（确定性语法解析）

## 3. Definition（定义）
- 输入：`.odl` 源文本（UTF-8 `&str`）
- 输出：`Result<OntologyIR, OdlError>`
- 接口签名：`fn parse_odl(src: &str) -> Result<OntologyIR, OdlError>`
- 数据结构：
  `OntologyIR { object_types: Vec<ObjectType> }`
  `ObjectType { name: String, properties: Vec<PropertyDef> }`
  `PropertyDef { name: String, ty: ScalarType }`
- 错误：`OdlError { code: u32, message: String, line: u32, col: u32 }`

## 4. Method（方法）
- 技术选型：优先复用成熟 parser（pest / nom），查 workspace 已有依赖后再定；不重复造轮子
- 实现步骤：词法 token → 语法树 → 映射 IR → 错误定位
- 依赖：`ontology-ir`（ObjectType / PropertyDef 定义）
- TDD：先写 TC-001（Red：合法 .odl 解析出正确 IR；TC-001-b 非法输入返回错误码），再最小实现转 Green。

## 5. 完成判定（DoD）
TC-001 全绿（含异常用例 TC-001-b）+ 代码评审通过。
```

## 5. 风险点 & 注意事项
- Boundary 缺「不做」清单是范围蔓延的根源，评审须卡。
- Definition 与代码漂移会使契约失效；接口变更须同步更新本 Spec 与 Test。
- Task 过大（四要素写不满一页）应拆分，保持单一能力。

## 6. 关联文档
- [../07_测试文档/test-plan.md](../07_测试文档/test-plan.md)
- [../07_测试文档/test-case.md](../07_测试文档/test-case.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/acceptance-criteria.md](../02_需求设计/acceptance-criteria.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成：CBDM 四要素模板 + REQ-001 ODL Parser 示例 |
