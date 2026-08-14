# 项目开发约束总规则
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-01-002 |
| 所属模块 | OpenAIP-OS-Overview |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
本文档是项目仓库 Markdown 文档体系的**开发约束总规则**，定义目录结构、命名、内容模板、专项约束与评审流程，所有团队成员必须遵守，代码评审以此作为卡点。

## 2. 业务/技术背景
配套文档：glossary.md（术语字典）、environment-map.md（环境划分）。本规则适用于仓库内 /docs 下全部文档及关联代码注释。

## 3. 核心规则 & 约束（重点）
- [强制] 所有文档必须按第二节"目录强制结构"存放，禁止随意增删二级目录。
- [强制] 文件名、文件夹命名必须符合第三节规则，违反不允许合并。
- [强制] 每篇 MD 必须套用第四节"通用头部模板 + 单篇内容结构"。
- [强制] 标记符号统一：[强制] / [建议] / [废弃] / [待确认]。
- [强制] 架构、数据库、接口三类文档变更必须同步更新关联代码注释。
- [强制] 任何 MD 修改提交 Git 须经 1 人以上评审。
- [强制] 文档版本号规则：主版本。功能迭代.Bug 修复（Vx.y.z）。
- [强制] OpenAIP-OS 专有前缀统一：文档编号 WB-DOC-、表 wb_、接口 /api/v1/wb/（可选）、错误码 WBxxxx。

## 4. 详细设计/内容

### 4.1 目录强制结构
```
/docs
├── 01_项目总览
│   ├── project-overview.md       # 项目简介、目标、业务价值、干系人
│   ├── project-constraint.md     # 本开发约束总规则（本文档）
│   ├── environment-map.md        # 环境划分：开发/测试/预发/生产
│   └── glossary.md               # 统一术语字典（避免命名混乱）
├── 02_需求设计
│   ├── requirement-all.md        # 全量业务需求
│   ├── user-story.md             # 用户故事+场景
│   ├── prd-xxx.md                # 模块PRD，按功能拆分
│   └── acceptance-criteria.md    # 验收标准
├── 03_架构设计
│   ├── architecture-total.md     # 整体架构图、技术栈选型
│   ├── module-split.md           # 模块拆分、依赖关系
│   ├── security-design.md        # 权限、加密、防攻击设计
│   └── performance-design.md     # 性能、并发、缓存策略
├── 04_数据库设计
│   ├── db-total-erd.md           # ER图说明
│   ├── table-design.md           # 所有表结构、字段、索引、注释
│   ├── data-dict.md              # 数据字典、枚举值
│   └── sql-script.md             # 建表SQL、初始化脚本
├── 05_接口文档
│   ├── api-restful-spec.md       # RESTful 统一规范
│   ├── module-xxx-api.md         # 按模块拆分接口
│   └── error-code.md             # 全局错误码定义
├── 06_开发规范
│   ├── code-style-backend.md
│   ├── code-style-frontend.md
│   ├── git-commit-rule.md        # Git提交、分支管理约束
│   ├── dependency-lock.md        # 依赖版本锁定规则
│   └── third-party-sdk.md        # 三方组件使用约束
├── 07_测试文档
│   ├── test-plan.md
│   ├── test-case.md
│   ├── bug-record-template.md
│   └── regression-test.md
├── 08_部署运维
│   ├── deploy-steps.md
│   ├── docker-compose.md
│   ├── nginx-config.md
│   ├── monitor-alert.md
│   └── rollback-plan.md          # 回滚方案
├── 09_迭代与版本
│   ├── sprint-plan.md
│   ├── version-changelog.md      # 版本变更日志
│   └── todo-backlog.md
└── 10_复盘归档
    ├── problem-summary.md
    ├── optimization-record.md
    └── meeting-minutes-template.md
```

### 4.2 文件命名强制约束
- 文件名：全部小写英文，短横线 `-` 连接，禁止中文、空格、下划线、特殊符号。
- 格式：`[前缀]-[模块/主题]-[后缀].md`。
- 禁止示例：`数据库设计.md`、`接口文档V2.md`、`架构_定稿.md`。
- 正确示例：`table-user-design.md`、`api-user-module.md`、`v1.2.0-changelog.md`、`git-branch-rule.md`。
- 文件夹：二级目录 `01_xxx` 数字前缀排序、下划线分隔、中文简短描述；最深子目录只用英文小写 + 短横线。

### 4.3 单篇 Markdown 内容结构强制模板
```
# 文档标题
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-XX-001 |
| 所属模块 | OpenAIP-OS-XXX |
| 版本 | V1.0 |
| 创建人 | XXX |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
## 2. 业务/技术背景
## 3. 核心规则 & 约束（重点）   # 用 - [强制] / - [建议] 标记
## 4. 详细设计/内容
## 5. 风险点 & 注意事项
## 6. 关联文档
## 修订日志
```

### 4.4 标记符号统一约定（全局生效）
- `[强制]`：必须遵守，代码评审卡点，违反不允许合并。
- `[建议]`：最佳实践，可酌情调整，但需备注理由。
- `[废弃]`：旧方案禁止使用。
- `[待确认]`：未敲定内容，需会后更新为最终规则。

