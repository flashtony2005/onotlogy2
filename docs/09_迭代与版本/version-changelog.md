# 版本变更日志
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-002 |
| 所属模块 | OpenAIP-OS-Version |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
按版本倒序记录新增/修复/优化，作为上线核对依据。

## 2. 业务/技术背景
被 deploy-steps.md 与 rollback-plan.md 引用。

## 3. 核心规则 & 约束（重点）
- [强制] 按版本倒序，固定三级分类：新增功能 / Bug 修复 / 优化调整。
- [强制] 版本号规则：Vx.y.z（主版本。功能迭代。Bug 修复）。

## Phase 3：真实部署与运维生产化（2026-08-18）

### 新增功能

1. S3a–S3c：Dockerfile、Compose、`/healthz`、`/readyz`、`/metrics` 和优雅停机顺序。
2. S3e：真实 release binary 部署冒烟 `make ops-smoke`，并接入 CI；冒烟在停机后继续
   验证同一数据库的 backup/restore 运维命令和审计读取。
3. S3d：`make ops-rehearsal` 真实执行旧/新 binary 的 schema 升级、备份恢复、降级拒绝和
   升级前快照回滚；旧 binary 直读新 schema 明确返回 `WB2101`。
4. S3d：`make release-manifest` 生成不可误认为正式 release 的候选/正式证据清单，记录
   binary SHA-256、schema 版本、Git commit 和容器镜像标签。
5. Phase 3 收尾：`make container-smoke` 与 `make compose-smoke` 真实构建并启动容器，
   验证运维探针、Compose healthcheck、优雅停止和临时 volume 清理；返回码 `2` 明确表示
   Docker 环境不可用，不伪造通过。
6. CI 上传 `phase3-deployment-evidence`，保留 `release-manifest.json` 与四条 Phase 3 smoke
   日志作为部署证据入口。
7. CI 使用 `HEAD^` 构建真实旧 binary 并执行 `make ops-rehearsal`，将 schema 升级、备份恢复、
   降级拒绝和升级前快照回滚纳入发布门禁。

### 优化调整

- 将二进制回滚与数据库回滚拆成两个明确步骤，不保留“旧程序兼容新 schema”的回退路径。
- 将实测 RTO 与生产目标分开记录，避免把小型临时库的结果宣称为生产 SLA。
- 将 `/metrics` 的 run/trace 总量改为 SQL 聚合计数，避免高频抓取时加载完整业务与审计正文。

## 4. 详细设计/内容
> 产品版本采用 V0.1~V0.6 路线（见 sprint-plan.md），以下按产品里程碑记录，固定三级分类：新增功能 / Bug 修复 / 优化调整。

## V0.14.1（发布证据链收口，2026-08-26）

### Bug 修复
1. 身份投影合成类改用基于 schema 内容指纹的 `urn:oaip:internal:identity-v1:*` 命名空间，
   消除固定 `urn:oaip:identity:*` 与用户本体冲突的风险；新增 N=0/1 和旧名不别名回归测试。

### 加固
2. `reasoner-scale` SLA 明确拆分推理阶段与全管线耗时；CI 使用 3000ms 共享 runner 预算，
   本机严格红线保持 `REASONER_SCALE_SLA_MS=1000`。实测 5k 推理阶段 233.0ms。
3. Golden 场景显式提示当前使用确定性 process executor；新增 opt-in 的真实 DSH 集成 CI job，
   并对空的 `OAIP_DSH_COMMAND` 快速失败。
4. Rust workspace、UI 和 macOS bundle 版本统一为 `0.14.1`，发布 manifest 可绑定最终提交与 tag。

## V2 架构落地（可执行本体内核 + V2 认知闭环 + 投影体系，2026-08-15 ~ 08-17）
> 架构版本 V2（区别于产品版本号 V0.x）。本周期完成 V1 planner 退役、演化引擎归一，
> 并落地 World Model 投影体系（Phase 5–8）与 Agent Fabric 审批 / 异步 / 增量三阶段。

