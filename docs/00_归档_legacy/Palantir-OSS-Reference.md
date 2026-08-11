> [废弃] 历史归档，已被现行文档体系（docs/01~10）取代，仅作溯源参考，不作为实现依据。

# Palantir 开源参考 · 本体平台架构基准

> 用途：为 OpenAIP-OS（开源 Palantir AIP Alternative）的本体平台架构决策提供成熟实现基准。  
> 对应原则：AGENTS.md #8「参考成熟方案」——先研究同类开源项目验证过的模式，不重复发明。  
> 关联文档：OpenAIP-OS V1.1–V1.6（Turso 本体 OS / Ontology DSL / Compiler / Rust Runtime）。

## 1. 核心结论

| 问题                | 结论                                                                    |
| ----------------- | --------------------------------------------------------------------- |
| Palantir 本体核心开源吗？ | ❌ **否**。Foundry / AIP 的 Ontology 引擎、推理、细粒度权限、数据血缘**全部闭源、不可自部署**。      |
| 有值得参考的开源吗？        | ✅ **有**。围绕 Ontology 的开发者工具链（OSDK、Conjure、Blueprint、MCP）均开源，是成熟且活跃的项目。 |
| 对我们最值得抄的是什么？      | **「本体即类型化 API」的范式**：从本体 schema 自动生成强类型客户端/服务端接口，让"操作对象"而非"操作表"成为编程模型。 |

**关键判断**：Palantir 的 Ontology 本质是"数据访问层 + 治理层"；OpenAIP-OS 走得更远——本体是**可执行**的（通过对 Rust Runtime 的 Capability/Workflow 调用）。因此 OSDK 的"接口范式"可借鉴，**执行内核必须自建**（这正是 V1.3 的 Ontology Compiler + Rust Runtime 在做的事）。

---

## 2. 可用开源仓库（按对你的价值排序）

| 仓库                                          | 星标/状态               | 是什么                                                               | 对你的价值                                             |
| ------------------------------------------- | ------------------- | ----------------------------------------------------------------- | ------------------------------------------------- |
| **palantir/osdk-ts**                        | 活跃(2026-08)         | Ontology SDK：把本体当作类型化对象 API（`objects.Employee.get()` 由 schema 生成） | 最接近"可执行本体"的接口范式，直接对标你的 Resource/Agent/Workflow 模型 |
| **palantir/conjure**                        | 活跃(4.52.0, 2025-08) | 契约优先 API 定义 + 多语言代码生成（YAML → IR → Java/TS/Python/Rust/Go）         | 对标你的 Ontology DSL → Compiler → 多语言绑定              |
| **palantir/ontology-starter-react-app**     | 34★                 | 在 Foundry Ontology 上构建 React 应用的起步仓库                              | 前端消费本体对象的样板                                       |
| **palantir/foundry-platform-typescript**    | 26★                 | Foundry 原生 API 的官方 TS 库                                           | 平台级 API 组织方式参考                                    |
| **palantir/palantir-mcp**                   | MIT                 | 把 Ontology 暴露为 MCP 服务器                                            | 本体 + LLM（P3.5 Planner）桥接层参考                       |
| **palantir/blueprint**                      | 21.8k★              | Foundry 前端使用的 React UI 工具包                                        | 本体驱动的界面组件范式                                       |
| **Defense Ontology + defense-sdk-examples** | 公开                  | 一个真实的公开本体 schema（抽象类型/接口）+ 示例                                     | 如何用"抽象类型接口"建模本体、build-once-deploy-anywhere        |
| **palantir/atlasdb**（历史）                    | 不活跃                 | Palantir 底层分布式事务数据库，曾驱动 Foundry 存储                                | 存储层模式参考（你的 turso/libSQL 已选型，仅作对照）                 |

> 闭源部分（不可参考实现，仅作能力对标）：Ontology 语义层、推理引擎、行/列级权限、数据血缘、版本管理。

---

## 3. 深度：osdk-ts 的工作机制

