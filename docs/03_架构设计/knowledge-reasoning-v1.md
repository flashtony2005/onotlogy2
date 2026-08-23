# Knowledge Reasoning & Governance v1 — 实现契约

| 项 | 内容 |
|----|------|
| 文档 ID | WB-DOC-03-KNOWLEDGE-V1 |
| 状态 | 当前实现契约（随代码演进刷新） |
| 范围 | `oaip-knowledge`、Runtime IR 投影、live-world ABox、推理 API 与本体演化治理 |
| 非目标 | 宣称完整 OWL 2 DL 推理、完整 XSD datatype/regex 兼容，或无边界大规模知识图谱平台 |

## 1. 定位与一句话契约

知识层把可执行 Runtime IR 和 live World 事实投影成 DL 知识库，用成熟 reasoner 做一致性与
包含推理；对 backend 尚未完整覆盖的数据片段使用确定性、保守的显式检查。所有本体变化都必须
经过影响分析、具名审批和 live-world 校验后才能生成新的不可变版本。

```text
ODL/Runtime IR ─┐
                ├─→ Canonical Knowledge Ontology ─→ Horned-OWL / owl-dl-reasoner
Live World ABox ┘                    │
                                     ├─→ Consistency / Classification / Explanation
                                     └─→ Governed Proposal → Review → Immutable Version
```

核心源码入口：

- 知识模型：`crates/oaip-knowledge/src/model.rs`
- 显式数据一致性：`crates/oaip-knowledge/src/data.rs`
- DL reasoner 适配：`crates/oaip-knowledge/src/reasoner.rs`
- Runtime IR 投影：`crates/oaip-knowledge/src/runtime.rs`
- 演化影响分析：`crates/oaip-knowledge/src/evolution.rs`
- 持久化治理：`crates/oaip-storage/src/ontology.rs`
- HTTP 契约：`src/server/ontology.rs`

## 2. 三类语义源

| 层 | 内容 | 边界 |
|----|------|------|
| Ontology / TBox | 类、属性、关系、约束、动作产生的静态 schema | 不可变版本保存 |
| Runtime IR | ODL 编译后的 object/action/policy/relation ABI | 可随版本持久化并热更新 |
| Live World / ABox | storage 中的对象标量值和关系边 | 每次诊断重新读取快照 |

Runtime world facts 不是“假装已经改变”的 projection。执行链必须先产生真实世界效应；
知识层只读取效应后的 snapshot。

## 3. Runtime ↔ Knowledge 投影

### 3.1 Runtime schema

`runtime_ir_to_knowledge` 会为每个对象类型生成 `Object ⊑ Thing`；每个标量属性生成
data-property domain、range 和 functional axiom；`object X extends Parent` 生成 named-class
SubClassOf 公理。显式关系和 action `link_edge` 共享同一命名空间，并生成 typed domain/range。
关系特征按 IR 投影为 transitive、symmetric、functional、inverse-functional、reflexive、
irreflexive、asymmetric 和 inverse。

规则：

- action edge 必须有唯一 source/target 类型。
- action edge 不能与显式 relation 的 endpoint 冲突。
- 关系不能同时声明 contradictory characteristics，例如 symmetric + asymmetric。
- canonical replacement 时先移除旧 runtime 投影，再加入新投影，避免版本残留。

### 3.2 Live world ABox

`runtime_world_to_knowledge` 把对象标量字段转成 typed data assertion，把边转成 object
property assertion。不同 storage ID 会被断言为 `DifferentIndividuals`，防止 OWL 因 functional
relation 把两个真实实体合并成同一个 individual。非标量 runtime 数据会 fail-closed，而不是被
静默丢成字符串。

## 4. 支持的知识表示子集

### 4.1 Literal

| Rust/Literal | XSD |
|--------------|-----|
| `Boolean` | `xsd:boolean` |
| `Integer` | `xsd:integer` |
| `Double` | `xsd:double` |
| `String` | RDF simple literal / `xsd:string` |
| `Decimal` | `xsd:decimal`, canonical exact lexical form |
| `Date` | `xsd:date`, strict calendar date, no timezone |
| `DateTime` | RFC 3339 instant canonicalized to UTC |

`decimal` 不经 IEEE float。小数比较和 digit 统计使用精确十进制词法算法。运行时 JSON 小数若会被
反序列化成 IEEE f64，则拒绝；整数 JSON number 可接受。

### 4.2 Class expression

支持：