### 新增
1. **M1 退役 V1 planner**：删 `src/planner/`（747 行）、`action.rs`、`RuntimeKernel` 门面、V1 三表（wb_run/wb_memory/wb_approval）；CLI `agent run` 与 server `/run` 改走 V2 cognitive loop；v0_3_* 测试退役，由 `tests/v2_parity/` 契约测试覆盖（G1–G10 钉绿）
2. **M2 演化引擎归一**：`evolution2`（Success/Failure 双通道 + Gate）= 经验层；`rule/evolution.rs` = 行为规则层；`skill` mutation = 资产层；删旧 `evolution.rs` + 三表 + `/evolution` 端点
3. **Phase 5–8 投影体系（projection-v1.md）**：P1 纯函数（Event Log→Kernel 同态）/ P2 幂等（applied 水位）/ P4 确定性（normalize_ts 微秒→秒轴）/ P5 写入路径同事务 / P6 沟通黑匣子入世界 / P7 裸写不可追溯；`Projection` 模块 + `project_from_storage`
4. **T1 Agent Fabric 审批接入（已合 master）**：Bridge 挂起态 + `fabric_resolve` 复用 `/run/:run_id/approval`；HTTP `facade_fabric_submit` 返回挂起态、pending 走审批流。bridge（`b6087fc`）+ UI GovernancePanel 人工裁决台（`07430da`）均已 fast-forward 合入 master。
5. **T2a Agent Fabric Cancel MVP（已合 master `a03f4ca`）**：`Bridge.cancel` + 取消标记集 + `ProcessAdapter` 子进程 kill + `POST /facade/fabric/:id/cancel` 端点 + `tc_f9` 端到端契约（含 submit/cancel 死锁修复）。
6. **T2b-1 实时广播**（已合入 master `2ec5259`）：`StorageWorldSink` 加 `with_broadcast(event_tx)` ——落库成功后旁路扇出到 WS `/stream`，修复 `event_tx` 零发布者之洞（此前 WS 只有历史回放、无实时）；契约测试 `tc_storage_world_sink_broadcasts_to_event_tx` 钉绿；GovernancePanel 免轮询。
7. **T2b-2 异步契约**（已合入 master）：`Bridge::submit` 改 `(立即回复, Option<RunJob>)` + `sessions` 改 `Arc<Mutex<SessionManager>>`；`finish` 重构为 `execute(RunJob)`（保留 adapter killed 互斥终态判定）/ `run_in_background`；`FabricService` 持 tokio Handle，submit 内 `spawn_blocking` 后台执行子进程——立即返 `running` + `correlation_id`（fire-and-forget）；CLI 提交后轮询审计事件拿终局；tc_f4/f5/f7/f9/f10 + tc_g3/tc_p3 全部改异步契约断言。
7. **P3 增量投影（watermark）**：世界关系 + watermark 持久化于 `wb_meta`（JSON blob，零 schema 迁移）；`after_id` 只应用新事件，跨 run 不再全量重放；`tc_pj9` 钉增量终局==全量重放

### 已知缺口（已修复，见 tech-debt-reduction-plan.md 与 fix/survivor-gaps）

> 以下三项原于 V1.3 诚实声明为「仍存缺口」，已于 `fix/survivor-gaps` 全部修复并钉测试：

1. ~~存储正确性~~ → 已修复：`link()` 加 `uq_edge_active` 部分唯一索引（活跃边不重复，并发双插归 WB2007）；`update()` 加 `WHERE version=?` 乐观锁 + `StorageError::Conflict`（并发后写覆盖前写归 Conflict，重试最新版本成功）。
2. ~~投影类型丢失~~ → 已修复：投影写 `Relation.metadata` 保留类型化值（数值不文本化），`EntityView` 出边优先取 `metadata`，`tc_o4` 钉 `amount=100 → Number(100)`。
3. ~~Facade 契约方法未暴露~~ → 已修复：`fabric.cancel`/`fabric.stream`/`capability.discover`/`session.get` 落地为 `FabricService` + `Facade` 固有方法（facade-v1.md V1.4）。

