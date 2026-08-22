---
name: business-pack-operations
description: 管理 OpenAIP-OS 公司业务包和领域插件。当用户要求创建、检查、安装、启动、验证、同步、停用或排查业务包（DomainPack），尤其是 lottery-analysis、crm-retention 等业务包时触发。遵循本体定义、bootstrap 初始事实、插件能力和宿主运行时分层，不把业务规则硬编码到宿主中。
---

# Business Pack Operations

## 目标

- goal: 在不修改宿主本体运行时的前提下，安全地校验、安装、运行和排查一个业务包

## 输入

- input: 业务包目录路径，例如 domain-packs/lottery-analysis
- input: 本地数据库路径，例如 ontology.db
- input: 业务操作，例如 validate、install、sync、analyze、backtest、review
- input: 业务数据文件，例如 dlt.csv 或 double.csv

## 输出

- output: 业务包校验结果
- output: 安装报告（项目、对象、关系）
- output: 插件能力执行结果
- output: 可追溯的错误、冲突和审计信息

## 边界原则

1. 业务包是宿主上的领域模块，不是另一套数据库。
2. `ontology.odl` 定义概念、属性和关系；`bootstrap.json` 提供初始设置和事实；插件代码提供可执行能力。
3. 宿主负责项目、存储、权限、审计、任务调度和插件注册；业务包负责领域解析、推导、统计和回测。
4. 不直接修改数据库表，不绕过 `WorldCommand`，不绕过 `PluginCatalog`。
5. 不把 dsh 或 Pi Agent 作为基础导入、校验和统计的前置依赖。
6. 不因重复安装而覆盖内容不同的对象或改变已有本体语义。
7. 不把实验性策略混入事实导入；统计、启发式和玄学策略必须分别执行和回测。

## Stage 1: Identify Pack

先检查业务包目录是否包含：

- `manifest.json`
- `ontology.odl`
- `bootstrap.json`
- 可选的 `scenarios/`
- 可选的插件和 UI 资源

读取 `manifest.json`，确认 `id`、`name`、`version`、`ontology`、`bootstrap`。业务包 id 是项目 id；已有项目的本体内容不允许静默替换。

## Stage 2: Validate Pack

执行只读校验：

```bash
oaip --db ontology.db domain-pack validate domain-packs/lottery-analysis
```

校验失败时停止后续操作，优先检查：

- manifest 引用的文件是否存在且位于业务包目录内；
- ODL 是否能解析；
- bootstrap 对象类型是否在 ODL 中定义；
- bootstrap 字段是否符合对象定义；
- relation 的 source 和 target 是否在 bootstrap 中声明；
- id、name、version 是否为空或非法。

## Stage 3: Install Pack

只有校验成功后才能安装：

```bash
oaip --db ontology.db domain-pack install domain-packs/lottery-analysis
```

安装必须通过宿主的 DomainPack 安装入口完成。确认安装报告中的：

- `project_id` 等于 manifest.id；
- 新建和跳过的对象数量符合预期；
- 新建和跳过的关系数量符合预期；
- 重复安装只跳过完全相同的对象和关系；
- 已存在但内容不同的对象导致明确错误，而不是覆盖。

## Stage 4: Start Runtime

本地服务使用业务包的本体定义启动：

```bash
oaip --db ontology.db serve \
  --domain-pack domain-packs/lottery-analysis \
  --host 127.0.0.1 \
  --port 18420
```

macOS `.app` 运行时可以通过资源目录或 `ONTOLOGY_DOMAIN_PACK` 指定业务包。不要同时把不同业务包的 ODL 混用到同一个项目中。

## Stage 5: Execute Lottery Capabilities

彩票业务包的插件能力通过 `PluginCatalog` 调用。Skill 只引用已经注册的能力，不把 CLI 文本命令伪装成新的 capability：

- capability: lottery.data.sync
- capability: lottery.import
- capability: lottery.analyze
- capability: lottery.backtest
- capability: lottery.review

CLI 示例：

```bash
增量导入/同步 CSV：
oaip --db ontology.db lottery import \
  --game dlt \
  --project lottery-analysis \
  data/dlt.csv

统计分析：
oaip --db ontology.db lottery analyze \
  --game dlt \
  --window 50 \
  --project lottery-analysis

滚动回测：
oaip --db ontology.db lottery backtest \
  --game dlt \
  --train-window 50 \
  --test-count 30 \
  --project lottery-analysis

Pi Agent 复核（可选）：
oaip --db ontology.db lottery pi \
  --game dlt \
  --window 50 \
  --test-count 30 \
  --project lottery-analysis
```