- named class、`Thing`、`Nothing`
- object intersection / union / complement
- `ObjectSomeValuesFrom`
- `ObjectAllValuesFrom`
- `ObjectHasValue`
- `ObjectMinCardinality`
- `ObjectMaxCardinality`
- `DataSomeValuesFrom`
- `DataAllValuesFrom`
- `DataHasValue`
- `DataMinCardinality`
- `DataMaxCardinality`

未列出的 OWL 2 DL 表达式不应假设已支持。

### 4.3 Axiom

TBox / property：

- `SubClassOf`
- `EquivalentClasses`
- `DisjointClasses`
- Object property domain/range/subproperty
- Transitive、Symmetric、Functional、InverseFunctional、Reflexive、Irreflexive、Asymmetric
- Inverse object properties
- Data property domain/range/subproperty/disjointness
- `DataPropertyOneOf`
- `DataPropertyRangeRestriction`
- `DataPropertyDataRange`（可组合 datatype / oneOf / restriction / complement / union / intersection）
- Functional data property

ABox：

- class assertion
- positive/negative object property assertion
- positive/negative data property assertion
- same/different individuals

### 4.4 ODL object taxonomy

Executable object 可以直接声明继承：

```odl
object Animal:
  properties:
    name: string

object Pet extends Animal:
  properties:
    note: string

object Dog extends Animal, Pet:
  properties:
    breed: string

disjoint Dog, Cat
equivalent Pet, Companion
```

每个 edge 投影为 `SubClassOf(Dog, Parent)`，因此 classification、instances、realization 和
演化影响分析都会看到同一路径。父类型必须是已声明 object；重复父类型、自继承和继承环在 ODL 与
Runtime IR 校验边界 fail-closed。

顶层 `disjoint` / `equivalent` 分别投影为 `DisjointClasses` 和 `EquivalentClasses`。组内至少两个
已声明 object、成员不重复、同一语义组不可重复声明；这些约束同样作用于手工构造或持久化反序列化的
Runtime IR。

### 4.5 ODL property constraints

Executable ODL 属性现在可以在标量类型后直接声明保守约束：

```odl
object Customer:
  properties:
    code: string min_length 2 max_length 8 pattern "[A-Z]{2}[0-9]{4}"
    age: int min_inclusive 18 max_exclusive 65
    balance: decimal total_digits 10 fraction_digits 2
    nickname: string max_cardinality 0
    status: string one_of [Active,Pending]
```

解析器会按 declared scalar type 校验 facet 兼容性和 bound；quoted pattern 保留 regex escape。
Runtime IR 将这些约束物化为 `DataPropertyRangeRestriction`，因此它们参与 live-world 一致性检查、
推理诊断和不可变本体版本治理。

标量属性可另外声明 `min_cardinality` / `max_cardinality`。runtime JSON 标量字段最多只有一个
非空值，因此边界只接受 `0` 或 `1`：`min_cardinality 1` 要求 closed snapshot 观察到值，
`max_cardinality 0` 禁止值。该边界会投影为 `SubClassOf(object, DataMin/MaxCardinality(...))`，
参与 DL 推理和演化影响分析；executable admission 同时按 live snapshot 检查继承实例的缺失或
禁止字段，diagnosis 在 `dataCardinalityViolations` 显式报告。

`one_of` 可直接声明标量枚举，例如 `status: string one_of [Active,Pending]`。值按 declared scalar
type 做类型和 canonical form 校验（如 decimal 精确比较、date 日历日、dateTime UTC instant），最多
64 个成员且去重。ODL 枚举投影为 `DataPropertyOneOf`，参与 live-world 一致性、推理诊断、不可变版本
治理和 proposal 影响分析；不再需要为常见枚举手写 DL proposal JSON。

### 4.6 ODL relation cardinalities

Executable relation 可直接声明 source 端的参与基数：

```odl
relation placed:
  source: Customer
  target: Order
  min_cardinality: 1
  max_cardinality: 2
```

语义边界：

- `min_cardinality` / `max_cardinality` 是包含边界，约束每个 source 实例的出边数量；
- 未声明的一侧不引入约束，`min <= max` 在解析和 Runtime IR 治理边界共同强制；
- 投影为 `SubClassOf(source, ObjectMin/MaxCardinality(property, target))`；
- pure DL reasoning 保持 OWL 开放世界语义：缺少可观察的 min 边不会误报 logical inconsistency；
- executable admission 按 closed snapshot 逐个 source 实例精确检查，并沿 `extends` /
  `equivalent` named-class 关系识别子类型实例；evolution gate 拒绝违反，diagnosis 以
  `cardinalityViolations` 显式报告。

