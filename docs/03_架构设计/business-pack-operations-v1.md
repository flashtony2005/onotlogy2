# 业务包说明与操作规范 v1

## 1. 定位

业务包（Domain Pack）是 OpenAIP-OS 面向一个业务领域的可安装模块。它不是另一套数据库，也不是把公司业务硬编码进宿主，而是把以下内容组合成一个可校验、可安装、可审计的目录：

```text
业务包
├── manifest.json       包身份、版本和入口文件
├── ontology.odl       业务概念、属性、关系和动作
├── bootstrap.json     公司/业务初始设置与事实
├── scenarios/         验收场景（可选）
├── plugin/             业务能力实现（可选）
└── ui/                 业务页面资源（可选）
```

本体论宿主和业务包是分工合作关系：

| 组件 | 负责内容 |
|---|---|
| 宿主 Runtime | 项目、SQLite/libSQL、WorldCommand、权限、审计、插件目录、任务调度 |
| 业务包 | 领域本体、初始设置、领域解析、统计、推导、回测、业务页面 |
| 插件 | 把业务操作实现为可授权的 capability |
| Pi/dsh | 可选的解释、研究或外部执行能力，不作为基础数据导入前置依赖 |

## 2. 彩票业务包

当前彩票业务包：

```text
domain-packs/lottery-analysis/
├── manifest.json
├── ontology.odl
└── bootstrap.json
```

彩票能力插件位于：

```text
/Users/lushaohui/project/ontology2/src/lottery_plugin.rs
```

彩票领域实现位于：

```text
/Users/lushaohui/project/ontology2/src/lottery.rs
/Users/lushaohui/project/ontology2/src/lottery/source.rs
```

对宿主公开的能力：

```text
lottery.import
lottery.data.sync
lottery.analyze
lottery.backtest
lottery.review
```

其中：

- `lottery.data.sync`：计算源文件哈希、解析 CSV、生成导入计划、增量写入和记录冲突；
- `lottery.analyze`：计算确定性的统计特征；
- `lottery.backtest`：按训练窗口执行滚动回测；
- `lottery.review`：可选调用 Pi Agent 生成解释；
- 统计、启发式、玄学等策略必须独立注册、独立回测，不允许混合评分。

## 业务包与 Skill 管理界面

网页端“业务包”页面调用宿主 API 扫描 `ONTOLOGY_DOMAIN_PACKS_DIR` 指向的目录，并展示：

- 业务包名称、版本和校验状态；
- 已安装/可安装状态；
- 本体类型、Bootstrap 对象和关系数量；
- `manifest.skills` 声明的 Skill 及其 Registry 状态。

Developer 可以在页面安装有效业务包；Operator 及以上可以查看，但不能安装。安装后页面重新扫描状态，重复安装只返回跳过计数。单独的“Skill 管理”页面继续管理 Skill Registry，业务包页面只显示声明式关联。

接口约定：

```text
GET  /api/v1/domain-packs
POST /api/v1/domain-packs/:id/install
```

## 3. 标准操作流程

### 3.1 校验

```bash
oaip --db ontology.db domain-pack validate domain-packs/lottery-analysis
```

校验包括：

- manifest 文件引用和路径安全；
- ODL 语法；
- bootstrap 对象类型和字段；
- 关系引用；
- 业务包 id、名称和版本。

### 3.2 安装

```bash
oaip --db ontology.db domain-pack install domain-packs/lottery-analysis
```

安装会：

1. 创建或确认 `lottery-analysis` 项目；
2. 写入本体定义；
3. 通过 `WorldCommand` 导入 bootstrap 对象和关系；
4. 产生可审计的世界事件；
5. 对完全相同的内容执行跳过；
6. 拒绝覆盖内容不同的已有事实。

同一业务包重复安装应该是幂等的。若 ODL 内容变化，不使用同一个 id 静默覆盖，应发布新的业务包 id 或显式迁移版本。

### 3.3 启动

```bash
oaip --db ontology.db serve \
  --domain-pack domain-packs/lottery-analysis \
  --host 127.0.0.1 \
  --port 18420
```

macOS 本地版可以把业务包放入 `.app/Contents/Resources`，通过 `ONTOLOGY_DOMAIN_PACK` 指定。数据库仍保存在用户目录的 `ontology.db`，不做云端同步。

### 3.4 彩票数据同步与分析

```bash
oaip --db ontology.db lottery import \
  --game dlt \
  --project lottery-analysis \
  data/dlt.csv

oaip --db ontology.db lottery analyze \
  --game dlt \
  --window 50 \
  --project lottery-analysis
oaip --db ontology.db lottery backtest \
  --game dlt \
  --train-window 50 \
  --test-count 30 \
  --project lottery-analysis
```

