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
irreflexive、asymmetric、inverse 和 composed-of property chain。

规则：

- action edge 必须有唯一 source/target 类型。
- action edge 不能与显式 relation 的 endpoint 冲突。
- 关系不能同时声明 contradictory characteristics，例如 symmetric + asymmetric。
- `inverse_of` 目标必须唯一；单向声明合法，但端点必须互换。若两端都声明，则必须互逆。
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
- `ObjectOneOf`（nominal / enumerated class；空枚举等价 `Nothing`）
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
- Object property domain/range/subproperty/property chain/disjointness
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

常见 covered-class 模式可直接声明 disjoint union，不再手写 proposal JSON：

```odl
object Vehicle:
  properties:
    label: string

object Car:
  properties:
    wheels: int

object Boat:
  properties:
    draft: decimal

disjoint_union Vehicle = Car, Boat
```

该声明投影为 `EquivalentClasses(Vehicle, ObjectUnionOf(Car, Boat))` 和
`DisjointClasses(Car, Boat)`。目标类与成员都必须是已声明 object；成员至少两个、不重复，且不能
包含目标类。同一目标类只能定义一次。ODL 解析、手工构造 Runtime IR 和持久化治理边界使用同一条
校验语义；结果参与一致性、classification、entailment、诊断和演化影响分析。

对象类型还可以在 `restrictions:` 块直接声明 object-property 必要条件：

```odl
object Parent extends Person:
  properties:
    note: string
  restrictions:
    child: some Person
    contact: only Person
    guardian: value alice
```

三条声明分别投影为 `SubClassOf(Parent, ObjectSomeValuesFrom(child, Person))`、
`SubClassOf(Parent, ObjectAllValuesFrom(contact, Person))` 和
`SubClassOf(Parent, ObjectHasValue(guardian, alice))`。relation 必须是已声明显式 relation 或
action link edge；`some` / `only` 的 filler 必须是已声明类，`value` 的 individual 在 TBox 层保持
开放世界语义。这些限制参与 classification、live-world 一致性、诊断和演化影响分析。

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

### 4.7 ODL relation property chains

Executable relation 可以直接声明 OWL 2 property chain，不必手写 proposal JSON：

```odl
relation has_parent:
  source: Person
  target: Person

relation has_grandparent:
  source: Person
  target: Person
  composed_of: has_parent has_parent
```

`composed_of` 是有序属性序列，投影为
`SubObjectPropertyChainOf { properties, super_property }`。因此 `has_parent ∘ has_parent`
的 ABox 蕴含会进入同一 canonical ontology，供 entailment、explanation、诊断和演化影响分析使用。

语义边界：

- 至少两个成员；成员必须是显式 relation 或 action 声明的 object property；
- 同一属性可重复出现，用于表达传递式的组合；
- ODL 解析和手工构造 / 反序列化 Runtime IR 的 governance boundary 都会拒绝未声明属性或单元素链；
- super relation 自身仍必须声明 typed source/target，其 domain/range 和关系特征照常投影。

### 4.8 ODL relation hierarchy, disjointness, and equivalence

Executable relation 可以直接声明对象属性包含、不相交性和等价性：

```odl
relation biological_parent:
  source: Person
  target: Person

relation legal_parent:
  source: Person
  target: Person

relation parent:
  source: Person
  target: Person

relation guardian:
  source: Person
  target: Person

sub_relation biological_parent, parent
disjoint_relation biological_parent, legal_parent
equivalent_relation parent, guardian
```

三类顶层声明分别投影为 `SubObjectPropertyOf`、`DisjointObjectProperties`，以及等价组内
所有有序对的 `SubObjectPropertyOf`。因此 `biological_parent(a,b)` 会蕴含 `parent(a,b)`，
`parent(a,b)` 与 `guardian(a,b)` 互相蕴含；同一 individual pair 同时出现
`biological_parent` 和 `legal_parent` 时会进入 mature-reasoner 一致性失败，并被
live-world evolution gate 拒绝。

语义边界：

- `sub_relation` 恰好接受两个已声明显式 relation，且子关系不能等于父关系；
- `disjoint_relation` 至少接受两个已声明显式 relation，组内不重复、同一语义组不可重复声明；
- `equivalent_relation` 至少接受两个已声明显式 relation，组内不重复、同一语义组不可重复声明；
- 层级允许跨不同 typed endpoints，由此引发的 domain/range 冲突由 canonical ontology 的
  consistency gate fail-closed，而不是在语言层静默丢弃；