## V0.14.0（S1/S2/S4 加固 + D7 性能基线 · 超时可配 + 孤儿 job 回收 + 并发事务修复，2026-08-22）
> 在 B1（shell-exec 提供方）门禁已绿基础上叠加的纯加固增量，风险最低、收益直接。
> 三项均不引入新行为语义，只补边界与收口；`make check` 全绿（含 `tc_f13`），已合 master。

### 加固
1. **S1 异步轮询超时上限可配**：CLI `oaip facade fabric submit` 新增 `--timeout <SECS>`（默认 30），轮询窗口从硬编码 `300×100ms` 改为 `timeout×10` 轮；`0` = 提交后立即返回不等待。消除「长任务必 30s 超时退出杀后台」的刚性约束（`src/bin/oaip/main.rs` + `facade.rs`）。
2. **S2 孤儿后台 job 回收**：服务 SIGINT/SIGTERM 优雅关闭时回收所有 adapter 运行中的后台子进程——`Facade::shutdown → FabricService::shutdown → Bridge::shutdown → AgentAdapter::shutdown`（`ProcessAdapter` 覆写 drain `running` 子进程并对**整个进程组**发 SIGKILL；spawn 时 `process_group(0)` 把子进程设为进程组 leader，`/bin/kill -9 -<pgid>` 即 killpg，连 `sh -c` 派生的孙子进程一并回收）。与 `cancel` 同机制（`kill_child_tree`）。`AgentAdapter` trait 加 `shutdown` 默认 no-op；`serve_with_facade` 经 `axum::serve(...).with_graceful_shutdown` 接线（Cargo.toml 新增 tokio `signal` feature）。
3. **S4 broadcast lag 声明**：broadcast 缓冲 1024 → WS 实时为 best-effort（慢客户端/生产风暴可能丢缓冲外实时事件，仅实时缺口）；但 WS 断线重连走 `after_id` 游标从存储（权威）补齐，重连不丢数据、最终一致。已在 `src/facade.rs` 频道创建处与本文档声明语义。

### 说明
4. S2 回收用 SIGKILL（与 `cancel` 一致，零新依赖）；若需严格 SIGTERM 优雅退出需引入 `libc`/`nix` 发信号——可单列增量。
5. `shutdown` 专项回归 `tc_f13_shutdown_kills_running_child`（`tests/fabric_v1.rs`）：真实 `/bin/sh -c "touch …; sleep 30"` 跑长 `sleep` → 进 `running` map → 调 `shutdown` → 子进程被杀、stream 提前 EOF 返回且终态非 `completed`；marker 文件就绪信号 + `mpsc::recv_timeout` fail-fast，确定性且不挂死。**该测试首跑即暴露真 bug**：原 `Child::kill()` 只杀直接子进程 `/bin/sh`，孙子 `sleep` 继承 stdout 管道继续持有 → EOF 不来 → `stream` 永久阻塞、且 shutdown 遗留孤儿；已改为进程组回收修复（零新依赖，复用 `/bin/kill`）。

### Bug 修复
6. **共享连接并发事务冲突**：真实 HTTP 并发负载暴露同一个 libSQL 逻辑连接可能同时进入多个事务，
   返回 `cannot start a transaction within a transaction`。现在进程内所有写事务入口共享一把异步写锁，
   审计链、world、关系和 automation 事务按序进入；新增 24 路并发 world command 回归测试。

### 性能基线
8. **D7 核心路径基线**：新增 Criterion 基准 `make bench`，固定 ODL 编译（24 objects）、投影重放
   （1,000 events）、世界查询（500 objects）和审计链写入四条负载；首轮数字与环境入档
   `docs/03_架构设计/performance-baseline.md`。这是核心路径微基准，HTTP/WS P99 由后续负载测试补齐。
9. **HTTP/WS 负载基线**：新增 `make load-baseline`，固定 10 次 submit/approve、并发 5、
   3 个 WebSocket 客户端；修复后 10/10 成功，P99 17.986ms，吞吐 282.911 rps。数字是
   本机开发环境基线，不是生产 SLA。