执行顺序：

```text
lottery.data.sync
  → imported > 0 ? lottery.analyze
  → lottery.backtest
  → 可选 lottery.review
```

事实导入和统计分析不依赖 Pi Agent；Pi 只用于可选解释和复核。

## Stage 6: Verify Results

验证以下对象和结果：

- `LotteryDraw`：开奖事实没有被覆盖；
- `DataImport`：保存最近一次导入摘要；
- `DataImportBatch`：保存 source_hash、行数、导入数、跳过数和冲突数；
- `DataImportConflict`：保存相同期号但事实不同的冲突；
- `AnalysisRun`：保存统计窗口和结果；
- `BacktestRun`：保存训练窗口、测试数量和指标；
- `Prediction`：必须关联到对应分析或回测运行。

相同期号且开奖日期或号码不同，必须报告冲突并保留原始 `LotteryDraw`，严禁静默覆盖。

## Stage 7: Diagnose

按以下顺序排查：

1. 业务包目录和 manifest 路径是否正确；
2. `domain-pack validate` 是否通过；
3. `project_id` 是否等于业务包 manifest.id；
4. 数据库是否是目标本地 SQLite/libSQL 文件；
5. CSV 表头、期号、日期和号码范围是否符合彩种；
6. `source_hash` 是否变化；
7. 是否已有相同期号的冲突事实；
8. 插件能力是否已经在 `PluginCatalog` 注册；
9. 是否误把 Pi 或 dsh 当成导入前置条件。

## 当前部署边界

当前 `LotteryPlugin` 由宿主的 `PluginCatalog::with_builtins` 静态注册；业务包目录、本体和 bootstrap 可以独立校验/安装，但插件代码还不是运行时动态加载的独立二进制。不要声称当前已经支持热插拔插件。

## Stage 8: Disable or Remove

当前版本支持 `validate` 和 `install`，不提供自动卸载命令。不得手工删除数据库对象来模拟卸载。需要停用业务包时：

- 停止相关自动任务；
- 停止调用业务插件；
- 保留历史事实和分析结果；
- 记录停用原因；
- 后续由显式 uninstall/migration 流程处理项目和对象清理。

## 业务包结构示例

```text
domain-packs/lottery-analysis/
├── manifest.json
├── ontology.odl
├── bootstrap.json
└── scenarios/
```

业务包和宿主的关系：

```text
宿主：通用本体运行时、项目、存储、权限、审计、调度
业务包：领域本体、初始事实、领域插件、分析策略、业务 UI
```

## 完成标准

一个业务包操作只有在以下条件全部满足时才算完成：

- 业务包校验通过；
- 安装过程可重复且幂等；
- 内容冲突不会被覆盖；
- 所有写入经过宿主世界命令；
- 插件能力经过统一目录调用；
- 结果写入本体对象并可追溯；
- 失败时给出明确错误，不通过回退逻辑掩盖问题。


## Phase 2.4：配置业务包运行能力

读取运行能力：

```text
GET /api/v1/domain-packs/<pack-id>/operations
```

更新运行配置（仅 Developer）：

```text
PUT /api/v1/domain-packs/<pack-id>/runtime
```

请求至少包含：

```json
{
  "enabled": true,
  "source_path": "/path/to/dlt.csv",
  "watch_enabled": true,
  "poll_interval_seconds": 30,
  "auto_sync": true,
  "auto_analyze": true,
  "auto_backtest": true,
  "auto_review": false,
  "strategy_ids": ["statistics"]
}
```

启停使用：

```text
POST /api/v1/domain-packs/<pack-id>/enable
POST /api/v1/domain-packs/<pack-id>/disable
```

配置只描述运行意图，不能把未实现的 watcher、队列或 Pi Agent 调度当作已经执行。自动任务真正执行前必须确认业务包已安装、数据源路径可读、策略已经注册，并分别保存导入、分析和回测证据。

查看策略注册表：

```text
GET /api/v1/domain-packs/<pack-id>/strategies
```

策略必须按 `family` 隔离回测；没有回测证据或低于随机基线的策略不能发布。
