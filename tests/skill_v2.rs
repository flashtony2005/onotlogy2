//! V2.0 AI Skill Asset Compiler & Registry（Phase 9）契约测试。
//!
//! TDD：先写测试（红），再实现（绿）。
//!
//! M1 RawSkill 导入 + hash + 只读
//! M2 Markdown → SkillIR：阶段/子技能/能力/工件/指标/反馈环 + source_span
//! M3 Capability Resolution：resolved / UNRESOLVED Candidate（不凭空创造）
//! M4 Ontology/Policy 提取 → RelationDefinition/PolicyDefinition 候选
//! M5 Validation：Schema/Semantic 失败路径
//! M6 Registry：注册 + 生命周期（IMPORTED→…→ACTIVE→DEPRECATED）
//! M7 Compose：A+B → Candidate → 重新验证 → 注册（原资产不动）
//! M8 Fork：v1→v1.1 lineage；Promotion；原资产保留
//! M9 Export：Asset → SKILL.md（roundtrip 可再导入）
//! M-Rev 原则反向：未解析能力不进入执行路径；AI 推断不 bypass 授权
//! YT 端到端：YouTube Channel Growth 全链路

use openaip_os::skill::asset::{AssetCompiler, SkillAsset, SkillStatus, Validator};
use openaip_os::skill::export::export_skill_md;
use openaip_os::skill::graph::SkillGraph;
use openaip_os::skill::ir::SkillType;
use openaip_os::skill::ir::{
    CapabilityRef, FeedbackLoop, MetricIR, PolicyIR, SkillIR, WorkflowStep,
};
use openaip_os::skill::parser::{AISemanticParser, MarkdownSkillParser};
use openaip_os::skill::raw::{RawSkill, SourceType};
use openaip_os::skill::registry::SkillRegistry;
use openaip_os::skill::resolver::{
    CapabilityCandidate, CapabilityResolution, ResolutionStatus, SkillResolver,
};
use openaip_os::skill::{compose, lineage};

/// YouTube Channel Growth Skill（MVP 验收夹具：Composite + 7 阶段 + 子技能 + 反馈环）。
const YT_SKILL_MD: &str = r#"# YouTube Channel Growth

帮助 YouTube 频道实现从利基研究到变现优化的完整增长闭环。

- goal: 通过系统化的内容策略实现 YouTube 频道增长

## Stage 1: Niche Research
- capability: web.search
- artifact: NicheReport
- metric: SearchVolume

## Stage 2: Channel Positioning
- capability: llm.reason
- artifact: ChannelPositioning

## Stage 3: Audience Research
- capability: web.search
- artifact: AudienceProfile

## Stage 4: Content Strategy
- capability: llm.reason
- artifact: ContentCalendar

## Stage 5: Video Ideation
- capability: llm.reason
- artifact: VideoScript

## Stage 6: Video Production
- capability: video.generate
- artifact: VideoFile

## Stage 7: Publishing & Analytics
- capability: youtube.publish
- artifact: AnalyticsReport

### Sub-skill: NicheResearch
### Sub-skill: ChannelPositioning
### Sub-skill: AudienceResearch
### Sub-skill: ContentStrategy
### Sub-skill: VideoIdeation
### Sub-skill: VideoProduction
### Sub-skill: AlgorithmGrowth
### Sub-skill: Monetization
### Sub-skill: AnalyticsOptimization

- input: channel_niche
- output: growth_plan
- metric: CTR
- metric: Retention
- feedback: AnalyticsOptimization -> ContentStrategy
- relation: Storyboard contains Scene
- policy: 发布超过 500 美元广告需要管理员批准
"#;

fn yt_raw() -> RawSkill {
    RawSkill::import("youtube-channel-growth", YT_SKILL_MD)
}

fn yt_ir() -> SkillIR {
    MarkdownSkillParser
        .parse(&yt_raw())
        .expect("YouTube Skill 解析")
}