### 4.7 Composable data ranges

Governed DL proposal 可以使用通用 `DataPropertyDataRange` 表达属性值域：

```text
DataPropertyDataRange {
  property: "contact",
  range: Union([
    Restriction {
      datatype: String,
      facets: [Pattern("[^@]+@[^@]+")],
    },
    Datatype(Integer),
  ])
}
```

ODL 属性现在可以直接声明同一 base datatype 内的嵌套组合，不再必须为该子集手写
proposal JSON：

```odl
object Customer:
  properties:
    code: string range(intersection(datatype,union(restriction(pattern "[A-Z]{3}-[0-9]{4}"),one_of [ADMIN,GUEST])))
```

也可以用 `complement` 排除一个嵌套 data range：

```odl
object Customer:
  properties:
    code: string range(complement(restriction(pattern "TEST-.*")))
```

支持的组合子：

- `datatype`
- `one_of`
- `restriction`
- `complement`
- `union`
- `intersection`

实现边界：

- ODL 属性的 leading scalar type 是唯一 base datatype；`datatype` 与 `restriction`
  都指向它。跨 datatype 的 union 仍由 governed DL proposal 表达，避免破坏 executable
  row 的单一类型契约；
- `complement(child)` 表示 typed literal 全域中不属于 `child` 的部分。ODL admission 先
  强制 leading scalar type，因此 executable property 不会通过 complement 意外引入其他
  JSON/XSD 类型；
- `range` 是普通属性约束命名空间中的一个 facet，同一属性最多声明一个；它与
  `min_cardinality` / `max_cardinality` 可组合；
- boolean 组至少两个分支；
- 嵌套深度上限 8，节点总数上限 64，单个 enumeration 上限 64 个 literal；
- uncertain 或超界构造 fail-closed；
- positive ABox 断言由确定性 kernel 精确检查；mature tableau 同时接收对应 OWL 2 语法；
- ODL projection、closed-world admission、live-world evolution gate 和 HTTP diagnosis
  使用同一条 nested range 语义；
- 多个普通 range/restriction 公理仍按交集处理。

### 4.8 Datatype facet matrix

| Base datatype | 支持的 facet |
|---------------|--------------|
| `string` | `minLength`, `maxLength`, `pattern` |
| `integer` | `minInclusive`, `minExclusive`, `maxInclusive`, `maxExclusive` |
| `double` | 同 integer ordering facets |
| `date` / `dateTime` | 同 ordering facets，按日历日或 UTC instant 比较 |
| `decimal` | ordering facets + `totalDigits`, `fractionDigits` |
| `boolean` | 当前不允许 facet |

同一 restriction 内 facet 不重复。多个 range restriction 或 enumeration 会相交。

#### `xsd:pattern` 语义

当前实现是保守子集：

1. pattern 隐式完全锚定，等价于 `^(?:pattern)$`。
2. 只允许 `string` datatype 和 string bound。
3. 多个 `pattern` restriction 相交。
4. 使用 Rust `regex` 执行匹配。
5. uncertain syntax fail-closed：
   - 显式 `^` / `$`
   - inline modifier/group，如 `(?i)`
   - Rust word-boundary escapes `\b`, `\B`, `\A`, `\z`
   - nested/subtracted character class
   - 非 XSD character-class union/intersection
6. Unicode category 形式只接受可校验的 `\p{...}` / `\P{...}` 子集。

因此它不是完整 XSD regex 实现。拒绝优先于错误解释。

## 5. 推理语义

### 5.1 一致性

`Reasoner::is_consistent` 先执行 kernel 的 deterministic data consistency checks，再把剩余问题交给
`owl-dl-reasoner`。这样 mature tableau 未覆盖的 functional data conflict、range mismatch、facet
violation、enumeration conflict、subproperty polarity conflict 不会误报 consistent。

### 5.2 查询

HTTP `POST /api/v1/knowledge/reason` 支持：

- `consistency`
- `classification`
- satisfiability
- subclass entailment
- object/data property entailment
- instances / instance check
- realization

`GET /api/v1/knowledge/diagnosis` 一次性检查 persisted TBox + current live ABox，输出整体
consistency、unsatisfiable classes、unsatisfiability explanation 和 inconsistency explanation。

### 5.3 解释

解释接口覆盖 subclass、instance、object/data property、unsatisfiable class 和 inconsistency。
ontology 超过 `MAX_EXPLANATION_AXIOMS = 512` 时解释请求 fail-closed，避免把无界搜索伪装成证明。
同一 immutable snapshot 上的已完成解释结果进入容量为 128 的 LRU 缓存；缓存 key 覆盖全部
六类查询，失败探测不缓存。新版本创建新的 reasoner snapshot，因此不存在跨版本脏缓存。