- 手工构造或反序列化 Runtime IR 必须通过同一 validation boundary；
- 新 axiom 参与 entailment、explanation、OWL Functional Syntax 往返和演化影响分析。

### 4.9 ODL data-property hierarchy and disjointness

Executable scalar properties can declare the common data-property DL axioms directly:

```odl
object Person:
  properties:
    email: string
    work_email: string
    mobile: string
    landline: string

sub_data_property work_email, email
disjoint_data_property mobile, landline
equivalent_data_property primary_contact, contact
```

The three top-level declarations project to `SubDataPropertyOf`, `DisjointDataProperties`, and
ordered pairs of `SubDataPropertyOf` for an equivalence group. Therefore a value asserted on
`work_email` entails the same value on `email`; a value on either equivalent contact property
entails the same value on the other; one individual with both `mobile` and `landline` values
makes the canonical knowledge base inconsistent and is rejected by the live-world evolution gate.

语义边界：

- `sub_data_property` 恰好接受两个已声明的 executable scalar property，且子属性不能等于父属性；
- `disjoint_data_property` 至少接受两个已声明 scalar property，组内不重复、同一语义组不可重复声明；
- `equivalent_data_property` 至少接受两个已声明 scalar property，组内不重复、同一语义组不可重复声明；
- 属性名解析为 data-property 命名空间，不会把显式 relation 当作合法成员；
- 手工构造或反序列化 Runtime IR 必须通过同一 validation boundary；
- 新 axiom 参与 entailment、explanation、OWL Functional Syntax 往返和演化影响分析。

### 4.10 Composable data ranges

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

常见跨 datatype 值域可以直接用显式 typed union 表达：

```odl
object Customer:
  properties:
    contact: string range(union(restriction(pattern "[^@]+@[^@]+"),datatype(int)))
```

支持的组合子：

- `datatype`（不限定参数时表示 leading scalar type；`datatype(int)` 等显式形式表示另一个 base datatype）
- `one_of`
- `restriction`
- `complement`
- `union`
- `intersection`

实现边界：

- 不带参数的 `datatype` 和 `restriction` 指向 property 的 leading scalar type。
  `datatype(int)` 这类显式 typed branch 只能作为 union 的直接分支，用来扩大该属性可接受的
  finite scalar-type 集合；cross-datatype union 必须直接保留一个 leading-type 分支（裸
  `datatype`、leading `one_of` 或 leading `restriction`）；
- cross-datatype range 不能与外部 value facet 或 enumeration 组合。datatype-specific
  facet 必须放进对应 union branch，避免把 leading-type constraint 错误应用到其他类型；
  `max_cardinality 0` 可组合，但 required `min_cardinality 1` 会 fail-closed；
- executable admission 接受 declared type 加上所有显式 typed branch；其他 JSON 类型仍然
  fail-closed。存储和 runtime row 继续以 leading scalar type 作为主 schema 类型；
- 无 cross branch 时，`complement(child)` 表示 typed literal 全域中不属于 `child` 的部分，
  admission 先强制 leading scalar type；有显式 typed branch 时，只有列出的 branch 类型可以
  进入该 range；
- `range` 是普通属性约束命名空间中的一个 facet，同一属性最多声明一个；它与
  `min_cardinality` / `max_cardinality` 可组合；
- boolean 组至少两个分支；
- 嵌套深度上限 8，节点总数上限 64，单个 enumeration 上限 64 个 literal；
- uncertain 或超界构造 fail-closed；
- positive ABox 断言由确定性 kernel 精确检查；mature tableau 同时接收对应 OWL 2 语法；
- cross-datatype projection 用同一条 `DataPropertyDataRange` 表示完整 union，并省略会与之
 矛盾的单独 leading `DataPropertyRange` axiom；因此 reasoner、diagnosis 和演化 gate 看到
 同一个允许集合；
- ODL projection、closed-world admission、live-world evolution gate 和 HTTP diagnosis
  使用同一条 nested range 语义；
- 多个普通 range/restriction 公理仍按交集处理。

### 4.11 Datatype facet matrix

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
- types of an individual（包含继承和表达式蕴含的类型）
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
新版本必须创建新的 `Reasoner`，因此不存在跨版本脏缓存。