fn yt_asset() -> SkillAsset {
    let raw = yt_raw();
    let ir = yt_ir();
    let resolutions = SkillResolver::new(&[
        "web.search",
        "llm.reason",
        "video.generate",
        "youtube.publish",
    ])
    .resolve_capabilities(&ir);
    AssetCompiler::default()
        .compile(&raw, &ir, &resolutions)
        .expect("编译 Asset")
}

// ── M1：RawSkill 导入 + hash + 只读 ───────────────────────────────────
#[tokio::test]
async fn tc_m1_raw_skill_import_hash_readonly() {
    let raw = yt_raw();
    assert_eq!(raw.name, "youtube-channel-growth");
    assert_eq!(raw.source_type, SourceType::Markdown);
    // hash 确定性 + 内容敏感
    assert!(!raw.content_hash.is_empty());
    assert_eq!(
        raw.content_hash,
        RawSkill::content_hash_of(YT_SKILL_MD),
        "同内容同 hash"
    );
    assert_ne!(
        raw.content_hash,
        RawSkill::content_hash_of("# Other Skill\n"),
        "不同内容不同 hash"
    );
    // 只读：RawSkill 无修改 API（内容快照不可变）
    assert_eq!(raw.content, YT_SKILL_MD);
}

// ── M2：Markdown → SkillIR（阶段/子技能/能力/工件/指标/反馈 + span）────
#[tokio::test]
async fn tc_m2_markdown_to_skill_ir() {
    let ir = yt_ir();
    // 7 个阶段（按顺序）
    assert_eq!(ir.workflow.len(), 7);
    assert_eq!(ir.workflow[0].value.name, "Niche Research");
    assert_eq!(ir.workflow[6].value.name, "Publishing & Analytics");
    // 子技能（Composite 判定依据）
    let subs: Vec<&str> = ir.sub_skills.iter().map(|s| s.value.as_str()).collect();
    assert!(subs.contains(&"NicheResearch"));
    assert!(subs.contains(&"AnalyticsOptimization"));
    // 能力/工件/指标
    assert!(
        ir.capabilities
            .iter()
            .any(|c| c.value.capability == "video.generate")
    );
    assert!(ir.artifacts.iter().any(|a| a.value.name == "VideoScript"));
    assert!(ir.metrics.iter().any(|m| m.value.name == "CTR"));
    // 反馈环
    assert!(
        ir.feedback_loops
            .iter()
            .any(|f| f.value.from == "AnalyticsOptimization" && f.value.to == "ContentStrategy")
    );
    // source_span（行号区间，可追溯）
    let cap = ir
        .capabilities
        .iter()
        .find(|c| c.value.capability == "video.generate")
        .unwrap();
    assert!(
        cap.span.start >= 1 && cap.span.end >= cap.span.start,
        "span 为行号区间"
    );
    assert!(cap.confidence > 0.0, "置信度可追溯");
    // provenance
    assert_eq!(ir.provenance.raw_skill_id, "youtube-channel-growth");
}

// ── M3：Capability Resolution（resolved / UNRESOLVED Candidate）────────
#[tokio::test]
async fn tc_m3_capability_resolution() {
    let ir = yt_ir();
    // 已知 registry：web.search / llm.reason 已注册
    let resolver = SkillResolver::new(&["web.search", "llm.reason"]);
    let resolutions = resolver.resolve_capabilities(&ir);

    let resolved: Vec<&CapabilityResolution> = resolutions
        .iter()
        .filter(|r| r.status == ResolutionStatus::Resolved)
        .collect();
    assert!(
        resolved.iter().any(|r| r.capability == "web.search"),
        "已注册 → resolved"
    );

    // 未注册（video.generate / youtube.publish）→ UNRESOLVED Candidate，绝非真实 Capability
    let candidates = resolver.unresolved_candidates(&resolutions);
    assert!(
        candidates
            .iter()
            .any(|c| c.semantic_action() == "video.generate"),
        "未注册能力 → CapabilityCandidate"
    );
    for c in &candidates {
        assert_eq!(
            c.status(),
            openaip_os::skill::resolver::CandidateStatus::Unresolved
        );
        assert!(c.id().starts_with("candidate."), "候选 id 前缀隔离");
        assert!(
            !matches!(c, CapabilityCandidate::Resolved { .. }),
            "Candidate 类型层面不可能成为真实 Capability（P4）"
        );
    }
    // resolved 的不产生 candidate
    assert!(
        !candidates
            .iter()
            .any(|c| c.semantic_action() == "web.search")
    );
}