10. **版本对齐**：Rust workspace、UI 和 macOS bundle 版本统一为 `0.14.0`；MCP clientInfo 改用编译期
   Cargo 版本，消除硬编码漂移。

## V0.13.0（shell.execute 能力诚实 · 专用 shell-exec 提供方，2026-08-18）
> B1 决策：harness（pi/dsh/deepseek）撤下 `shell.execute` 假广告，执行归专用 `shell-exec` worker——
> 复用 tests 已验证的 `/bin/sh` 通道。E2E 曾暴露 `run_shell` 经能力路由落到 dsh（LLM harness 不认
> `-c`）→ 真实执行链必 failed 的死穴；本版本根治，agent「执行」半边首次真能跑命令。

### Bug 修复（能力市场诚实 + 执行链死穴）
1. **harness 假广告撤下**：`pi_worker` / `dsh_worker` / `deepseek_worker` 的 manifest 移除 `shell.execute`——它们不真提供 shell，「用自身工具链做任务」是误用。路由确定性随之提升（消除同优先级 HashMap 插入序竞态）。
2. **专用 shell-exec 提供方**：新增 `shell_worker`（manifest 只声明 `shell.execute`，命令默认 `/bin/sh`）；`ProcessAdapter` 加 `shell: bool` 字段与 `shell_args_of`（把 payload 收敛成 `sh -c "<cmd>"`：兼容 harness 透传的 `-c "<cmd>"` 形态与裸 args 形态）；新增 `shell_adapter` 工厂。
3. **接线（两处对称）**：`src/server/mod.rs::build_facade` 与 `src/bin/oaip/facade.rs::cli_facade` 的适配器数组均加 `shell_adapter(None)`。`run_shell` → `shell.execute` → 真 `/bin/sh` 执行（仍 `mark_high_risk` → 人工审批闸门 + 审计 + 可取消兜底）。

### 测试
4. 新契约 `tc_sf6_run_shell_executes_via_shell_exec_provider`（server_facade_v1）：submit run_shell → 高风险管理挂起 → approve → 真跑 /bin/sh → 终态 `completed` + `sender=="shell"`（B1 判别器，修复前落 dsh 必 failed）+ 世界投影 `fabric.status=completed`。
5. 同步修正假广告断言：`tc_f6`（fabric_v1）改为「dsh 含 agent.task 且**不含** shell.execute」；`dsh_worker_manifest_*` / `pi_worker_manifest_*` 两处 manifest 测试去除 `supports("shell.execute")` 断言。

## V0.12.0（T2b 异步契约 review 收口 · fabric 实时 + CLI 轮询 + 取消全窗口，2026-08-18）
> T2b（异步契约：submit 立即返 running + RunJob 后台 spawn_blocking）已在 master（`1d91c38`）。
> 经独立深度 review 发现 2 Critical + 2 High 接线/语义缺口，本版本一次性收口（commit `0b8f7ba`），
> 全量门禁（fmt/clippy/test/unwrap-budget）通过，T2「异步 + 实时」交付承诺在真实运行下成立。

### Bug 修复（review 4 处洞）
1. **C1 fabric 事件未接 WS 实时广播**：Facade 自建 `event_tx` 广播频道，`StorageWorldSink` 落库后 `with_broadcast` 扇出，server `AppState` 复用同一频道（不再另建收不全的频道）。GovernancePanel 免轮询承诺成立（原仅认知 Runner 路径接上）。
2. **C2 CLI 轮询必然漏终局**：`AuditService::query` 审计域默认 `newest_first=true`（DESC LIMIT 500 取最新 500 条）；`list_events` 据此条件式排序。`TraceQuery` 新增 `newest_first` 字段；WS 断线补齐游标仍 ASC（`after_id`）不受影响。终局 fabric 事件（最新）不再被 ASC 窗口截断。
3. **H1 run_in_background 吞错**：`execute` 中 `adapter.stream` 失败时先发 `fabric.task_failed`（带错误文本）+ 会话终态，再返 Err；`run_in_background` 的 `let _ = e` 不再导致 correlation 永久悬挂、CLI 误导「超时未终局」。
4. **H2 early-cancel 竞态**：`Bridge` 新增 `cancel_requested: Mutex<HashSet>` 标记集，`cancel` 写入、`execute` 在 spawn 子进程前检查命中即发 `fabric.task_cancelled` 不执行；与 late-cancel（adapter `killed` mutex）互补，覆盖 submit 返 running 与子进程进 running map 之间的窗口。