### 4.5 各类核心文档专项开发约束

#### 4.5.1 数据库文档约束 table-design.md
- 每张表必须包含：表中文名、用途、主键、索引、唯一约束、字段注释、数据类型、是否非空、默认值。
- 表名规则：`wb_模块名_表名`，下划线分隔，全小写。例：`wb_user_account`、`wb_task_record`。
- 必须统一字段：`id` 主键（系统表 `INTEGER AUTOINCREMENT`；领域对象表 `TEXT` 语义ID，如 `customer_001` / `tr_xxx`）、`create_time`、`update_time`、`is_delete` 逻辑删除。
- 枚举全部抽离到 data-dict.md，代码与文档枚举值完全一一对应。
- 所有修改必须更新修订日志，DML/DDL 脚本单独存放，禁止直接在线改库。

#### 4.5.2 接口文档约束 xxx-api.md
统一接口描述字段，缺一不可：
```
### 1.1 获取用户详情
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/user/info
- [强制] 权限要求：登录用户
- 请求参数（Query/Body）
- 响应成功示例
- 响应失败错误码
- 特殊逻辑：如缓存、幂等、分页规则
```
约束：RESTful 风格，版本放在路径 `/api/v1/`；返回体统一包装 `code`、`msg`、`data`；分页统一参数 `pageNum`、`pageSize`、`total`；所有入参校验规则写在文档中。

#### 4.5.3 Git & 提交规范 git-commit-rule.md（开发强约束）
分支模型：
- `main`：生产稳定分支，禁止直接 push，仅 merge/release
- `dev`：开发主分支
- `feature/xxx`：功能分支
- `hotfix/xxx`：线上紧急修复
- `release/vx.x.x`：版本发布分支

Commit 格式强制：
```
[type](模块): 简短描述
```
类型：`feat`/`fix`/`docs`/`style`/`refactor`/`test`/`chore`。
示例：
```
feat(user): 新增用户手机号绑定接口
fix(task): 修复任务状态流转bug
docs: 更新 OpenAIP-OS 数据库表结构文档
```

#### 4.5.4 版本日志 version-changelog.md 约束
按版本倒序，固定三级分类：新增功能 / Bug 修复 / 优化调整。
```
## V1.0.0 (2026-08-11)
### 新增
1. 用户登录模块
2. 任务创建、编辑、删除基础CRUD
### 修复
无
### 优化
统一全局异常拦截器返回格式
```

#### 4.5.5 会议纪要模板 meeting-minutes-template.md
固定结构：会议主题、时间、参会人、待决议题、决议结果、责任人、截止时间、关联迭代版本。

### 4.6 Markdown 语法书写强制规则
- 标题层级严格递进：`#` 一级 → `##` 二级 → `###` 三级，不跨级。
- 禁止：`# 标题` 直接接 `### 小标题`。
- 代码块必须标注语言：`sql` `java` `javascript` `shell`。
- 表格对齐统一，所有表格加表头，不使用简易不规则表格。
- 图片统一存放 `/docs/assets/`，相对路径引用，禁止网络外链图片。
- 内部文档跳转使用相对路径 `[标题](./xxx.md)`。
- 列表统一用 `-` 无序列表，有序列表仅用于步骤流程。
- 中文与英文、数字之间加空格，如 `V 1.0`、`id 字段`、`HTTP 200`。

### 4.7 文档版本与评审约束（落地执行规则）
- 任何 MD 修改提交到 Git 必须经过 1 人以上评审。
- 架构、数据库、接口三大类文档变更，必须同步更新关联代码注释。
- 文档版本号规则：主版本。功能迭代.Bug 修复 Vx.y.z。
- 上线前必须核对对应版本的 changelog、部署文档、回滚方案完整性。
- 过期文档标记 `[废弃]`，不删除，保留归档溯源。

### 4.8 OpenAIP-OS 专有前缀统一（全局约束）
- 文档编号：`OAIP-DOC-` 或保留 `WB-DOC-`（若已有习惯）
- 数据库表：`wb_xxx_xxx`（保持现有表名一致）
- 接口前缀：`/api/v1/wb/xxx`（可选）
- 错误码前缀：`WBxxxx`（保留）

## 5. 风险点 & 注意事项
- 目录/命名违规会在评审阶段被卡，需工具化校验（如 CI 脚本扫描文件名）。
- 枚举与代码漂移是长期隐患，需以 data-dict.md 为单一来源。
- 文档与代码不同步会迅速失效，三大类文档变更必须同步注释。

## 6. 关联文档
- [./project-overview.md](./project-overview.md)
- [./glossary.md](./glossary.md)
- [./environment-map.md](./environment-map.md)
- [../04_数据库设计/table-design.md](../04_数据库设计/table-design.md)
- [../05_接口文档/api-restful-spec.md](../05_接口文档/api-restful-spec.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-13 | 品牌统一为 OpenAIP-OS；前缀说明与 Git 示例对齐 |
