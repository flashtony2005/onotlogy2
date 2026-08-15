# Plugin & Capability Runtime（Phase 10，设计定稿 + Implementation Spec）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V10-PLUGIN-RUNTIME |
| 状态 | **设计定稿 + Implementation Spec**（评审拍板） |
| 定位 | 解决"能力如何连接真实世界"——**Phase 9 管能力资产，Phase 10 管能力实现** |

## 0. 核心抽象边界（置顶）

> **Skill 是能力需求（requires capability），Tool/Plugin 是能力实现（provides capability）。**

```
Skill: generate_video ──requires──▶ Capability: video.generate
                                          │ implemented_by
                                          ▼
                      Provider: OpenAI / Google / Runway / Kling / LocalModel
                                          │
                                          ▼
                                  Tool: generate_video(...)
```

**Skill 不关心"到底是谁生成视频"**——它只要求 `video.generate`，Runtime 决定 Provider。
第三方 Skill 写死 `use GPT-5` 是 Vendor 锁定；正确形态是 `llm.reason → Router → GPT/Claude/Gemini/Qwen`，
Skill 保持 **Provider Agnostic**。

## 1. 后续路线（正式定稿）

| Phase | 名称 | 解决 |
|-------|------|------|
| 8 | Runtime | 怎么可靠执行 ✅ |
| 9 | Skill Asset Compiler & Registry | 能力从哪里来 ✅（设计定稿） |
| **10** | **Plugin & Capability Runtime** | **能力如何连接真实世界（本阶段）** |
| 11 | Skill Composition & Planning | 多个能力如何组合完成目标 |
| 12 | Skill Evaluation & Learning | 什么 Skill 更好 |
| 13 | Skill Evolution | Skill 如何自我改进 |
| 14 | Self-Evolving Agent | Agent 如何自主获得、组合、进化能力 |

## 2. 为什么是 Plugin System 而不是 Tool Registry

Phase 9 之后系统有大量 `Capability`（video.generate / image.generate / web.search /
document.extract / voice.generate / youtube.publish），**必须有人实现**——Plugin 解决
"能力实现从哪里进入系统"。Plugin 不是 Tool：**Plugin 是 Tool/AI/API/MCP/外部服务进入
OpenAIP-OS 的适配与扩展机制**。

```
Plugin ──provides──▶ Capability ──implemented_by──▶ Tool
Kling Plugin  →  video.generate  →  kling.video.generate
Runway Plugin →  video.generate  →  runway.video.generate
```

## 3. 三 Registry 架构（严格分层，互不混装）

```
┌────────────────────┐
│   Skill Registry   │   "如何完成目标"（Phase 9）
└─────────┬──────────┘
          │ requires
          ↓
┌────────────────────┐
│ Capability Registry│   "需要什么能力"（契约库 + 绑定）
└─────────┬──────────┘
          │ implemented by
          ↓
┌────────────────────┐
│   Plugin Registry  │   "谁来提供能力"（实现库）
└────────────────────┘
```

## 4. Plugin Manifest（插件的"身份证"）

```yaml
id: openai.plugin
name: OpenAI Provider
version: 1.0.0
runtime: http            # http | mcp | local | ai
capabilities:
  - id: llm.generate
    version: "1"
  - id: vision.analyze
    version: "1"
config:
  - name: api_key
    type: secret
permissions:
  network:
    - api.openai.com
  filesystem:
    read: ["/workspace/input"]
    write: ["/workspace/output"]
  secrets: ["API_KEY"]
security:
  sandbox: true
```

## 5. Plugin SDK（第三方开发者接入面）

```rust
pub trait Plugin {
    fn manifest(&self) -> PluginManifest;
    fn capabilities(&self) -> Vec<CapabilityId>;
    async fn execute(&self, capability: CapabilityId, input: Value) -> Result<Value, PluginError>;
}
```

第三方实现 `Plugin` → 系统 `install → discover → register → execute`。

**Plugin 类型（MVP 支持子集）**：

| 类型 | 语义 | MVP |
|------|------|-----|
| Local Function | Rust 函数直接执行 | ✅（确定性执行 + 测试主力） |
| HTTP API | 声明式 HTTP 调用 | ✅（manifest 声明；网络访问经 Permission Policy） |
| AI Provider | LLM/Image/Video/Embedding 等 | ✅（声明式；无真实 LLM 环境 → mock provider，接口就位） |
| MCP | 接入 MCP Server | ⚠️（manifest/类型就位；协议接入列 Non-goal） |
| Agent | 外部 Agent 作为 Capability Provider | ❌ Non-goal |

## 6. Plugin Lifecycle

```
DISCOVERED → INSTALLED → LOADED → REGISTERED → ACTIVE
            → DISABLED → UNINSTALLED
```

安装后自动：`Plugin → Manifest → Capabilities → Capability Registry`（自动注册绑定）。

## 7. Plugin Security（从 Phase 10 开始，接 Phase 8 Policy Runtime）

Plugin 本质是**第三方代码进入 Runtime**——不能"下载即执行"。执行前检查：

```
Plugin → Requested Permission → Policy → ALLOW / DENY
```

- **Permission**：manifest 声明 network 白名单 / filesystem 读写路径 / secrets；
  运行时请求超出声明 → **DENY**（fail-closed）
- **Secret Isolation**：config 中 `type: secret` 的项不进入审计/日志
- **Resource Limit / Sandbox**：声明字段就位（sandbox: true）；实际沙箱列 Non-goal
- **Audit**：每次插件执行产生事件（plugin.executed / plugin.denied）

## 8. Capability Binding & Router