**架构 = 代码生成（codegen）+ 轻量运行时（runtime）混合。**

### 3.1 Schema 定义发生在哪里

- 本体 schema（Object Type / Action Type / Query / Link）在 **Foundry Developer Console** 定义。
- 本仓库**不定义 schema**，只消费生成的元数据。对应你方：本体 schema 由 **Ontology DSL（V1.2）** 定义，由 **Ontology Compiler（V1.3）** 编译——这一步你已规划，且比 Palantir 更开放（开放 DSL 而非锁定控制台）。

### 3.2 生成流程

- 生成器：`@osdk/foundry-sdk-generator`，CLI 形如 `osdk-unstable-typescript generate --internal`。
- 产物：类型声明（`.d.ts`）+ 客户端绑定，输出到 `src/generatedNoCheck/`。
- 运行时：`@osdk/client` 提供 `createClient(stackUrl, ontologyRid, auth)`，运行时不依赖 schema，只调用生成好的类型化方法；`@osdk/oauth` 负责鉴权。

### 3.3 仓库结构

```
packages/       核心包：client / oauth / api / foundry-sdk-generator
plugins/        构建 & 代码检查插件（oxlint 自定义规则）
examples/       示例应用模板（React beta 等）
examples-extra/docs_example  含 codegen 演示任务
tests/          集成测试（含 AI SDK provider）
scripts/        发布脚本
```

### 3.4 可借鉴点

1. **生成与运行时分离**：codegen 产出静态强类型，runtime 只做 HTTP/OAuth/对象操作。→ 你的 Rust Runtime 也应把「本体编译产物（类型/绑定）」与「执行内核」解耦。
2. **`.d.ts` 即契约**：类型声明是本体与代码之间的唯一真相源。→ 你的 Ontology Compiler 应输出「类型定义 + 执行绑定」两套产物。
3. **monorepo + turbo/pnpm**：多包协同的工程化范式。

---

## 4. 深度：conjure 的工作机制

**契约优先（contract-first）API 工具链：定义一次，生成多语言接口。**

### 4.1 契约格式

- 人类可读的 **YAML**。结构：
  ```yaml
  types:
    definitions:
      objects:
        Airport:     # string 别名
        SearchRequest:
          fields:
            from: string
            to: string
            time: datetime
  services:
    FlightSearchService:
      base-path: /flights
      endpoints:
        search:
          http: POST /search
          args: SearchRequest
          returns: Connection
  ```

### 4.2 生成流水线

```
YAML 契约
   ↓  conjure 编译器
JSON 中间表示（IR）   ← 规范：docs/spec/intermediate_representation.md
   ↓  各语言 generator（符合 RFC002 的 CLI）
Java / TypeScript / Python / Rust / Go 客户端 & 服务端
```

- 可选验证：`conjure-verification`（线格式合规）、`conjure-postman`（生成 Postman 集合）、`conjure-backcompat`（IR 向后兼容检查）。
- 构建编排：`gradle-conjure` 插件。

### 4.3 可借鉴点

1. **IR 中间表示是关键**：YAML → IR → 多语言生成，使新增语言只需写一个 generator。→ 你的 Ontology Compiler 应设计**与语言无关的 IR**（如 OntologyIR），Rust 绑定只是其中一个 target。
2. **向后兼容检查**：`conjure-backcompat` 对 IR 做兼容性校验。→ 本体演化（V1.6 自我进化）必须有 IR 级兼容性闸门，这正是你 F4 内核冻结 + 扩展层（reaction/workflow/pipeline）的设计前提。
3. **契约即文档**：`conjure.schema.json` + `reference.md` 让契约自带 schema 校验与文档。

---

## 5. 映射到 OpenAIP-OS 架构