### 测试
5. 补三处回归契约（均合入）：`tc_f11`（H1 stream 失败发 `fabric.task_failed`）、`tc_f12`（H2 不等 running map 即 cancel 生效）、`list_events_newest_first_exposes_terminal_event_over_limit`（C2 倒序取到终局）。
6. **C1 完整性补刀（第 5 处洞，E2E 实证 + 契约钉死）**：C1 广播接上后 WS 过滤层（run_id/agent_id 精确匹配）仍丢弃双 None 的 fabric.* 事件——真实 E2E（服务 + WS 订阅）收到 0 条。修复：`ws_event_matches` 放行「双 None + fabric. 前缀」的世界级事实（`src/server/trace.rs`，缓冲与增量两处同源）；新契约 `tc_sf5_fabric_events_reach_ws_realtime` 红→绿。

## V0.11.0（治理闭环 + 实时可观察 · M0–M8 关闭，2026-08-18）
> 本周三件事落地：① T1 合入 master；② T2a Cancel MVP 端到端垂直切片；③ 文档收口宣布技术债 M0–M8 正式关闭。
> 至此项目进入「治理闭环 + 实时可观察」完整状态。

### 新增
1. **T1 Agent Fabric 审批接入（完整闭环，已合 master）**：T1 bridge（`b6087fc`）与 T1 UI GovernancePanel 人工裁决台（`07430da`）均已 fast-forward 合入 master；HTTP 挂起态审批端到端跑通（`facade_fabric_submit` 返回挂起态、pending 走 `/run/:run_id/approval` 审批流）。
2. **T2a Agent Fabric Cancel MVP（已合 master `a03f4ca`）**：`Bridge.cancel` + 取消标记集（`cancelled: Mutex<HashSet>`）+ `ProcessAdapter` 子进程 kill + `POST /api/v1/facade/fabric/:id/cancel` 端点 + `tc_f9` 端到端契约（提交→取消→杀子进程→发 `fabric.task_cancelled` 事件，无 `fabric.task_failed`）；含死锁修复：`submit` 持 `sessions` 锁阻塞子进程期间，`cancel` 不再持 `sessions` 锁、改经 `bridge.cancel` 并发抢占 kill，否则 cancel 永不抢占。

### 收口
3. **技术债 M0–M8 全部正式关闭**（`tech-debt-reduction-plan.md`）：M0 基线冻结 / M1 退役 V1 planner / M2 演化引擎归一 / **M3 概念收敛**（world_exec 改名 + ontology→compose 并入 + 三 registry 按重叠度保留独立，均经代码核查）/ M4 workspace crates 拆分 / M5 巨文件拆分 / M6 unwrap 清零 / M7 文档重建 / M8 流程文档激活。
4. 项目状态：进入「治理闭环 + 实时可观察」完整状态（Agent Fabric 审批 + 取消闭环、V2 内核 + 投影体系、技术债清零）。