```
CapabilityRegistry: capability_id → 契约（input/output schema + effects）
                     + bindings: [Provider { plugin_id, tool_id, cost, latency, reliability }]

Router: route(capability, criteria) → Provider
  选择依据：Capability + Input + Policy + Cost + Latency + Quality + Availability
```

MVP 选择策略（确定性）：`explicit(指定 plugin)` / `first(注册序)` / `best_effort(简单评分)`。

## 9. 与 Phase 9 的闭环（YouTube MVP 验收）

```
Skill: YouTube Video Creation（Phase 9 Asset）
  requires: research / script.generate / image.generate / video.generate
            / voice.generate / subtitle.generate / analytics
      ↓ Capability Registry（契约）
      ↓ Plugin Registry（实现）
  Web Search Plugin / LLM Plugin / Image Plugin / Video Plugin / TTS Plugin / Subtitle Plugin
      ↓ Router
      ↓ Runtime 执行
  用户目标 → Skill → Capability → Plugin → 第三方 AI/API → 视频
```

## 10. Implementation Spec（差距清单）

| # | 差距 | 内容 | 复用 |
|---|------|------|------|
| G1 | PluginManifest | id/name/version/runtime/capabilities/config/permissions/security（可序列化） | Phase 9 ABI 模式 |
| G2 | Plugin trait（SDK） | manifest/capabilities/execute + PluginError | — |
| G3 | PluginRegistry + Lifecycle | DISCOVERED→…→UNINSTALLED；install/discover/register/disable/uninstall | Phase 9 Registry + Phase 7 状态机 |
| G4 | CapabilityRegistry（契约库） | capability_id → 契约（schema/effects）+ bindings | Phase 9 G13 schema |
| G5 | CapabilityBinding | capability ↔ (plugin, tool) 绑定；自动注册于 plugin 安装 | — |
| G6 | Router | explicit/first/best_effort 选择策略 | — |
| G7 | PluginRuntime | execute 入口 + 权限检查（Permission Policy ALLOW/DENY）+ 审计事件 | Phase 8 Policy/事件 |
| G8 | Executors | LocalFunctionExecutor（确定性）；HttpPlugin/AIPlugin 声明式（mock 执行器） | — |
| G9 | Secret Isolation | config secret 不进审计/日志 | — |
| G10 | 三 Registry 闭环 | Skill.requires → CapabilityRegistry → PluginRegistry → 执行 | Phase 9 SkillAsset |

## 11. 测试规划

| 测试 | 验证 |
|------|------|
| P1 | Manifest 解析 + 序列化 roundtrip |
| P2 | Plugin 安装 + Lifecycle（INSTALLED→REGISTERED→ACTIVE→DISABLED→UNINSTALLED） |
| P3 | Capability Binding：plugin 安装自动注册提供能力 |
| P4 | Router：多 provider 选择（explicit/first/best_effort） |
| P5 | Runtime 执行：Local Function 真实执行 + 审计事件 |
| P6 | 闭环：Skill requires → Capability → Plugin → 执行（Phase 9 Asset 接入） |
| P7 | 权限：请求超出 manifest 声明 → DENY（fail-closed）；secret 不进日志 |
| P8 | 无实现能力 → 不可执行（UNRESOLVED，不进入任何执行路径） |
| P9 | YouTube MVP：research/script/image/video/voice/subtitle/analytics 全链路路由+执行 |
| P-Rev | 反向：Plugin 不能提供未声明的 Capability；Skill 不能直接绑定 Provider（必须经 Router） |

## 12. Non-goals

- ❌ Plugin Marketplace / 自动跨平台发布 / 自动部署第三方能力
- ❌ 真实 HTTP/MCP/AI 网络接入（需外部服务与密钥；manifest + 接口就位，mock 执行器先行）
- ❌ MCP 协议实现、沙箱（cgroup/容器）、资源限额实际执行（声明字段就位）
- ❌ Provider 成本/质量真实指标（字段就位；best_effort 用确定性占位评分）
- ❌ UI 页面（API + 测试验收）

## 13. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | PluginManifest + RuntimeType + Permissions + Lifecycle 状态机 | `src/plugin/{mod,manifest,lifecycle}.rs` |
| 2 | Plugin trait（SDK）+ PluginError | `src/plugin/sdk.rs` |
| 3 | PluginRegistry：install/discover/register/disable/uninstall | `src/plugin/registry.rs` |
| 4 | CapabilityRegistry + Binding + Router | `src/plugin/capability.rs` |
| 5 | PluginRuntime：execute + Permission Policy（ALLOW/DENY）+ 审计 | `src/plugin/runtime.rs` |
| 6 | Executors：LocalFunctionExecutor / HttpExecutor(mock) / AIExecutor(mock) | `src/plugin/exec.rs` |
| 7 | Secret Isolation（config secret 脱敏） | `src/plugin/runtime.rs` |
| 8 | 测试 P1-P9 + P-Rev（YouTube 全链路） | `tests/plugin_v2.rs` |
| 9 | 门禁（fmt/clippy/test 全绿）+ 提交 | — |

## 14. Phase 9 → Phase 10 边界（一句话定死）

> **Phase 9 管"能力资产"，Phase 10 管"能力实现"。**

```
              OpenAIP-OS
        ┌──────────┴──────────┐
        ↓                     ↓
   Skill Layer           Plugin Layer
   Skill Registry        Plugin Registry
        ↓                     ↓
  Capability ─────────→ Implementation
        └──────────┬──────────┘
                   ↓
                Runtime → Agent
```

三层架构第一次真正形成：**Skill → Capability → Implementation**。
之后 Phase 11（组合）→ 12（评价）→ 13（进化）→ 14（自主获得与进化）路线自然衔接。