| Palantir 模式                  | OpenAIP-OS 对应                                 | 差异 / 行动                                        |
| ---------------------------- | --------------------------------------------- | ---------------------------------------------- |
| Foundry Console 定义 Ontology  | **Ontology DSL（V1.2）**                        | 你更开放：DSL 文本化、可版本化、可自托管                         |
| OSDK codegen → 类型化客户端        | **Ontology Compiler（V1.3）**                   | 你的产物不仅是类型，还要生成**执行绑定**（Capability/Workflow 调用） |
| OSDK runtime（`createClient`） | **Rust Runtime Kernel（F4）**                   | 你的内核真执行，Palantir 偏查询/写入网关                      |
| Conjure YAML → IR → 多语言      | **Ontology DSL → OntologyIR → 多 target**      | 设计语言无关的 IR；Rust 是首个 target，未来可加 TS/Go          |
| Defense Ontology 抽象类型        | **对象模型五维**（Data/Logic/Action/Policy/Relation） | 抽象接口 + 具体实现分离，已对齐                              |
| palantir-mcp                 | **P3.5 Planner ↔ 本体的 MCP 桥**                  | 让 LLM 通过 MCP 读写本体对象                            |

拉 

`osdk-ts`

的 

`@osdk/foundry-sdk-generator`

子包看 codegen 模板实现细节

## 6. 不可抄 vs 必须自建

**不可抄（闭源，仅对标能力）：**

- 推理引擎、行/列级权限、数据血缘、本体版本管理。
- → 你的 OAE RBAC 4 层、Saga 补偿、血缘（如需）必须自建，这正是 OpenAIP-OS 的护城河。

**必须借鉴（开源且成熟）：**

1. 本体即类型化 API（OSDK 范式）。
2. 契约优先 + IR + 多语言生成（Conjure 范式）。
3. codegen 与 runtime 解耦。
4. IR 级向后兼容闸门（支撑本体自我进化）。

---

## 7. 下一步建议

1. **设计 OntologyIR**：在 Ontology Compiler 中引入语言无关的中间表示，作为 Rust 绑定及其他未来 target 的统一输入。
2. **Compiler 双产物**：编译本体 → 同时输出「类型定义」+「执行绑定（Capability/Workflow）」。
3. **兼容性闸门**：对 OntologyIR 做向后兼容校验，支撑 V1.6 的自我进化闭环。
4. **MCP 桥**：参考 `palantir-mcp`，把本体对象暴露为 MCP 资源/工具，供 P3.5 Planner 调用。
5. **前端样板**：参考 `ontology-starter-react-app` + `blueprint`，定义前端消费本体对象的标准模式。

> 数据来源：github.com/palantir/osdk-ts、github.com/palantir/conjure（2026-08-10 抓取）。闭源能力边界依据 Palantir 官方文档与社区分析。

---

## 附录 A：@osdk/foundry-sdk-generator 的 codegen 实现细节

> 来源：github.com/palantir/osdk-ts → `packages/generator/src/v2.0/`（2026-08-10 抓取）。
> 目的：看 Palantir 真实如何把本体元数据编译成类型化 TS，作为你 Ontology Compiler 的实现参照。

### A.1 技术栈：**字符串模板，不是 AST**
- 每个实体生成函数返回一段拼接的 TS 源码字符串，用自研 `stringify()`（`util/stringify.ts`）遍历对象产出 `key/value` 对，最后 `formatTs()` 美化。
- 产物即 `.ts` 源文件（类型声明与运行时 const 一体），**不是** ts-morph / TS Compiler API 构建。
- 启示：codegen 不必上 AST 框架；模板字符串 + 一个对象序列化器足够，简单且易维护（符合 AGENTS.md #2）。

### A.2 目录结构（核心）
```
packages/generator/src/v2.0/
  generateClientSdkVersionTwoPointZero.ts   主流程编排
  wireObjectTypeV2ToSdkObjectConstV2.ts      ObjectType → TS
  generatePerActionDataFiles.ts             Action → TS
  generatePerQueryDataFiles.ts              Query → TS
  wireInterfaceTypeV2ToSdkObjectConst.ts    Interface Type → TS
  generateMetadata.ts / generateRootIndexTsFile.ts
  GenerateContext/  shared/  util/stringify.ts
```