进程内还保留一个容量为 16 的 immutable snapshot LRU。相同 ontology 的多个 `Reasoner`
实例共享同一份 snapshot；consistency 和 classification 一旦完成，也会随 snapshot 对后续
实例可见。cache key 只用于定位候选，复用前仍执行完整 ontology equality，避免 fingerprint
碰撞串用知识库。24 类固定链路的 Criterion 基准显示，首次分类约 1.36 ms，命中缓存后约
9.4 µs（本机开发环境微基线）。

### 5.5 Large-ABox consistency

`validate_data_consistency` 在单次调用内按 property 索引 range、restriction、enumeration，
并按 canonical individual + property 索引负断言；functional 冲突和 disjoint data properties
在扫描正断言时同步判定。这样避免了对每个事实线性搜索全部公理或负断言，也不改变既有
确定性检查语义。

同一 immutable ontology 的已完成结果还会进入进程内容量为 32 的 exact-result LRU，包括
确定性错误结论；因此 repeated governance/diagnosis 调用不会重复索引同一个不可变版本。
cache key 只用于定位候选，命中前仍执行完整 ontology equality，避免 fingerprint collision。
Criterion cold path 使用每个迭代唯一的 inert `SameIndividuals`（单个 term 不建立 alias），
防止 exact-result 和 prefix cache 隐藏索引成本；本机 1,000 条 functional-property 断言的
cold rebuild 约 973 µs，命中 exact-result cache 后约 35 µs。

在上述 exact-result 复用之上，data fragment 现已支持 **append-only ABox delta
consistency**。缓存条目保留 data-property hierarchy、range/restriction/enumeration、
negative assertion、functional conflict 和 disjoint-property 的索引状态。新查询先按
data-relevant axiom 序列寻找最长已缓存前缀：忽略 `ClassAssertion`、object assertion、
`DifferentIndividuals` 等不影响该确定性片段的 axiom；若后缀只是新增 positive/negative
data assertion，就克隆索引并只检查新增事实。任何 SameIndividuals、TBox 或其他可改变既有
canonical identity / constraint set 的后缀都会退回全量重建。前缀命中必须逐 axiom 比较；
fingerprint 只用于 exact-result 定位，不会授权前缀复用。

Criterion 固定基准在 1,000 条 base 上测量有界 append-only chain：每次追加到上一次已验证
snapshot，而不是生成互不为前缀的 sibling snapshot。delta path 约 191 µs；它仍需 fingerprint、
projection 和逐 axiom 前缀确认，因此不宣称 O(suffix)，但避免重建完整 data index。delta 结果
也会写入同一个容量 32 的 LRU，供下一次追加继续形成前缀。

append-only 之外，成功缓存的 snapshot 还可作为 **assertion patch base**：当两个 ontology 的
非断言 data-relevant axiom 完全相等（SameIndividuals、subproperty、range 和 restriction 都未变），
正/负 assertion 可按 multiset 对称差归约为 remove/add 操作。删除会先经 subproperty hierarchy 展开，
并在修改克隆索引前证明全部可删除；canonical decimal、date、dateTime、double 等值使用等价 key，
避免只改词法形式的事实被误判为冲突。dateTime key 保留秒和 sub-second nanos，避免远未来时间戳
折叠后隐藏断言差异；删除一个 property 时也会保留同 individual 的其他 property 索引，使 disjoint
检查继续完整。这里的“absence”只是移除旧 positive assertion，不会转换成 OWL negative assertion。
schema 不相等或 removal 无法应用时仍退回全量重建，patch 复用计入 prefix-hit 指标。
Criterion 的 bounded replace-chain 每次替换 1,000 条 functional ABox 中的一条断言，当前本机
patch path 约 780 µs；该路径需要 schema equality、multiset delta、cached-state clone 和局部重索引，
因此不是 O(1)。

append-only 序列中的真实 `SameIndividuals`（至少两个 term）现在也可增量应用：先合并
identity graph，再在新 canonical individual 下重新索引既有正/负 assertion，因此旧事实会立即参与
functional conflict、positive/negative contradiction 和 disjoint-property 检查。若该 axiom 只是重复
已有等价关系，则不触发重索引；单个 term 的 inert `SameIndividuals` 继续退回全量重建，以保留 cold-path
基准对索引成本的可见性。identity 合并的成本与现有 assertion 数量相关，不宣称 O(1)。