自动流水线目标：

```text
文件变化
  → lottery.data.sync
  → imported > 0
  → lottery.analyze
  → lottery.backtest
  → 可选 lottery.review
```

插件执行单次能力；文件监测、去抖、排队、重试、超时和取消由宿主负责。

## 当前部署边界

当前业务包的数据部分可以独立校验和安装；彩票插件能力通过宿主 `PluginCatalog::with_builtins` 静态注册。也就是说，当前已经实现逻辑解耦和统一能力入口，但尚未实现插件代码的运行时热加载、热卸载和独立二进制分发。

## 4. 安装、启用、停用、卸载

当前已经支持：

```text
validate
install
```

当前还没有自动 `uninstall` 命令。因此停用业务包不能通过手工删表或删除对象完成。正确做法是：

1. 停止业务包相关自动任务；
2. 停止调用业务插件；
3. 保留历史事实、导入批次、分析和回测记录；
4. 后续通过显式卸载/迁移流程处理注册信息和对象清理。

未来生命周期：

```text
发现 → 校验 → 安装 → 启用 → 运行 → 停用 → 升级/卸载
```

## 5. 操作验收标准

- 同一业务包重复安装不产生重复对象和关系；
- ODL 内容变化不会静默覆盖已有项目；
- 同一期号但号码或日期不同会产生 `DataImportConflict`；
- 原始 `LotteryDraw` 不被冲突 CSV 覆盖；
- 每个导入批次保存 `source_hash`、导入数、跳过数和冲突数；
- CLI、HTTP、网页和自动任务都通过 `PluginCatalog` 调用能力；
- Pi Agent 不阻塞基础导入、统计和回测；
- 所有事实变更经过 `WorldCommand` 并可审计。

## 6. 相关 Skill

操作型 Skill：

```text
skills/business-pack-operations/SKILL.md
```

它用于指导 Agent 完成业务包识别、校验、安装、启动、彩票同步、分析、排错和停用操作，并明确当前版本不支持自动卸载。


## Phase 2.4：运行配置与策略注册表

网页端业务包卡片提供以下配置：

1. 启用/停用业务包；
2. CSV 数据源路径与轮询间隔；
3. 文件监测、自动同步、自动统计、自动回测和可选 Pi 复核开关；
4. 自动任务允许的策略集合；
5. Plugin 关联能力；
6. 策略版本、参数、训练窗口、回测次数、随机基线和发布资格。

配置保存后进入 `DomainPackRuntime` 世界对象并写入审计链。当前“自动任务”开关只是运行意图配置，实际 watcher/worker 调度已在 Phase 2.5 接入；网页端可直接观察队列、手动触发、扫描和取消任务。

策略发布门槛：策略必须启用、至少有一次回测，并且红球和蓝球指标都不低于对应随机基线；这只是最小门槛，不代表彩票具有可预测性。玄学实验策略必须保持独立证据链，不得与统计策略混合评分。


## Phase 2.5.6：网页端自动运行与真实数据验证

业务包页面的“自动运行与真实数据验证”区域只调用宿主自动化 API，不在浏览器读取 CSV 或实现彩票算法：


```text
立即运行 / 扫描数据源 / 单步操作
  → HTTP API
  → AutomationEngine
  → 持久化任务队列
  → lottery Plugin capability
  → AutomationRun 本体对象与审计事件
```

支持的页面操作：

```text
GET  /api/v1/automation/jobs
GET  /api/v1/automation/jobs/:id
POST /api/v1/automation/jobs/:id/cancel
POST /api/v1/domain-packs/:id/run
POST /api/v1/domain-packs/:id/scan
POST /api/v1/domain-packs/:id/operations/:operation/run
```

页面按业务包过滤任务，展示任务状态、策略、输入版本、数据行数、新增期号、回测摘要和失败原因；queued/running 任务会自动轮询，Pi 复核失败不会覆盖统计和回测结果。

真实数据验证脚本：

```bash
ONTOLOGY_LOTTERY_SOURCE_ROOT=/Users/lushaohui/project/test灯 \
  scripts/phase2.5.6-real-data.sh
```

插件仍然采用显式目录权限。服务端启动时必须把用户授权的数据目录加入 ONTOLOGY_LOTTERY_SOURCE_ROOT，否则插件会 fail-closed 拒绝读取文件；这保证网页运行配置不能绕过文件权限边界。