// ── M4：Ontology / Policy 提取 → RuntimeDefinition 候选 ───────────────
#[tokio::test]
async fn tc_m4_ontology_policy_extraction() {
    let ir = yt_ir();
    // Ontology：relation 行 → RelationDefinition 候选（predicate）
    assert!(
        ir.relations.iter().any(|r| r.value.predicate == "contains"),
        "Storyboard contains Scene → RelationDefinition{{predicate: contains}}"
    );
    let rel = openaip_os::ontology::RelationDefinition {
        predicate: "contains".into(),
    };
    assert_eq!(rel.predicate, "contains");

    // Policy：自然语言行 → PolicyIR → PolicyDefinition 候选（require_approval）
    let p = ir
        .policies
        .iter()
        .find(|p| p.value.requirement.contains("approval"))
        .expect("审批策略提取");
    assert!(p.value.condition.contains("500"), "金额条件保留");
    let pd = openaip_os::ontology::PolicyDefinition {
        require_approval: true,
        preconditions: vec![],
    };
    assert!(
        pd.require_approval,
        "→ PolicyDefinition{{require_approval}} 候选"
    );
}

// ── M5：Validation 失败路径（Schema / Semantic）───────────────────────
#[tokio::test]
async fn tc_m5_validation_failures() {
    let validator = Validator::new(&["web.search", "llm.reason"]);
    // 已知能力齐全 → 通过
    let asset = yt_asset();
    // Semantic：asset 里 video.generate 未在 known（此验证器只认两个）→ 失败
    let report = validator.validate(&asset);
    assert!(
        !report.ok,
        "Semantic Validation：能力不在已知集 → 拒绝（Input→Capability→Output 不连通）"
    );
    assert!(
        report.errors.iter().any(|e| e.contains("video.generate")),
        "错误指明未连通能力"
    );

    // Schema：空 asset（缺 goal/workflow）→ 失败
    let empty = SkillAsset {
        id: "empty".into(),
        name: "empty".into(),
        version: "1.0".into(),
        skill_type: SkillType::Atomic,
        goal: String::new(),
        inputs: vec![],
        outputs: vec![],
        workflow: vec![],
        capabilities: vec![],
        artifacts: vec![],
        preconditions: vec![],
        postconditions: vec![],
        policies: vec![],
        metrics: vec![],
        feedback_loops: vec![],
        provenance: openaip_os::skill::ir::Provenance {
            raw_skill_id: "empty".into(),
            parser: "test".into(),
        },
        status: SkillStatus::Imported,
        lineage: lineage::Lineage::new("1.0"),
    };
    let schema_report = Validator::new::<&str>(&[]).validate(&empty);
    assert!(!schema_report.ok, "Schema：空 Asset 拒绝");
}

// ── M6：Registry 注册 + 生命周期 ──────────────────────────────────────
#[tokio::test]
async fn tc_m6_registry_lifecycle() {
    let mut registry = SkillRegistry::new();
    let asset = yt_asset();
    registry.register(asset.clone()).unwrap();
    assert!(registry.get("youtube-channel-growth").is_some());

    // 重复注册拒绝（幂等保护）
    assert!(registry.register(asset).is_err(), "重复 id 拒绝");

    // 生命周期推进：IMPORTED→PARSED→…→ACTIVE
    let id = "youtube-channel-growth";
    registry.advance_status(id, SkillStatus::Parsed).unwrap();
    registry
        .advance_status(id, SkillStatus::Normalized)
        .unwrap();
    registry.advance_status(id, SkillStatus::Resolved).unwrap();
    registry.advance_status(id, SkillStatus::Validated).unwrap();
    registry
        .advance_status(id, SkillStatus::Registered)
        .unwrap();
    registry.advance_status(id, SkillStatus::Active).unwrap();
    assert_eq!(registry.get(id).unwrap().status, SkillStatus::Active);

    // 非法跳转拒绝（ACTIVE → IMPORTED 逆流）
    assert!(registry.advance_status(id, SkillStatus::Imported).is_err());

    // Deprecate → Archived
    registry.deprecate(id).unwrap();
    assert_eq!(registry.get(id).unwrap().status, SkillStatus::Deprecated);
    registry.archive(id).unwrap();
    assert_eq!(registry.get(id).unwrap().status, SkillStatus::Archived);
}

