# Ontology macOS 本地版

## 目标

当前版本先提供一个不依赖 dsh、不依赖 Turso 云端同步的 macOS `.app`。运行时使用本地 libSQL 文件，数据库结构保持 SQLite 兼容。

## 构建

在 macOS 仓库根目录执行：

```bash
make ui
make mac-app
```

或者直接执行：

```bash
./scripts/build-mac-app.sh
```

产物：

```text
target/mac/Ontology.app
```

启动：

```bash
open target/mac/Ontology.app
```

## 运行时目录

程序数据不会写入应用包，而是写入用户目录：

```text
~/Library/Application Support/Ontology/
├── ontology.db
├── first-run-credentials.txt
└── logs/ontology.log
```

这样升级 `.app` 时不会覆盖用户数据，也方便使用 SQLite 工具备份 `ontology.db`。

## 首次登录

第一次启动会生成随机本地管理员密码，保存在：

```text
~/Library/Application Support/Ontology/first-run-credentials.txt
```

该文件权限为 `0600`。登录地址默认是：

```text
http://127.0.0.1:18420/
```

## 本地业务包

启动壳支持通过环境变量指定随包业务目录。相对路径以 `.app/Contents/Resources` 为基准：

```bash
ONTOLOGY_DOMAIN_PACK=domain-packs/crm-retention \
  open /Users/lushaohui/project/ontology2/target/mac/Ontology.app
```

未指定时使用内置的 `ontology.odl`。启动壳同时导出：

```text
ONTOLOGY_DOMAIN_PACKS_DIR=.app/Contents/Resources/domain-packs
```

因此网页端“业务包”页面会扫描随 `.app` 发布的全部业务包，而不是只看到启动时选中的单个业务包。后续彩票业务包加入 `domain-packs/lottery-analysis` 后，可以直接复用这条启动链路；业务包安装仍然写入本地 `ontology.db`，不涉及云端同步。

## 当前边界

- 数据库只使用本地路径 `ontology.db`。
- 启动壳会清除 `TURSO_AUTH_TOKEN`、`TURSO_DATABASE_URL` 和 `LIBSQL_URL`，避免误接云端。
- 当前 `.app` 是本地 HTTP 服务加浏览器 UI，不是原生 WebView 窗口。
- 当前不打包 dsh，也不要求系统安装 dsh。
- Pi Agent 的完整随包分发仍是后续工作；本阶段只完成 Ontology 本地运行壳。
- 正式发布前需要使用 Apple Developer ID 对 `.app` 签名并公证；脚本目前仅做 ad-hoc 签名。