## V0.10.0（模块化重构 + 认证收口 + 测试基建，2026-08-14）
### 优化
1. 长文件模块化：planner(1855行)/storage(1662)/server(1371) 拆目录模块共 22 文件；odl 去重；App.tsx 拆 4 组件
2. V0.7 认证收口：恢复 `/auth/me` + 静态 fallback；UI 登录链路（token sessionStorage + Bearer + WS `?token=`）
3. 安全：/graph /trace 角色钳制、登录限流（5 次锁 5 分钟）、会话令牌 SHA-256 哈希、LIKE 通配符转义
4. 正确性：update 原子递增、link 事务化、WS 订阅-缓冲-补历史、审批超时落库、知识按 (类型,动作) 键控
5. 测试：v0_3(3084行) 按领域拆 5 文件；tests/common 统一 FakeEngine/http_call/ODL 常量（4 文件复用）
6. 工程：版本号统一 0.9.0；pre-commit 质量门禁（.githooks，fmt/clippy/test）

## V0.9.0（ToolBackend 缝 + 流水线闸门，2026-08-14）
### 新增
1. `ToolBackend` / `ActionHook`：出世界执行与 pre/post 闸门；默认 `NullBackend` 不影响 CRM
2. Planner `with_backend` / `with_hook`；执行序 pre → backend → ActionEngine → post
3. `examples/code.odl`：File.lint 创建 Finding（参数由后端回填）

## V0.8.7（目标绑定作用对象，2026-08-14）
### 新增
1. `resolve_target_ids`：目标文本点名优先，其次解析 refs，否则不限制
2. `select_candidates` / 重规划按范围收窄；派生任务（`{id}_*` / `customer`）跟随源对象
3. 金额扫描剔除已匹配对象 id；T1–T4 覆盖点名退款/挽留与不点名兼容

## V0.8.6（目标参数抽取，2026-08-14）
### 新增
1. Planner 从 goal/Intent 抽取动作参数，只填 ODL `params:` 声明字段
2. 具名 `amount=80` 优先，否则 Int/Float 取文本最后一个数字；`refund this customer 500` 会改 balance

## V0.8.5（观察层任务视图，2026-08-14）
### 新增
1. `GET /api/v1/wb/agent/tasks` 列出 `RetentionTask`（id/title/customer/status/priority，Pending 优先）
2. 观察 UI 增加 Tasks 标签，Action/Planning 事件实时刷新
3. `oaip object list` 对带 `status` 的对象渲染 `id title status`

## V0.8.4（Intent 过滤动作，2026-08-14）
### 新增
1. Planner 用用户目标（优先）+ 解析 Intent 限制动作族：任务/挽留 → retain/start/complete；退款 → refund；关户 → closeAccount
2. 认不出意图时不过滤，只靠前置条件（兼容旧闭环）

## V0.8.3（一轮目标推到任务稳态，2026-08-14）
### 新增
1. Planner 执行后重快照、重规划（最多 8 波），本轮已执行的 `type:action:target` 不再重复
2. 一条 `agent run` 即可走完 Pending → InProgress → Completed（不引入独立 Workflow）

## V0.8.2（Planner 按状态推进任务，2026-08-14）
### 优化
1. `build_plan` 只纳入当前快照满足前置条件的动作（Pending→start，InProgress→complete；缺参 refund 不误触发）
2. `self.field` 解析兼容世界快照的 `data` 嵌套，状态机前置条件可对真实对象字段生效

## V0.8.1（本体论任务管理，2026-08-13）
### 新增
1. `RetentionTask` 升为一等本体：属性 + `start` / `complete` 状态机（Pending → InProgress → Completed）
2. `Customer.retain` 创建任务时写入 `status=Pending`，供 Planner 在下一轮按本体动作管理任务

## V0.8.0（GraphRAG 基础，2026-08-13）
### 新增
1. `get_object_graph` 方法（storage.rs）——返回对象-关系图
2. `/api/v1/wb/graph` 路由（server.rs）——暴露对象-关系图
3. V0.8 正式起步（GraphRAG 基础，无需新表）

## V0.7.0（用户与账号管理，已实现）
### 新增
1. V0.7 完整用户/账号管理：`oaip user list` / `create` / `create-admin` / `delete`（复用 auth.rs + storage.rs）
2. 账号列表、创建（默认 Developer）、删除（级联会话）
3. 更新 CLI 帮助与 `user` 子命令