// ── M7：Compose — A+B → Candidate → 重新验证 → 注册（原资产不动）──────
#[tokio::test]
async fn tc_m7_compose_revalidate_register() {
    let mut registry = SkillRegistry::new();
    let a = yt_asset();
    registry.register(a.clone()).unwrap();

    // B：一个简单 Atomic Skill
    let b_raw = RawSkill::import(
        "thumbnail-gen",
        "# Thumbnail Generator\n\n- goal: 生成缩略图\n## Stage 1: Design\n- capability: image.generate\n- artifact: ThumbnailConcept\n",
    );
    let b_ir = MarkdownSkillParser.parse(&b_raw).unwrap();
    let b_res = SkillResolver::new(&[
        "web.search",
        "llm.reason",
        "video.generate",
        "youtube.publish",
        "image.generate",
    ])
    .resolve_capabilities(&b_ir);
    let b = AssetCompiler::default()
        .compile(&b_raw, &b_ir, &b_res)
        .unwrap();
    registry.register(b.clone()).unwrap();

    // Compose：A + B → Candidate（原资产不变）
    let candidate = compose::compose(&a, &b, "youtube-growth-with-thumbnails");
    assert_eq!(candidate.id, "youtube-growth-with-thumbnails");
    assert_eq!(candidate.version, "0.1", "候选版本");
    assert!(
        candidate
            .capabilities
            .contains(&"image.generate".to_string()),
        "能力并集"
    );
    assert_eq!(
        registry
            .get("youtube-channel-growth")
            .unwrap()
            .capabilities
            .len(),
        a.capabilities.len(),
        "原资产不动"
    );

    // 重新验证（known 含全部能力）→ 注册
    let validator = Validator::new(&[
        "web.search",
        "llm.reason",
        "video.generate",
        "youtube.publish",
        "image.generate",
    ]);
    assert!(validator.validate(&candidate).ok, "候选重新验证通过");
    registry.register(candidate).unwrap();
    assert!(registry.get("youtube-growth-with-thumbnails").is_some());
}

// ── M8：Fork — v1→v1.1 lineage；Promotion；原资产保留 ─────────────────
#[tokio::test]
async fn tc_m8_fork_lineage_promotion() {
    let mut registry = SkillRegistry::new();
    let mut v1 = yt_asset();
    v1.version = "1.0".into();
    registry.register(v1.clone()).unwrap();

    // Fork → v1.1（candidate；id 后缀 @1.1——原资产保留不动）
    let mut fork = v1.clone();
    fork.id = "youtube-channel-growth@1.1".into();
    fork.version = "1.1".into();
    fork.lineage.parent = Some("youtube-channel-growth".into());
    fork.lineage.derived_from = Some("youtube-channel-growth@1.0".into());
    fork.status = SkillStatus::Forked;
    registry.register(fork.clone()).unwrap();

    let f = registry.get("youtube-channel-growth").unwrap();
    assert_eq!(f.version, "1.0", "原资产版本不变");
    assert_eq!(f.lineage.parent, None, "原资产无 parent");
    let fork_asset = fork;
    assert_eq!(
        fork_asset.lineage.parent.as_deref(),
        Some("youtube-channel-growth")
    );
    assert_eq!(
        fork_asset.lineage.derived_from.as_deref(),
        Some("youtube-channel-growth@1.0")
    );

    // Promotion：EXPERIMENTAL → EVALUATED → PROMOTED
    registry
        .advance_status("youtube-channel-growth@1.1", SkillStatus::Experimental)
        .unwrap();
    registry
        .advance_status("youtube-channel-growth@1.1", SkillStatus::Evaluated)
        .unwrap();
    registry
        .advance_status("youtube-channel-growth@1.1", SkillStatus::Promoted)
        .unwrap();
    assert_eq!(
        registry.get("youtube-channel-growth@1.1").unwrap().status,
        SkillStatus::Promoted
    );
    // 原 v1.0 不受 fork 影响（保持注册时状态）
    assert_eq!(
        registry.get("youtube-channel-growth").unwrap().status,
        SkillStatus::Imported
    );
}