这条缓存已经接入存储治理执行路径。`initialize_knowledge_version`、
`propose_knowledge_change` 和 `apply_knowledge_proposal` 统一通过
`validate_canonical_ontology_with_world` 调用 data consistency，因此长生命知识版本会自动复用
exact result、data-relevant prefix 和 append-only delta，而不需要 storage 维护第二套索引。
治理中的事实替换/删除会继续落入同一入口的 assertion patch 路径。
prefix 复用前只比较 `data_consistency_relevant(axiom)` 投影，避免 live-world snapshot 尾部的
`DifferentIndividuals` 重写错误地阻断可安全复用的前缀。`DataConsistencyCacheStats` 暴露
exact hits、prefix hits、rebuilds 和 delta assertions，用于验证治理流程确实命中缓存。
进程级 cache 由 mutex 序列化元数据访问；cached index 通过 immutable `Arc` 共享，delta 路径
需要修改时先 clone。并发回归覆盖多个 worker 同时验证各自 valid 与 conflicting ontology 的
场景，防止 exact/prefix 候选跨 snapshot 串用。

在短 burst 之外，固定 durability 基准继续覆盖 4 轮 ×（8 线程 × 128 次）的持续并发；
bounded LRU 驱逐后目标 snapshot 的完整索引恢复；以及包含 schema 初始化、已提交版本和
pending proposal 的 file-backed storage 真实重开。推理延迟覆盖新建 immutable snapshot 后的
full TBox/ABox consistency 与同 snapshot 命中 `OnceLock` 的 cached consistency。索引扩展基准
测量 250/500/1,000/2,000 assertion 的 cold rebuild 时间趋势；进程 RSS 由真实 HTTP/WS 负载脚本
周期采样并写入 `memory_rss_kb`。这些是本机开发环境基线，不把时间趋势等同生产内存审计，
也不替代目标硬件上的长周期 soak。

知识面长周期验证使用 `make soak-baseline`（`scripts/knowledge-soak.py`）。该演练启动真实
release 二进制，持续并发执行 consistency、classification 和 diagnosis，并按固定间隔创建、评审、
应用交替 remove/add 的 ontology proposal；全程采样 RSS，预热后的时间序列用于拟合增长趋势。
负载结束后写入一个 Pending `RecoveryProbe` proposal，用 SIGKILL 模拟进程崩溃，重启后校验当前
版本、Pending 提案和 diagnosis 完全恢复。JSON 产物记录 UTC 时间、主机、OS/CPU、Git revision、
二进制与 ODL SHA-256，以及原始 RSS 序列和失败样本。该入口保证演练可复现；未在目标硬件上归档
足够长时间的产物前，不宣称已完成生产化 soak。

### 5.6 明确限制

- 不宣称完整 OWL 2 DL entailment。
- general datatype entailment 仍受 mature backend 能力限制；data-property entailment 当前主要覆盖
  asserted fact、same-individual alias 和 data subproperty fragment。
- 上游 tableau 会把 reachable nominal（`ObjectOneOf`）保守分类为 snapshot-replay unsafe，
  并退回逐查询推理路径。这是性能边界，不是拒绝 nominal 语义。
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

1. 继续按真实业务需要扩展更多 DL 结构到 ODL 直接表达面，进一步减少手写 proposal JSON。
   `disjoint_union` 与 object-property `some` / `only` / `value` 限制已经直接进入 ODL。
2. 在真实业务需要时继续扩展 XSD 构造（如自定义 datatype 定义），保持保守语义。
3. 在 update/delete、append-only SameIndividuals、数据属性约束和
   `SubDataPropertyOf` 已支持增量复用后，继续处理其余 TBox 变更。
   长生命快照已接入存储执行路径并具备缓存命中率可观测性。
4. 已增加持续并发、缓存驱逐恢复、持久化 pending proposal 重开、推理一致性和固定规模索引
   扩展基准；HTTP 负载现在采样进程 RSS。`make soak-baseline` 已补齐知识推理、受控演化、
   RSS 增长拟合与 Pending proposal SIGKILL 重启校验的统一演练入口；仍需在目标硬件上执行并
   归档长周期生产化基线。
5. 在能力矩阵继续扩展时同步刷新本文档。