### A.3 ObjectType → TS（关键范式）
生成三件套：`namespace X {}` + `interface X extends $ObjectTypeDefinition` + `const X = {...} satisfies X`。
```ts
export namespace Employee {
  // PropertyKeys：字符串联合类型
  // Props：readonly "apiName": $PropType["wireType"]
  // ObjectSet extends $ObjectSet<Employee>
  // OsdkInstance = $Osdk.Instance<Employee, OPTIONS, K>
}
export const Employee = { type:"object", apiName:"...", primaryKeyApiName:"...",
  osdkMetadata: $osdkMetadata, ... }
  satisfies Employee & { internalDoNotUseMetadata: { rid: string } } as Employee;
```
- **字段映射靠索引表，无硬编码分支**：
  ```ts
  // 默认分支：$PropType[JSON.stringify(propertyDefinition.type)]
  // multiplicity → "[]"，nullable → "| undefined"
  ```
  string/integer/datetime/attachment 全部经 `PropertyValueWireToClient` 这张索引表映射。
- **Link 映射**：
  ```ts
  readonly key: multiplicity
    ? `${linkTarget}.ObjectSet`
    : `$SingleLinkAccessor<${linkTarget}>`;
  ```

### A.4 Action → TS（函数签名接口，非 class）
```ts
export interface Signatures {
  applyAction<OP>(args: Employee.Params, options?: OP): Promise<ActionReturnTypeForOptions<OP>>;
  batchApplyAction<OP>(args: ReadonlyArray<Employee.Params>, options?: OP): ...;
}
// 参数：ActionParam.PrimitiveType<"integer"> / ActionParam.ObjectType<Employee>
// 数组→ReadonlyArray，可空→| null
// 最终：interface Employee extends ActionDefinition<Employee.Signatures> + const Employee
```

### A.5 Query → TS（switch 参数类型）
```ts
// Signature: (query?: X.Parameters) => Promise<X.ReturnType>
case "object":  Query${type}.ObjectType<Employee>
case "struct":  `{ ${stringify(input.struct)} }`
case "array":   ReadonlyArray<${recurse(input.array)}>
case "objectSet": ObjectSetType
case "typeReference": CustomTypes 命名空间
```

### A.6 生成物与运行时衔接（最重要的一点）
- 生成的 `const Employee` 是**携带元数据的运行时值**（`apiName`/`primaryKeyApiName`/`rid`）。
- `@osdk/client` 提供泛型"蓝图"：`$Osdk.Instance` / `$ObjectSet` / `PropertyValueWireToClient` / `ActionDefinition` / `QueryDefinition`，由生成的定义参数化。
- `client(Employee).where(...)` 即消费这套「类型 + 元数据」。
- **本质**：本体元数据被编译成「带 `satisfies` 约束的 const 元数据 + 命名空间类型」——在 TS 中**既是编译期类型安全的 API 表面，又是运行期可反射的元数据**。

### A.7 对你 Ontology Compiler 的直接借鉴
1. **元数据即值（metadata-as-const）**：生成物要同时是「编译期类型」和「运行期可反射的元数据表」。Rust 侧可生成 `struct + const METADATA` 表，Runtime 读 METADATA 构造调用。
2. **查表式类型映射**：wireType → 目标类型用索引/查找表驱动（如 `PropertyValueWireToClient[wireType]`），**不要写 if/else 分支**。你的 OntologyIR → Rust 类型映射应同理。
3. **Definition 与 Instance 分离**：`定义（元数据/类型）` 与 `实例（类型化表面）` 解耦，对应你 V1.3 的「编译产物 = 类型定义 + 执行绑定」。
4. **模板字符串足矣**：codegen 不必引入 AST 框架，自研一个对象→源码的 `stringify` 即可，简单优先。
5. **`satisfies` 约束**：生成的 const 用 `satisfies` 校验符合接口，保证生成物与契约一致——你可用 Rust 的 trait bound / 编译期校验做等价保证。