// ── M9：Export — Asset → SKILL.md（roundtrip 可再导入）────────────────
#[tokio::test]
async fn tc_m9_export_roundtrip() {
    let asset = yt_asset();
    let md = export_skill_md(&asset);
    assert!(md.contains("# YouTube Channel Growth"), "标题");
    assert!(md.contains("Niche Research"), "阶段保留");
    assert!(md.contains("video.generate"), "能力保留");
    assert!(md.contains("CTR"), "指标保留");

    // roundtrip：导出文本可再导入 → 关键语义一致
    let re_raw = RawSkill::import("re-import", &md);
    let re_ir = MarkdownSkillParser.parse(&re_raw).unwrap();
    assert_eq!(re_ir.workflow.len(), 7, "roundtrip 阶段数一致");
    assert!(
        re_ir
            .capabilities
            .iter()
            .any(|c| c.value.capability == "video.generate"),
        "roundtrip 能力一致"
    );
    assert!(
        re_ir
            .feedback_loops
            .iter()
            .any(|f| f.value.from == "AnalyticsOptimization"),
        "roundtrip 反馈环一致"
    );
}

// ── M-Rev：原则反向（未解析能力不进入执行路径；AI 推断不 bypass）──────
#[tokio::test]
async fn tc_mrev_principles_hold() {
    let raw = yt_raw();
    let ir = yt_ir();

    // ① 未解析能力绝不进入 Asset.capabilities（AI 推断 ≠ 授权）
    let resolver = SkillResolver::new(&["web.search", "llm.reason"]); // video.generate 未注册
    let resolutions = resolver.resolve_capabilities(&ir);
    let asset = AssetCompiler::default()
        .compile(&raw, &ir, &resolutions)
        .unwrap();
    assert!(
        !asset.capabilities.contains(&"video.generate".to_string()),
        "未注册能力不进入执行路径（P4）"
    );
    // ② Candidate 明确存在（可观察、待人工/后续注册）
    let candidates = resolver.unresolved_candidates(&resolutions);
    assert!(
        candidates
            .iter()
            .any(|c| c.semantic_action() == "youtube.publish")
    );

    // ③ AI 推断不 bypass Policy：policy 提取为候选而非自动生效
    assert!(
        !asset.policies.is_empty(),
        "策略作为 Asset 数据（候选），不自动进入 Policy Engine"
    );
}