## V0.6.1（工程化清理，2026-08-13）
### 优化
1. 移除 Planner legacy 硬编码挽留计划；统一经 ODL `ActionRegistry` 声明式执行（默认 `examples/crm.odl`）
2. 文档品牌统一为 OpenAIP-OS（去除 WorkBuddy 模块前缀）
3. Mutex poison 恢复、审批路径避免 panic；补 README、GitHub Actions CI、Makefile 工具链 PATH
4. 测试全部改为 `Planner::with_actions` + retain 注册表；`cargo test --all-targets` 全绿

## V0.6.0（Evolution AutoApprove，已实现）
### 新增
1. REQ-018：5 道闸门 + 默认关闭 + `wb_approval_audit` 可审计 + 可撤销 AutoApprove
2. CLI/server `--auto-approve-evolution` 与 `POST .../auto-approve` API

## V0.5.0（Agent 自治，已实现）
### 新增
1. Memory 沉淀闭环（REQ-016）与观察 UI Memory 视图
2. Evolution Engine MVP（REQ-017，Learn≠Permission）

## V0.4.0（Action & Policy，已实现）
### 新增
1. 声明式 Action Engine（REQ-014）+ Policy 前置条件/审批闸门（REQ-015）
2. Console / Server 挂起式 Human Approval + HTTP 裁决端点

## V0.3.0（MVP，已实现）
### 新增
1. Agent 闭环：Pi 作 Cognitive Engine + OpenAIP Planner
2. 三层观察 UI + WebSocket 实时轨迹流
3. `TraceEvent(kind=cognitive)` + Trace API
### 优化
1. 统一 `TraceEvent` 模型（world / cognitive 同源）

## V0.2.0（已实现）
### 新增
1. 关系边 `wb_relation_edge`，支撑图查询
2. 领域事件扩展 world 事件语义

## V0.1.0（已实现）
### 新增
1. 极简 Ontology Kernel：`.odl → Compiler → IR → Runtime → Turso → Object CRUD`
2. 统一对象存储 + Event Bus + CLI 观察

## 5. 风险点 & 注意事项
- changelog 与实际发布不符会使回滚失准。

## 6. 关联文档
- [../08_部署运维/deploy-steps.md](../08_部署运维/deploy-steps.md)
- [../08_部署运维/rollback-plan.md](../08_部署运维/rollback-plan.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 重写 changelog 为 OpenAIP-OS V0.1~V0.5 产品路线 |
| V1.2 | lushaohui | 2026-08-13 | 同步 V0.1~V0.6 已实现状态；记录 V0.6.1 工程化清理 |
| V1.3 | lushaohui | 2026-08-18 | 补 V2 架构落地条目（M1/M2 退役 V1 planner + 演化归一、Phase 5–8 投影、T1/T2b/P3），并诚实声明存储/投影/Facade 三项仍存缺口 |
| V1.4 | lushaohui | 2026-08-18 | 修复 V1.3 声明三项缺口（fix/survivor-gaps）：① `link()` 加 `uq_edge_active` 部分唯一索引杜绝并发双插、`update()` 加 `WHERE version` 乐观锁 + `Conflict`；② 投影写 `Relation.metadata` 保留数值类型、`EntityView` 出边优先取 metadata；③ Facade 暴露 `fabric.cancel/stream`/`capability.discover`/`session.get`（facade-v1.md V1.4）；全部钉测试（storage 并发 link/update、tc_o4、facade/session 单测） |
| V1.5 | lushaohui | 2026-08-18 | 新增 V0.11.0 条目：宣布技术债 M0–M8 正式关闭 + 项目进入「治理闭环 + 实时可观察」；T1（bridge+UI）与 T2a Cancel MVP 已合 master 收口记录；V2 架构落地章节补 T1 合入标记、新增 T2a、修正 T2b 误挂 tc_f9 |
| V1.6 | lushaohui | 2026-08-26 | 新增 V0.14.1 发布收口条目：身份命名空间隔离、SLA 口径与 CI 预算分层、Golden 执行器显式化、跨端版本对齐 |