### 5.4 Snapshot caching

`Reasoner` 绑定一份不可变 ontology snapshot，并在实例内缓存 consistency 与完整 classification。
同一 snapshot 上重复 satisfiability、instances、realization 或 evolution impact 查询可以复用该结果；
新版本必须创建新的 `Reasoner`，因此不存在跨版本脏缓存。24 类固定链路的 Criterion 基准显示，
首次分类约 1.36 ms，命中缓存后约 9.4 µs（本机开发环境微基线）。

### 5.5 明确限制

- 不宣称完整 OWL 2 DL entailment。
- general datatype entailment 仍受 mature backend 能力限制；data-property entailment 当前主要覆盖
  asserted fact、same-individual alias 和 data subproperty fragment。
- facet/range/enumeration 的确定性检查用于阻止不一致持久化和错误 query 结果，不等于声明完整 XSD
  datatype tableau。
- 大 ontology、大 ABox 的性能尚未完成生产级审计；当前正确性优先。

## 6. 演化治理契约

### 6.1 版本

- 初始版本必须显式初始化。
- 版本不可变。
- applied version 记录 parent version。
- rollback 不是覆盖历史，而是创建受评审的 replace proposal。

### 6.2 Proposal lifecycle

```text
Proposed ──validate──► Pending review
        ├─ inconsistent target / invalid live world ⇒ reject or fail closed
        ├─ approved by named reviewer ⇒ Approved
        └─ apply ⇒ new immutable target version + hot runtime update
```

`validate_ontology_proposal` 在源和目标上分别构建 reasoner，计算：

- current/target consistency
- newly/resolved unsatisfiable classes
- added/removed subsumptions
- unsatisfiable-class explanations

批准前无法应用；apply 时会重新验证 proposal 影响面是否仍与评审时一致。

### 6.3 Live-world gate

proposal 创建和 apply 都会加载当前 world snapshot。任何会导致现有对象、关系或数据事实不一致的
narrowing 都会在生成新版本前失败。该检查发生在事务提交前，因此不会留下 half-applied 状态。

### 6.4 审计

初始化、proposal、review、apply、rollback 相关事件写入 append-only audit chain。人工审批必须携带
reviewer；系统不会因为 validation 通过而自动授权 evolution。

## 7. HTTP/UI surface

| Endpoint | 作用 |
|----------|------|
| `GET /api/v1/knowledge/ontology/current` | 当前不可变版本 |
| `GET /api/v1/knowledge/ontology/versions` | 版本谱系 |
| `POST /api/v1/knowledge/ontology/init` | 初始化首个版本 |
| `POST /api/v1/knowledge/reason` | 单项推理查询 |
| `GET /api/v1/knowledge/diagnosis` | TBox + live ABox 诊断；relation 基数违反在 `cardinalityViolations`，scalar 基数违反在 `dataCardinalityViolations` |
| `GET/POST /api/v1/knowledge/proposals` | 列出/创建演化提案 |
| `POST /api/v1/knowledge/proposals/:id/review` | 具名批准/驳回 |
| `POST /api/v1/knowledge/proposals/:id/apply` | 应用为新版本 |
| `POST /api/v1/knowledge/rollback` | 创建回滚提案 |

客户端只校验 proposal envelope；DL、datatype、live-world 和治理语义由 backend 决定。

## 8. 验证矩阵

最低合并门禁：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
git diff --check
```

知识相关重点：

```sh
cargo test -p oaip-knowledge
cargo test -p oaip-storage ontology
cargo test --test knowledge_http
```

端到端：

```sh
cd ui && npm run typecheck && npm test -- --run && npm run build
make golden
```

golden 场景必须证明 ODL 编译、真实执行、审批、append-only audit、world query、WebSocket、演化后
闭环和 pending approval 重启恢复全部可用。

## 9. 已知下一步

1. 继续扩展更多 DL 结构到 ODL 直接表达面（如跨 datatype composable range），进一步减少手写 proposal JSON。
2. 在真实业务需要时继续扩展 XSD 构造（如自定义 datatype 定义），保持保守语义。
3. 为大 ABox 增加 incremental consistency 和跨版本 classification 复用。
4. 增加长时并发、恢复、内存和推理延迟基准。
5. 在能力矩阵继续扩展时同步刷新本文档。