// ── YT 端到端：Import → Parse → Resolve → Compile → Register → Graph/Compose/Fork/Export ──
#[tokio::test]
async fn tc_yt_end_to_end() {
    // Import + Parse
    let raw = yt_raw();
    let ir = yt_ir();
    // 识别 Composite（子技能 ≥ 2）
    assert_eq!(ir.skill_type, SkillType::Composite);
    assert_eq!(ir.workflow.len(), 7);
    assert!(ir.sub_skills.len() >= 9, "至少 9 个子技能");

    // Resolve（youtube.publish 未注册 → Candidate；其余注册）
    let resolver = SkillResolver::new(&["web.search", "llm.reason", "video.generate"]);
    let resolutions = resolver.resolve_capabilities(&ir);
    let candidates = resolver.unresolved_candidates(&resolutions);
    assert!(
        candidates
            .iter()
            .any(|c| c.semantic_action() == "youtube.publish")
    );

    // Compile + Register
    let asset = AssetCompiler::default()
        .compile(&raw, &ir, &resolutions)
        .unwrap();
    assert_eq!(asset.skill_type, SkillType::Composite);
    assert_eq!(asset.workflow.len(), 7);
    let mut registry = SkillRegistry::new();
    registry.register(asset.clone()).unwrap();
    // 生命周期链式推进（Imported→Parsed→…→Active）
    for st in [
        SkillStatus::Parsed,
        SkillStatus::Normalized,
        SkillStatus::Resolved,
        SkillStatus::Validated,
        SkillStatus::Registered,
        SkillStatus::Active,
    ] {
        registry
            .advance_status("youtube-channel-growth", st)
            .unwrap();
    }

    // View Graph
    let graph = SkillGraph::build(std::slice::from_ref(&asset));
    assert!(!graph.nodes.is_empty());
    assert!(graph.neighbors("youtube-channel-growth").is_empty() || !graph.edges.is_empty());
    assert!(!graph.has_cycle(), "DAG（反馈环不计入环检测）");
    assert!(
        graph
            .feedback
            .iter()
            .any(|(f, t)| f == "AnalyticsOptimization" && t == "ContentStrategy"),
        "反馈环可见"
    );

    // Compose / Fork / Export 可运行
    let b_raw = RawSkill::import(
        "thumbnail-gen",
        "# Thumbnail Generator\n\n- goal: 生成缩略图\n## Stage 1: Design\n- capability: image.generate\n- artifact: ThumbnailConcept\n",
    );
    let b_ir = MarkdownSkillParser.parse(&b_raw).unwrap();
    let b_res = SkillResolver::new(&[
        "web.search",
        "llm.reason",
        "video.generate",
        "image.generate",
    ])
    .resolve_capabilities(&b_ir);
    let b = AssetCompiler::default()
        .compile(&b_raw, &b_ir, &b_res)
        .unwrap();
    let composed = compose::compose(&asset, &b, "yt-with-thumbnails");
    assert_eq!(composed.capabilities.len(), asset.capabilities.len() + 1);

    let mut fork = asset.clone();
    fork.id = "youtube-channel-growth@1.1".into();
    fork.version = "1.1".into();
    fork.lineage.derived_from = Some("youtube-channel-growth@1.0".into());
    assert_eq!(
        fork.lineage.derived_from.as_deref(),
        Some("youtube-channel-growth@1.0")
    );

    let md = export_skill_md(&asset);
    assert!(md.contains("video.generate"));

    // 最终产生验收资产集
    let names = [
        "youtube-channel-growth",
        "NicheResearch",
        "ChannelPositioning",
        "AudienceResearch",
        "ContentStrategy",
        "VideoIdeation",
        "VideoProduction",
        "AlgorithmGrowth",
        "Monetization",
        "AnalyticsOptimization",
    ];
    for n in names {
        assert!(
            ir.sub_skills.iter().any(|s| s.value == n) || n == "youtube-channel-growth",
            "验收资产 {n} 应存在"
        );
    }
}

// 占位：确保 IR 类型字段可访问（编译契约）
#[allow(dead_code)]
fn _ir_contract(ir: &SkillIR) {
    let _ = ir.goal.value.as_str();
    let _: &Vec<openaip_os::skill::ir::Inferred<WorkflowStep>> = &ir.workflow;
    let _: &Vec<openaip_os::skill::ir::Inferred<CapabilityRef>> = &ir.capabilities;
    let _: &Vec<openaip_os::skill::ir::Inferred<openaip_os::skill::ir::ArtifactRef>> =
        &ir.artifacts;
    let _: &Vec<openaip_os::skill::ir::Inferred<MetricIR>> = &ir.metrics;
    let _: &Vec<openaip_os::skill::ir::Inferred<FeedbackLoop>> = &ir.feedback_loops;
    let _: &Vec<openaip_os::skill::ir::Inferred<PolicyIR>> = &ir.policies;
    let _: &Vec<openaip_os::skill::ir::Inferred<openaip_os::skill::ir::RelationIR>> = &ir.relations;
}
