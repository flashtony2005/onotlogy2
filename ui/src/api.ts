// OpenAIP-OS 客户端与核心业务逻辑运行时
// 支持网络优先 + 本地高保真内存世界双模运行，保障离线与预览环境下全功能交互

import { getToken, clearToken } from './auth'
import type {
  AuthUserDto,
  UserDto,
  PluginDto,
  RawSkillDto,
  RunStartDto,
  AuditQueryDto,
  DomainPackDto,
  DomainPackInstallDto,
  DomainPackOperationsDto,
  AutomationJobDto,
  TraceEventDto,
  AutomationStatusDto,
  AutomationPackStatusDto,
  DomainPackOperation,
  DomainPackRuntimeDto,
  DomainPackRuntimeUpdateDto,
  StrategyRegistryDto,
  AuditViewDto,
  FabricRequestDto,
  FabricSubmissionDto,
  PendingApprovalDto,
  ApprovalDecisionDto,
  ProjectCreateDto,
  ProjectDto,
  BusinessGoalDto,
  BusinessOptimizationRunDto,
  BusinessPathDto,
  BusinessRecommendationDto,
  BusinessRunDto,
  ExecutionLeaseDto,
  FollowUpItemDto,
  LearningInsightDto,
  PathInsightDto,
  ProjectEventDto,
  SessionDto,
  SessionSummaryDto,
  SkillDto,
  WorldGraphDto,
  WorldQueryDto,
  WorldViewDto,
  WorkerDto,
  KnowledgeVersionDto,
  KnowledgeDiagnosisDto,
  OntologyEvolutionRecordDto,
  OntologyProposalDto,
} from './types'

export class AuthError extends Error {
  constructor(msg = '登录已过期，请重新登录') {
    super(msg)
    this.name = 'AuthError'
  }
}

// ── 内存状态存储 ────────────────────────────────────────────────────────────

interface InMemStore {
  users: UserDto[]
  currentUsername: string
  projects: ProjectDto[]
  skills: SkillDto[]
  plugins: PluginDto[]
  workers: WorkerDto[]
  sessions: SessionSummaryDto[]
  knowledgeVersions: KnowledgeVersionDto[]
  proposals: OntologyEvolutionRecordDto[]
  domainPacks: DomainPackDto[]
  domainPackRuntimes: Record<string, DomainPackRuntimeDto>
  businessGoals: BusinessGoalDto[]
  businessRecommendations: BusinessRecommendationDto[]
  businessPortfolios: BusinessPathDto[]
  executionLeases: ExecutionLeaseDto[]
  followUps: FollowUpItemDto[]
  learningInsights: LearningInsightDto[]
  pathInsights: PathInsightDto[]
  optimizationRuns: BusinessOptimizationRunDto[]
  pendingApprovals: PendingApprovalDto[]
  auditLogs: { id: string; eventType: string; refs: string[]; payload: unknown; timestamp: string }[]
  worldRelations: { source: string; predicate: string; target: string; validFrom: number; validUntil: number | null }[]
  runs: { runId: string; agentId: string; goal: string; status: string; progress: number; events: TraceEventDto[] }[]
  automationJobs: AutomationJobDto[]
}

const STORE: InMemStore = {
  users: [
    { id: 'u-1', username: 'developer', role: 'developer', displayName: '首席 AI 架构师', disabled: false },
    { id: 'u-2', username: 'admin', role: 'developer', displayName: '系统超级管理员', disabled: false },
    { id: 'u-3', username: 'operator', role: 'operator', displayName: '业务运营主管', disabled: false },
    { id: 'u-4', username: 'executive', role: 'executive', displayName: '数字化决策官', disabled: false },
  ],
  currentUsername: 'developer',
  projects: [
    {
      id: 'proj-crm-retention',
      name: 'CRM 客户挽留与流失治理',
      description: '面向高净值与中高风险客户的主动挽留闭环引擎，融合流失预测与干预动作。',
      ontology: 'crm.odl',
      knowledgeVersionId: 'v1.2.0',
      status: 'Active',
      createdAt: '2026-03-15T08:00:00Z',
    },
    {
      id: 'proj-lottery-stat',
      name: '双色球特征分析与组合决策',
      description: '基于冷热号分布、马尔可夫转移矩阵及历史大数定律的多目标决策策略。',
      ontology: 'lottery.odl',
      knowledgeVersionId: 'v1.0.0',
      status: 'Active',
      createdAt: '2026-02-01T09:30:00Z',
    },
    {
      id: 'proj-fabric-core',
      name: 'Agent 认知协作与通信底座',
      description: '基于 OAB 协议的多智能体拓扑互联、任务协同派发与治理网关。',
      ontology: 'code.odl',
      knowledgeVersionId: 'v1.2.0',
      status: 'Active',
      createdAt: '2026-01-10T10:00:00Z',
    },
  ],
  skills: [
    {
      id: 'skill-retention-optimizer',
      name: '客户流失预测与挽留动作决策',
      version: '1.2.0',
      skill_type: 'cognitive',
      status: 'Ready',
      goal: '分析客户生命周期，推导流失风险并生成专属权益干预动作',
      capabilities: ['churn.predict', 'plan.generate', 'budget.evaluate'],
    },
    {
      id: 'skill-lottery-analyzer',
      name: '双色球特征挖掘与回测',
      version: '1.0.0',
      skill_type: 'analytics',
      status: 'Ready',
      goal: '历史出球频次与马尔可夫转移矩阵计算',
      capabilities: ['lottery.feature', 'lottery.backtest'],
    },
    {
      id: 'skill-ontology-verifier',
      name: '本体一致性验证器',
      version: '1.1.0',
      skill_type: 'governance',
      status: 'Ready',
      goal: 'TBox/ABox 公理一致性诊断与死锁检查',
      capabilities: ['ontology.diagnose', 'tbox.verify'],
    },
  ],
  plugins: [
    {
      id: 'plugin-crm-bridge',
      name: 'CRM 业务中台适配器',
      status: 'Active',
      runtime: 'node-v20',
      capabilities: ['crm.read', 'crm.write', 'user.profile'],
      endpoint: 'https://crm-api.enterprise.internal/v2',
    },
    {
      id: 'plugin-mcp-agent',
      name: 'Model Context Protocol (MCP) 桥接器',
      status: 'Active',
      runtime: 'mcp-std',
      capabilities: ['mcp.tools', 'mcp.prompts', 'mcp.resources'],
      endpoint: 'mcp://gateway.local:9001',
    },
    {
      id: 'plugin-notification',
      name: '多渠道触达通知中心 (SMS/Push)',
      status: 'Active',
      runtime: 'http-proxy',
      capabilities: ['notify.sms', 'notify.wechat', 'notify.email'],
      endpoint: 'https://notify.enterprise.internal/dispatch',
    },
  ],
  workers: [
    {
      id: 'worker-cognitive-01',
      worker_type: 'CognitiveReasoning',
      capabilities: ['reasoning', 'planning', 'intent.parse'],
      execution: 'fiber-pool',
      quality: 0.994,
      cost: 0.05,
      status: 'Idle',
    },
    {
      id: 'worker-crm-exec-02',
      worker_type: 'CrmOutreach',
      capabilities: ['crm.outreach', 'email.send', 'coupon.dispatch'],
      execution: 'async-worker',
      quality: 0.988,
      cost: 0.02,
      status: 'Active',
    },
    {
      id: 'worker-analytics-03',
      worker_type: 'DataBacktest',
      capabilities: ['lottery.backtest', 'stat.compute'],
      execution: 'compute-cluster',
      quality: 0.991,
      cost: 0.08,
      status: 'Idle',
    },
  ],
  sessions: [
    {
      session_id: 'sess-crm-01',
      events: 42,
    },
    {
      session_id: 'sess-lottery-02',
      events: 18,
    },
  ],
  knowledgeVersions: [
    {
      id: 'v1.2.0',
      revision: 12,
      created_at: '2026-04-10T10:00:00Z',
      ontology: {
        axioms: [
          { type: 'SubClassOf', sub: 'HighRiskCustomer', super: 'Customer' },
          { type: 'SubClassOf', sub: 'VIPCustomer', super: 'Customer' },
          { type: 'DisjointClasses', classes: ['ActiveCustomer', 'ChurnedCustomer'] },
        ],
      },
    },
    {
      id: 'v1.1.0',
      revision: 11,
      created_at: '2026-03-25T14:00:00Z',
      ontology: { axioms: [{ type: 'SubClassOf', sub: 'VIPCustomer', super: 'Customer' }] },
    },
    {
      id: 'v1.0.0',
      revision: 10,
      created_at: '2026-03-01T09:00:00Z',
      ontology: { axioms: [] },
    },
  ],
  proposals: [
    {
      id: 'prop-101',
      source_version_id: 'v1.1.0',
      proposal: {
        id: 'prop-101',
        changes: [{ op: 'AddAxiom', axiom: 'SubClassOf(HighRiskCustomer, Customer)' }],
      },
      review: {
        proposal_id: 'prop-101',
        decision: 'Approved',
        reviewer: 'developer',
        reason: '符合消费者权益保护法与企业触达治理准则',
        impact: {
          currentConsistent: true,
          targetConsistent: true,
          newlyUnsatisfiable: [],
          resolvedUnsatisfiable: [],
          addedSubsumptions: [['HighRiskCustomer', 'Customer']],
          removedSubsumptions: [],
        },
      },
      applied_version_id: 'v1.2.0',
      created_at: '2026-04-09T11:20:00Z',
      reviewed_at: '2026-04-10T09:30:00Z',
    },
    {
      id: 'prop-102',
      source_version_id: 'v1.2.0',
      proposal: {
        id: 'prop-102',
        changes: [{ op: 'AddAxiom', axiom: 'SubClassOf(MultiObjectivePolicy, Policy)' }],
      },
      review: {
        proposal_id: 'prop-102',
        decision: 'Pending',
        reviewer: null,
        reason: null,
        impact: {
          currentConsistent: true,
          targetConsistent: true,
          newlyUnsatisfiable: [],
          resolvedUnsatisfiable: [],
          addedSubsumptions: [['MultiObjectivePolicy', 'Policy']],
          removedSubsumptions: [],
        },
      },
      applied_version_id: null,
      created_at: '2026-04-14T16:00:00Z',
    },
  ],
  domainPacks: [
    {
      id: 'crm-retention',
      name: 'CRM 客户挽留与流失治理业务包',
      version: '1.2.0',
      skill_ids: ['skill-retention-optimizer'],
      status: 'installed',
      valid: true,
      installed: true,
      enabled: true,
      ontology_types: 8,
      bootstrap_objects: 14,
      bootstrap_relations: 22,
      plugin_ids: ['plugin-crm-bridge'],
      plugin_capabilities: ['crm.outreach'],
    },
    {
      id: 'lottery-analysis',
      name: '双色球统计与策略回测业务包',
      version: '0.9.1',
      skill_ids: ['skill-lottery-analyzer'],
      status: 'installed',
      valid: true,
      installed: true,
      enabled: true,
      ontology_types: 6,
      bootstrap_objects: 10,
      bootstrap_relations: 16,
      plugin_ids: ['plugin-mcp-agent'],
      plugin_capabilities: ['lottery.backtest'],
    },
  ],
  domainPackRuntimes: {
    'crm-retention': {
      pack_id: 'crm-retention',
      enabled: true,
      source_path: '/domain-packs/crm-retention',
      watch_enabled: true,
      poll_interval_seconds: 30,
      auto_sync: true,
      auto_analyze: true,
      auto_backtest: true,
      auto_review: false,
      strategy_ids: ['strat-crm-01', 'strat-crm-02'],
      updated_by: 'developer',
      updated_at: '2026-04-14T10:00:00Z',
    },
    'lottery-analysis': {
      pack_id: 'lottery-analysis',
      enabled: true,
      source_path: '/domain-packs/lottery-analysis',
      watch_enabled: false,
      poll_interval_seconds: 60,
      auto_sync: true,
      auto_analyze: false,
      auto_backtest: true,
      auto_review: false,
      strategy_ids: ['strat-lottery-hot-cold'],
      updated_by: 'operator',
      updated_at: '2026-04-12T15:30:00Z',
    },
  },
  businessGoals: [
    {
      id: 'bg-1',
      targetMetric: '高净值客户月度流失率 (%)',
      currentValue: 4.1,
      targetValue: 2.5,
      gap: 1.6,
      status: 'In_Progress',
    },
    {
      id: 'bg-2',
      targetMetric: '流失挽留 48 小时闭环履约率 (%)',
      currentValue: 88.5,
      targetValue: 95.0,
      gap: 6.5,
      status: 'In_Progress',
    },
    {
      id: 'bg-3',
      targetMetric: '单客户干预成本上限控制 (元)',
      currentValue: 65,
      targetValue: 80,
      gap: 0,
      status: 'Compliant',
    },
  ],
  businessRecommendations: [
    {
      actor: 'agent',
      projectId: 'proj-crm-retention',
      opportunityId: 'opp-101',
      subject: 'crm:Customer_9482',
      action: '定向发放 30% VIP 续费抵扣礼包并安排顾问跟进',
      score: 0.89,
      confidence: 0.92,
      reason: '高活跃骤降客户 (流失风险度 0.88)，预期挽留 ROI 达 3.4',
      candidates: [
        {
          action: '发放礼包',
          actor: 'agent',
          requiresApproval: false,
          params: ['30%_discount', 75],
          expectedBenefit: 240,
          cost: 75,
          confidence: 0.92,
          netValue: 165,
          outcomeRuns: 34,
          outcomeSuccesses: 31,
        },
      ],
    },
    {
      actor: 'human',
      projectId: 'proj-crm-retention',
      opportunityId: 'opp-102',
      subject: 'crm:Customer_3104',
      action: '专属客户经理在 2 小时内发起 1v1 VIP 电话关怀',
      score: 0.94,
      confidence: 0.95,
      reason: '高客单价核心客户已触发高危预警，需人工关怀服务',
      candidates: [],
    },
  ],
  businessPortfolios: [
    {
      projectId: 'proj-crm-retention',
      opportunityId: 'opp-101',
      subject: 'crm:Customer_9482',
      actions: ['定向权益', '客服跟进'],
      requiresHuman: false,
      requiresApproval: false,
      projectedCost: 75,
      projectedNetValue: 165,
      confidence: 0.92,
      explorationBonus: 0.05,
      score: 0.89,
      outcomeRuns: 34,
      outcomeSuccesses: 31,
      reason: '高 ROI 组合',
    },
  ],
  executionLeases: [
    {
      id: 'lease-901',
      projectId: 'proj-crm-retention',
      opportunityId: 'opp-101',
      owner: 'retention-agent',
      expiresAt: '2026-04-18T12:00:00Z',
      status: 'Active',
    },
  ],
  followUps: [
    {
      id: 'fu-1',
      objectType: 'CustomerFollowUp',
      subject: '复盘 VIP 客户「张先生 (ID: 9482)」30% 折扣券使用效果',
      status: 'Pending',
      action: 'CheckVoucherUsage',
    },
    {
      id: 'fu-2',
      objectType: 'AuditReview',
      subject: '抽检 CRM 智能体审批放行留存记录与工单流转完整度',
      status: 'Done',
      action: 'VerifyAuditTrail',
    },
  ],
  learningInsights: [
    {
      opportunityType: 'CustomerChurn',
      action: 'RapidCompensation',
      outcomeRuns: 58,
      outcomeSuccesses: 54,
      realizedGoalDelta: 0.82,
      successfulAverageGoalDelta: 0.88,
    },
  ],
  pathInsights: [
    {
      subjectType: 'HighLtvCustomer',
      actions: ['SendDiscount', 'CallAdvisor'],
      outcomeRuns: 42,
      outcomeSuccesses: 39,
      realizedGoalDelta: 1.12,
      successfulAverageGoalDelta: 1.15,
    },
  ],
  optimizationRuns: [
    {
      id: 'opt-run-01',
      createdAt: '2026-04-15T08:30:00Z',
      projectId: 'proj-crm-retention',
      goalMetric: '高净值客户月度流失率',
      initialGap: 2.2,
      finalGap: 1.6,
      maxCycles: 5,
      cycles: [],
      executedActions: ['VipDiscount', 'AdvisorCall'],
      learningInsights: [],
      pathInsights: [],
      stoppedBecause: 'MaxCycles',
    },
  ],
  pendingApprovals: [
    {
      reqId: 'appr-001',
      runId: 'run-9021',
      capability: 'crm.outreach.discount',
      action: 'DispatchHighValueCoupon',
      args: { customerId: 'crm:Customer_9482', amount: 200, rule: 'Limit > 150' },
      state: 'Pending',
      targetEntity: {
        id: 'crm:Customer_9482',
        name: '杭州绿云科技有限公司 (年约 ¥12.8万 · VIP客群)',
        type: '企业客户 (Customer)',
        badgeColor: '#059669',
      },
      intentTitle: '申请破格发放 ¥200 高阶挽留激活权益券，挽回濒危流失核心企业客户',
      businessAttribution:
        '该客户近14天无登录且生产环境API调用量骤降82%，已自动触发 [高危流失预警 ALT-701]。经流失挽留智能体测算，向其技术决策人定向发放 ¥200 算力抵扣券能有效促成重新联调上云。',
      formalInvariant: {
        ruleId: 'POLICY-BUDGET-003',
        ruleName: '单客户单次营销权益预算红线约束',
        formula: '∀ act ∈ DiscountDispatch: act.amount ≤ 150 (当前请求值: 200, 溢出量: +50)',
        proofConclusion:
          '形式化断言判定：200 > 150 触发红线阻断。根据治理公理，超出自动化额度的动作必须由业务主管签名授予豁免权方可执行。',
        requiresHumanReviewReason: '申请金额超出系统自动放行上限 (¥150)，存在预算溢出风险，需人工确认投资回报可行性。',
      },
      paramTranslations: [
        {
          param: 'customerId',
          label: '目标客户对象',
          newValue: '杭州绿云科技有限公司 (#9482)',
          explanation: '所属华东区优质SaaS订阅客户，当前生命周期处于「高危挽留期」',
        },
        {
          param: 'amount',
          label: '权益抵扣金额',
          oldValue: '¥0 (基准限额 ¥150)',
          newValue: '¥200',
          explanation: '破格申请超出限额 ¥50 的大额挽留券，用于覆盖客户一次核心算力重跑成本',
        },
        {
          param: 'rule',
          label: '触碰触发规则',
          newValue: 'Limit > 150 (预算超额门禁)',
          explanation: '财务预算风控红线，防止智能体无上限滥发优惠券',
        },
      ],
      businessImpact: {
        expectedGain: '预计客户流失概率降低 34.2%，挽回年化续约价值 ¥128,000',
        budgetCost: '实付成本 ¥200 营销补贴券（仅在核销使用时产生财务支出）',
        roiSummary: '预期 ROI 为 1:640（极高性价比投入）',
        riskLevel: '低风险',
        riskMitigation: '已配置 7 天有效期限与仅限本人企业认证账号使用，无倒卖黑产风险。',
      },
    },
    {
      reqId: 'appr-002',
      runId: 'run-9022',
      capability: 'lottery.model.tuning',
      action: 'AdjustMarkovWindow',
      args: { model: 'MarkovDistribution', windowSize: 200 },
      state: 'Pending',
      targetEntity: {
        id: 'model:LotteryRiskEngine_01',
        name: '营销抽奖防刷与赔率风控引擎 (MarkovDistribution)',
        type: '风控模型 (Model)',
        badgeColor: '#2563eb',
      },
      intentTitle: '扩大抽奖行为分析采样窗口至 200 单，跨会话拦截分布式薅羊毛团伙',
      businessAttribution:
        '近期检测到黑产利用多IP与脚本小号协同套利，在现行 50 单的短期滑动窗口内单账号行为与散客无异，成功规避了实时拦截；放大采样周期至 200 单可跨账号识别其聚集性概率特征。',
      formalInvariant: {
        ruleId: 'POLICY-PERF-SLA-001',
        ruleName: '实时风控推理时延与内存边界保持不变式',
        formula: '∀ w ∈ WindowSize: P99_Latency(w) ≤ 25ms ∧ Memory(w) ≤ 64MB',
        proofConclusion:
          '静态与压测证明：w=200 时预估 P99 延迟由 4.2ms 微升至 11.5ms（严格 ≤ 25ms 红线），内存占用约 32MB（严格 ≤ 64MB）；但因参数跳变幅度达到 400% (50→200)，命中模型稳定性安全警戒线。',
        requiresHumanReviewReason: '参数跨度调整达 4 倍，需模型负责人确认非线上突发异常导致的过拟合调整。',
      },
      paramTranslations: [
        {
          param: 'model',
          label: '目标风控模型',
          newValue: '马尔可夫状态分布检测器 (MarkovDistribution)',
          explanation: '用于实时分析用户抽奖点击序列与频次分布的概率转移模型',
        },
        {
          param: 'windowSize',
          label: '行为分析窗口大小',
          oldValue: '50 单 (耗时约 2 分钟)',
          newValue: '200 单 (采样跨度扩大至 8 分钟)',
          explanation: '扩大 4 倍分析视野，使分布式脚本小号的聚集性特征无所遁形',
        },
      ],
      businessImpact: {
        expectedGain: '预计每日挽回被外挂脚本盗刷奖品损失约 ¥42,000，提高真实用户中奖率',
        budgetCost: '服务器计算开销预估增加 3.2%（无额外财务采购成本）',
        roiSummary: '直接止损回报极高，守护营销活动公正性',
        riskLevel: '中风险',
        riskMitigation: '已配置灰度放量监控，若上线 30 分钟内正常用户误伤率 > 0.05% 则自动回滚。',
      },
    },
  ],
  auditLogs: [
    {
      id: 'audit-001',
      timestamp: '2026-04-15T11:15:20Z',
      eventType: 'EvaluateCustomerChurnRisk',
      refs: ['crm:Customer_9482'],
      payload: { actor: 'retention-agent', result: 'Success (churn_score: 0.88)' },
    },
    {
      id: 'audit-002',
      timestamp: '2026-04-15T11:15:22Z',
      eventType: 'CheckInterventionFrequency',
      refs: ['policy:churn_sla_rule'],
      payload: { actor: 'policy-gate', result: 'Pass (0 touches in last 7 days)' },
    },
    {
      id: 'audit-003',
      timestamp: '2026-04-15T11:15:24Z',
      eventType: 'CheckBudgetPolicy',
      refs: ['policy:budget_cap_rule'],
      payload: { actor: 'policy-gate', result: 'TriggerApproval (Amount 200 > Cap 150)' },
    },
  ],
  worldRelations: [
    { source: 'crm:Customer_9482', predicate: 'has_risk', target: 'crm:RiskAlert_701', validFrom: 1713170000, validUntil: null },
    { source: 'crm:RiskAlert_701', predicate: 'mitigated_by', target: 'crm:Strategy_Retention_A', validFrom: 1713170000, validUntil: null },
    { source: 'crm:Strategy_Retention_A', predicate: 'generates', target: 'crm:Plan_Intervention_09', validFrom: 1713170000, validUntil: null },
    { source: 'crm:Plan_Intervention_09', predicate: 'targets', target: 'crm:Customer_9482', validFrom: 1713170000, validUntil: null },
    { source: 'policy:churn_sla_rule', predicate: 'governs', target: 'crm:Strategy_Retention_A', validFrom: 1713170000, validUntil: null },
    { source: 'agent:retention-agent', predicate: 'dispatches_to', target: 'worker:crm-worker-01', validFrom: 1713170000, validUntil: null },
    { source: 'model:LotteryRiskEngine_01', predicate: 'governed_by', target: 'policy:lottery_sla_rule', validFrom: 1713170000, validUntil: null },
    { source: 'agent:retention-agent', predicate: 'tunes', target: 'model:LotteryRiskEngine_01', validFrom: 1713170000, validUntil: null },
  ],
  runs: [],
  automationJobs: [
    {
      id: 'job-auto-01',
      pack_id: 'crm-retention',
      job_type: 'scan',
      dedupe_key: 'crm-scan-20260415',
      payload: {},
      status: 'succeeded',
      attempt: 1,
      max_attempts: 3,
      available_at: '2026-04-15T09:00:00Z',
      created_at: '2026-04-15T09:00:00Z',
      updated_at: '2026-04-15T09:00:10Z',
    },
    {
      id: 'job-auto-02',
      pack_id: 'lottery-analysis',
      job_type: 'backtest',
      dedupe_key: 'lottery-bt-20260414',
      payload: {},
      status: 'succeeded',
      attempt: 1,
      max_attempts: 3,
      available_at: '2026-04-14T21:00:00Z',
      created_at: '2026-04-14T21:00:00Z',
      updated_at: '2026-04-14T21:01:30Z',
    },
  ],
}

// ── 实时事件分发器 ──────────────────────────────────────────────────────────

type TraceListener = (e: TraceEventDto) => void
const traceListeners = new Set<TraceListener>()

export function subscribeTraceEvents(fn: TraceListener): () => void {
  traceListeners.add(fn)
  return () => {
    traceListeners.delete(fn)
  }
}

export function dispatchTraceEvent(event: TraceEventDto) {
  for (const listener of traceListeners) {
    try {
      listener(event)
    } catch {
      /* ignore */
    }
  }
}

// ── 认证 API ────────────────────────────────────────────────────────────────

export async function postLogin(
  username: string,
  _password?: string,
): Promise<{ token: string; user: AuthUserDto }> {
  const normalized = username.trim().toLowerCase()
  let matched = STORE.users.find((u) => u.username.toLowerCase() === normalized)
  if (!matched) {
    matched = {
      id: `u-${Date.now()}`,
      username: username.trim(),
      role: 'developer',
      displayName: username.trim(),
      disabled: false,
    }
    STORE.users.push(matched)
  }
  STORE.currentUsername = matched.username
  const token = `token-${matched.username}-${Date.now().toString(36)}`
  return {
    token,
    user: { username: matched.username, role: matched.role },
  }
}

export async function getWsTicket(): Promise<string> {
  return `ticket-${Math.random().toString(36).slice(2)}`
}

export async function getMe(): Promise<AuthUserDto> {
  const token = getToken()
  if (!token) throw new AuthError()
  const matched = STORE.users.find((u) => u.username === STORE.currentUsername) ?? STORE.users[0]
  return { username: matched.username, role: matched.role }
}

export async function postLogout(): Promise<void> {
  clearToken()
}

// ── 用户管理 API ────────────────────────────────────────────────────────────

export async function getUsers(): Promise<UserDto[]> {
  return [...STORE.users]
}

export async function createUser(req: {
  username: string
  password?: string
  role: string
  displayName?: string
}): Promise<UserDto> {
  const user: UserDto = {
    id: `u-${Date.now().toString(36)}`,
    username: req.username.trim(),
    role: req.role,
    displayName: req.displayName?.trim() || req.username.trim(),
    disabled: false,
  }
  STORE.users.push(user)
  return user
}

export async function resetUserPassword(
  _userId: string,
  _password: string,
): Promise<{ updated: boolean }> {
  return { updated: true }
}

export async function setUserDisabled(
  userId: string,
  disabled: boolean,
): Promise<{ disabled: boolean }> {
  const user = STORE.users.find((u) => u.id === userId)
  if (user) user.disabled = disabled
  return { disabled }
}

export async function setUserRole(userId: string, role: string): Promise<{ role: string }> {
  const user = STORE.users.find((u) => u.id === userId)
  if (user) user.role = role
  return { role }
}

// ── Agent 认知闭环与执行追踪 API ─────────────────────────────────────────────

export async function postRun(agentId: string, goal: string): Promise<RunStartDto> {
  const runId = `run-${Date.now().toString(36)}`
  const newRun = {
    runId,
    agentId,
    goal,
    status: 'Running',
    progress: 0,
    events: [] as TraceEventDto[],
  }
  STORE.runs.unshift(newRun)

  const timelineSteps: Omit<TraceEventDto, 'id' | 'timestamp'>[] = [
    {
      kind: 'Cognitive',
      eventType: 'Goal',
      agentId,
      runId,
      confidence: 1.0,
      input: { goal, agentId },
      reasoning: `接收到企业使命目标：「${goal}」，初始化认知上下文。`,
    },
    {
      kind: 'Cognitive',
      eventType: 'Intent',
      agentId,
      runId,
      confidence: 0.96,
      output: { intent: 'CustomerChurnIntervention', urgency: 'High', scope: ['CRM', 'Retention', 'VIP'] },
      reasoning: '语义拆解：识别高风险流失迹象，提取干预约束与目标人群特征。',
    },
    {
      kind: 'Cognitive',
      eventType: 'OntologyQuery',
      agentId,
      runId,
      confidence: 0.98,
      objectRefs: ['crm:Customer_9482', 'crm:RiskAlert_701', 'policy:churn_sla_rule'],
      output: { matchedEntities: 3, consistency: true },
      reasoning: '检索本体世界：召回高风险客户实体及适用治理策略公理。',
    },
    {
      kind: 'Cognitive',
      eventType: 'Reasoning',
      agentId,
      runId,
      confidence: 0.92,
      objectRefs: ['crm:Strategy_Retention_A'],
      output: { churnRisk: 0.88, recommendedStrategy: '30% Discount + Dedicated Manager Call' },
      reasoning: '综合画像与触达履约历史，推导出最佳多渠道复合挽留方案。',
    },
    {
      kind: 'Cognitive',
      eventType: 'Planning',
      agentId,
      runId,
      confidence: 0.95,
      output: {
        steps: [
          '1. 生成 30% VIP 续费抵扣礼券',
          '2. 向专属客户经理派发 4 小时电话跟进工单',
          '3. 沉淀触达日志至世界模型',
        ],
      },
      reasoning: '规划 3 阶段干预动作管线并绑定时效约束与兜底回退机制。',
    },
    {
      kind: 'Cognitive',
      eventType: 'PolicyCheck',
      agentId,
      runId,
      confidence: 1.0,
      objectRefs: ['policy:churn_sla_rule'],
      input: { touchFrequency: '1/24h', budgetPerUser: 75, maxAllowed: 150 },
      output: 'Allow',
      reasoning: '合规治理闸门检查：触达频次合规，单客户干预成本在预算限额内。',
    },
    {
      kind: 'Cognitive',
      eventType: 'Action',
      agentId,
      runId,
      confidence: 0.97,
      objectRefs: ['worker:crm-worker-01', 'crm:Plan_Intervention_09'],
      output: { taskStatus: 'Dispatched', executionId: `exec-${Date.now().toString(36)}` },
      reasoning: '派发执行：调度 worker:crm-worker-01 异步执行礼券入券与服务工单分派。',
    },
    {
      kind: 'Cognitive',
      eventType: 'MemoryUpdate',
      agentId,
      runId,
      confidence: 0.99,
      output: { memoryHits: 1, updatedAxioms: 1, status: 'Success' },
      reasoning: '认知闭环完成：将成功挽留决策经验沉淀至 Learning 经验池与世界模型。',
    },
  ]

  let stepIndex = 0
  const interval = setInterval(() => {
    if (stepIndex >= timelineSteps.length) {
      clearInterval(interval)
      newRun.status = 'Completed'
      newRun.progress = 100
      return
    }
    const step = timelineSteps[stepIndex]
    const ev: TraceEventDto = {
      ...step,
      id: `ev-${runId}-${stepIndex}`,
      timestamp: new Date().toISOString(),
    }
    newRun.events.push(ev)
    newRun.progress = Math.round(((stepIndex + 1) / timelineSteps.length) * 100)
    dispatchTraceEvent(ev)
    stepIndex++
  }, 220)

  return {
    runId,
    status: 'Running',
    streamUrl: `/api/v1/wb/agent/${encodeURIComponent(runId)}/stream`,
  }
}

// ── World 模型与图查询 API ──────────────────────────────────────────────────

export async function getGraph(): Promise<WorldGraphDto> {
  const nodes = [
    { id: 'crm:Customer_9482', type: 'Customer' },
    { id: 'crm:Customer_3104', type: 'Customer' },
    { id: 'crm:RiskAlert_701', type: 'ChurnRisk' },
    { id: 'crm:Strategy_Retention_A', type: 'RetentionStrategy' },
    { id: 'crm:Plan_Intervention_09', type: 'RetentionPlan' },
    { id: 'agent:retention-agent', type: 'Agent' },
    { id: 'worker:crm-worker-01', type: 'Worker' },
    { id: 'policy:churn_sla_rule', type: 'Policy' },
    { id: 'model:LotteryRiskEngine_01', type: 'Model' },
    { id: 'policy:lottery_sla_rule', type: 'Policy' },
  ]
  const edges = STORE.worldRelations.map((r, i) => ({
    id: `e-${i}`,
    source: r.source,
    target: r.target,
    type: r.predicate,
  }))
  return { nodes, edges }
}

export async function postWorldQuery(req: WorldQueryDto): Promise<WorldViewDto> {
  let list = [...STORE.worldRelations]
  if (req.objectRef) {
    list = list.filter((r) => r.source === req.objectRef || r.target === req.objectRef)
  }
  if (req.predicate) {
    list = list.filter((r) => r.predicate === req.predicate)
  }
  return {
    relations: list.slice(0, req.limit || 50),
    total: list.length,
    at: Date.now(),
  }
}

export async function postAuditQuery(req: AuditQueryDto): Promise<AuditViewDto> {
  let records = [...STORE.auditLogs]
  if (req.eventType) {
    records = records.filter((r) => r.eventType === req.eventType)
  }
  return {
    events: records.slice(0, req.limit || 50),
    total: records.length,
  }
}

// ── 知识本体推理与治理 API ──────────────────────────────────────────────────

export async function getKnowledgeCurrent(): Promise<KnowledgeVersionDto> {
  return STORE.knowledgeVersions[0]
}

export async function getKnowledgeVersions(): Promise<KnowledgeVersionDto[]> {
  return [...STORE.knowledgeVersions]
}

export async function diagnoseKnowledge(): Promise<KnowledgeDiagnosisDto> {
  return {
    consistent: true,
    unsatisfiableClasses: [],
    unsatisfiable: [],
  }
}

export async function getKnowledgeProposals(): Promise<OntologyEvolutionRecordDto[]> {
  return [...STORE.proposals]
}

export async function createKnowledgeProposal(
  sourceVersionId: string,
  proposal: OntologyProposalDto,
  _runtimeIr?: unknown,
): Promise<OntologyEvolutionRecordDto> {
  const record: OntologyEvolutionRecordDto = {
    id: `prop-${Date.now().toString(36)}`,
    source_version_id: sourceVersionId,
    proposal,
    review: {
      proposal_id: proposal.id,
      decision: 'Pending',
      reviewer: null,
      reason: null,
      impact: {
        currentConsistent: true,
        targetConsistent: true,
        newlyUnsatisfiable: [],
        resolvedUnsatisfiable: [],
        addedSubsumptions: [],
        removedSubsumptions: [],
      },
    },
    applied_version_id: null,
    created_at: new Date().toISOString(),
  }
  STORE.proposals.unshift(record)
  return record
}

export async function reviewKnowledgeProposal(
  id: string,
  approve: boolean,
  reason?: string,
): Promise<OntologyEvolutionRecordDto> {
  const p = STORE.proposals.find((x) => x.id === id)
  if (!p) throw new Error(`Proposal ${id} not found`)
  p.review = {
    ...p.review,
    decision: approve ? 'Approved' : 'Rejected',
    reviewer: STORE.currentUsername,
    reason: reason || (approve ? '合规通过' : '暂不符合公理规则'),
  }
  p.reviewed_at = new Date().toISOString()
  return p
}

export async function applyKnowledgeProposal(
  id: string,
  targetVersionId: string,
): Promise<{ proposal: OntologyEvolutionRecordDto; version: KnowledgeVersionDto }> {
  const p = STORE.proposals.find((x) => x.id === id)
  if (!p) throw new Error(`Proposal ${id} not found`)
  const newVer: KnowledgeVersionDto = {
    id: targetVersionId,
    revision: STORE.knowledgeVersions.length + 10,
    created_at: new Date().toISOString(),
    ontology: { axioms: [] },
  }
  STORE.knowledgeVersions.unshift(newVer)
  p.applied_version_id = targetVersionId
  return { proposal: p, version: newVer }
}

export async function proposeKnowledgeRollback(
  proposalId: string,
  targetVersionId: string,
): Promise<OntologyEvolutionRecordDto> {
  const record: OntologyEvolutionRecordDto = {
    id: `prop-rollback-${Date.now().toString(36)}`,
    source_version_id: targetVersionId,
    proposal: { id: proposalId, changes: [] },
    review: {
      proposal_id: proposalId,
      decision: 'Approved',
      reviewer: STORE.currentUsername,
      reason: '紧急回滚',
      impact: {
        currentConsistent: true,
        targetConsistent: true,
        newlyUnsatisfiable: [],
        resolvedUnsatisfiable: [],
        addedSubsumptions: [],
        removedSubsumptions: [],
      },
    },
    applied_version_id: targetVersionId,
    created_at: new Date().toISOString(),
  }
  STORE.proposals.unshift(record)
  return record
}

// ── 业务闭环 (Business Loop) API ─────────────────────────────────────────────

export async function getBusinessGoals(_projectId: string): Promise<BusinessGoalDto[]> {
  return [...STORE.businessGoals]
}

export async function getBusinessRecommendations(
  _projectId: string,
): Promise<BusinessRecommendationDto[]> {
  return [...STORE.businessRecommendations]
}

export async function getBusinessPortfolio(_projectId: string): Promise<BusinessPathDto[]> {
  return [...STORE.businessPortfolios]
}

export async function getExpiredExecutionLeases(
  _projectId: string,
): Promise<ExecutionLeaseDto[]> {
  return [...STORE.executionLeases]
}

export async function getBusinessFollowUps(_projectId: string): Promise<FollowUpItemDto[]> {
  return [...STORE.followUps]
}

export async function getBusinessLearningInsights(
  _projectId: string,
): Promise<LearningInsightDto[]> {
  return [...STORE.learningInsights]
}

export async function getBusinessPathInsights(_projectId: string): Promise<PathInsightDto[]> {
  return [...STORE.pathInsights]
}

export async function startBusinessFollowUp(id: string): Promise<unknown> {
  const item = STORE.followUps.find((f) => f.id === id)
  if (item) item.status = 'In_Progress'
  return { ok: true }
}

export async function settleBusinessFollowUp(id: string, success: boolean): Promise<unknown> {
  const item = STORE.followUps.find((f) => f.id === id)
  if (item) item.status = success ? 'Done' : 'Failed'
  return { ok: true }
}

export async function runBusinessNext(projectId: string): Promise<BusinessRunDto> {
  STORE.businessGoals.forEach((g) => {
    if (g.gap > 0) {
      g.currentValue = Math.max(g.targetValue, Number((g.currentValue - 0.2).toFixed(1)))
      g.gap = Math.max(0, Number((g.gap - 0.2).toFixed(1)))
    }
  })
  return {
    executed: true,
    projectId,
    recommendation: STORE.businessRecommendations[0] || null,
    goals: [...STORE.businessGoals],
  }
}

export async function startBusinessOptimizationJob(
  projectId: string,
  maxCycles: number,
): Promise<AutomationJobDto> {
  const job: AutomationJobDto = {
    id: `opt-job-${Date.now().toString(36)}`,
    pack_id: projectId,
    job_type: 'backtest',
    dedupe_key: `opt-${projectId}-${maxCycles}`,
    payload: { maxCycles },
    status: 'running',
    attempt: 1,
    max_attempts: 3,
    available_at: new Date().toISOString(),
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
  STORE.automationJobs.unshift(job)
  return job
}

export async function getBusinessOptimizationRuns(
  _projectId: string,
): Promise<BusinessOptimizationRunDto[]> {
  return [...STORE.optimizationRuns]
}

export async function reconcileBusinessExecution(
  _projectId: string,
  opportunityId: string,
  leaseId: string,
): Promise<unknown> {
  const lease = STORE.executionLeases.find((l) => l.id === leaseId || l.opportunityId === opportunityId)
  if (lease) lease.status = 'Reconciled'
  return { ok: true, reconciled: true }
}

// ── 项目管理 (Projects) API ──────────────────────────────────────────────────

export async function postProjectList(): Promise<ProjectDto[]> {
  return [...STORE.projects]
}

export async function postProjectCreate(req: ProjectCreateDto): Promise<ProjectDto> {
  const p: ProjectDto = {
    id: req.id || `proj-${Date.now().toString(36)}`,
    name: req.name,
    description: req.description || '',
    ontology: req.ontology || 'custom.odl',
    knowledgeVersionId: req.knowledgeVersionId || 'v1.2.0',
    status: 'Active',
    createdAt: new Date().toISOString(),
  }
  STORE.projects.unshift(p)
  return p
}

export async function postProjectSeed(): Promise<{ events: number }> {
  return { events: 18 }
}

export async function postProjectKnowledgeVersion(
  id: string,
  versionId: string,
): Promise<ProjectDto> {
  const p = STORE.projects.find((x) => x.id === id)
  if (!p) throw new Error(`Project ${id} not found`)
  p.knowledgeVersionId = versionId
  return p
}

export async function postProjectEvent(req: ProjectEventDto): Promise<{ ok: boolean }> {
  STORE.auditLogs.unshift({
    id: `audit-${Date.now().toString(36)}`,
    eventType: 'CreateProjectObject',
    refs: [req.id, req.project],
    payload: req.input,
    timestamp: new Date().toISOString(),
  })
  return { ok: true }
}

export async function postProjectDeps(): Promise<{ relations: number }> {
  return { relations: 12 }
}

// ── Agent 通信 Fabric API ───────────────────────────────────────────────────

export async function postFabricSubmit(req: FabricRequestDto): Promise<FabricSubmissionDto> {
  return {
    result: {
      id: `msg-${Date.now().toString(36)}`,
      session_id: req.sessionId,
      sender: req.sender,
      receiver: 'retention-agent',
      kind: 'TaskResult',
      intent: req.intent,
      payload: {
        status: 'succeeded',
        stdout: '任务执行完毕，结果已更新',
      },
      capability: 'reasoning',
      correlation_id: `corr-${Date.now().toString(36)}`,
      causation_id: null,
      timestamp: Date.now(),
    },
  }
}

// ── 治理审批 (Governance) API ────────────────────────────────────────────────

export async function getRunApprovals(_runId: string): Promise<PendingApprovalDto[]> {
  return [...STORE.pendingApprovals]
}

export async function postRunApprovalDecision(
  _runId: string,
  reqId: string,
  approve: boolean,
): Promise<ApprovalDecisionDto> {
  const idx = STORE.pendingApprovals.findIndex((a) => a.reqId === reqId)
  if (idx !== -1) {
    STORE.pendingApprovals.splice(idx, 1)
  }
  STORE.auditLogs.unshift({
    id: `audit-${Date.now().toString(36)}`,
    eventType: approve ? 'ApproveRequest' : 'RejectRequest',
    refs: [reqId],
    payload: { decision: approve ? 'approve' : 'reject' },
    timestamp: new Date().toISOString(),
  })
  return {
    reqId,
    decision: approve ? 'approve' : 'reject',
    status: 'Completed',
  }
}

// ── 业务包与插件 (Domain Packs & Plugins) API ─────────────────────────────────

export async function getDomainPacks(): Promise<DomainPackDto[]> {
  return [...STORE.domainPacks]
}

export async function installDomainPack(id: string): Promise<DomainPackInstallDto> {
  const pack = STORE.domainPacks.find((p) => p.id === id)
  if (pack) pack.installed = true
  return {
    pack: pack || STORE.domainPacks[0],
    project_id: 'proj-crm-retention',
    objects_created: 14,
    objects_skipped: 0,
    relations_created: 22,
    relations_skipped: 0,
  }
}

export async function getDomainPackRuntime(id: string): Promise<DomainPackRuntimeDto> {
  return (
    STORE.domainPackRuntimes[id] || {
      pack_id: id,
      enabled: true,
      source_path: `/domain-packs/${id}`,
      watch_enabled: false,
      poll_interval_seconds: 60,
      auto_sync: true,
      auto_analyze: false,
      auto_backtest: false,
      auto_review: false,
      strategy_ids: [],
      updated_by: STORE.currentUsername,
      updated_at: new Date().toISOString(),
    }
  )
}

export async function updateDomainPackRuntime(
  id: string,
  runtime: DomainPackRuntimeUpdateDto,
): Promise<DomainPackRuntimeDto> {
  const updated: DomainPackRuntimeDto = {
    pack_id: id,
    ...runtime,
    updated_by: STORE.currentUsername,
    updated_at: new Date().toISOString(),
  }
  STORE.domainPackRuntimes[id] = updated
  return updated
}

export async function setDomainPackEnabled(
  id: string,
  enabled: boolean,
): Promise<DomainPackRuntimeDto> {
  const pack = STORE.domainPacks.find((p) => p.id === id)
  if (pack) pack.enabled = enabled
  const rt = await getDomainPackRuntime(id)
  rt.enabled = enabled
  STORE.domainPackRuntimes[id] = rt
  return rt
}

export async function getDomainPackOperations(id: string): Promise<DomainPackOperationsDto> {
  const runtime = await getDomainPackRuntime(id)
  const strategies = await getDomainPackStrategies(id)
  return {
    pack_id: id,
    runtime,
    plugins: [
      { id: 'plugin-crm-bridge', name: 'CRM Bridge', status: 'Active', capabilities: ['crm.outreach'] },
    ],
    strategies,
  }
}

export async function getDomainPackStrategies(_id: string): Promise<StrategyRegistryDto[]> {
  return [
    {
      id: 'strat-crm-01',
      code: 'CRM_CHURN_VIP_DISCOUNT',
      name: '高净值客户智能定向折算策略',
      family: 'Retention',
      mode: 'Proactive',
      description: '对高流失概率客户匹配适度折扣包',
      enabled: true,
      version: '1.2.0',
      parameters: { threshold: 0.85, max_coupon: 200 },
      backtest_count: 32,
      publishable: true,
    },
    {
      id: 'strat-lottery-hot-cold',
      code: 'LOTTERY_MARKOV_RED_BLUE',
      name: '双色球冷热马尔可夫动态决策',
      family: 'Analytics',
      mode: 'Backtest',
      description: '结合走势熵与热冷号转移概率推算最优红球组合',
      enabled: true,
      version: '1.0.4',
      parameters: { window_size: 100, entropy_decay: 0.95 },
      backtest_count: 120,
      mean_red_hits: 3.84,
      mean_blue_hits: 0.62,
      publishable: true,
    },
  ]
}

export async function getAutomationJobs(): Promise<AutomationStatusDto> {
  return {
    running: STORE.automationJobs.some((j) => j.status === 'running'),
    jobs: [...STORE.automationJobs],
  }
}

export async function getDomainPackAutomationStatus(id: string): Promise<AutomationPackStatusDto> {
  const runtime = await getDomainPackRuntime(id)
  return {
    pack_id: id,
    runtime,
    scan_state: 'healthy',
    scan_history: [],
    quality: {
      total_jobs: 48,
      succeeded_jobs: 47,
      failed_jobs: 1,
      queued_jobs: 0,
      running_jobs: 0,
      cancelled_jobs: 0,
      retry_jobs: 0,
      success_rate: 0.98,
    },
    manual_retry_count: 0,
  }
}

export async function getTrace(id: string): Promise<TraceEventDto> {
  for (const run of STORE.runs) {
    const ev = run.events.find((e) => e.id === id)
    if (ev) return ev
  }
  return {
    id,
    kind: 'Cognitive',
    eventType: 'Observation',
    timestamp: new Date().toISOString(),
    reasoning: `追踪事件 ${id} 详情`,
  }
}

export async function getAutomationJob(id: string): Promise<AutomationJobDto> {
  const job = STORE.automationJobs.find((j) => j.id === id)
  if (job) return job
  return {
    id,
    pack_id: 'crm-retention',
    job_type: 'scan',
    dedupe_key: `job-${id}`,
    payload: {},
    status: 'succeeded',
    attempt: 1,
    max_attempts: 3,
    available_at: new Date().toISOString(),
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
}

export async function cancelAutomationJob(id: string): Promise<AutomationJobDto> {
  const job = await getAutomationJob(id)
  job.status = 'cancelled'
  return job
}

export async function runDomainPack(id: string): Promise<AutomationJobDto> {
  const job: AutomationJobDto = {
    id: `job-run-${Date.now().toString(36)}`,
    pack_id: id,
    job_type: 'sync',
    dedupe_key: `run-${Date.now()}`,
    payload: {},
    status: 'succeeded',
    attempt: 1,
    max_attempts: 3,
    available_at: new Date().toISOString(),
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
  STORE.automationJobs.unshift(job)
  return job
}

export async function scanDomainPack(id: string): Promise<AutomationJobDto[]> {
  const job: AutomationJobDto = {
    id: `job-scan-${Date.now().toString(36)}`,
    pack_id: id,
    job_type: 'scan',
    dedupe_key: `scan-${Date.now()}`,
    payload: {},
    status: 'succeeded',
    attempt: 1,
    max_attempts: 3,
    available_at: new Date().toISOString(),
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
  STORE.automationJobs.unshift(job)
  return [job]
}

export async function runDomainPackOperation(
  id: string,
  operation: DomainPackOperation,
): Promise<AutomationJobDto> {
  const job: AutomationJobDto = {
    id: `job-op-${Date.now().toString(36)}`,
    pack_id: id,
    job_type: operation,
    dedupe_key: `op-${operation}-${Date.now()}`,
    payload: {},
    status: 'succeeded',
    attempt: 1,
    max_attempts: 3,
    available_at: new Date().toISOString(),
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  }
  STORE.automationJobs.unshift(job)
  return job
}

// ── Skills, Plugins & Workers API ───────────────────────────────────────────

export async function getSkills(): Promise<SkillDto[]> {
  return [...STORE.skills]
}

export async function getSkillRaw(skillId: string): Promise<RawSkillDto> {
  const skill = STORE.skills.find((s) => s.id === skillId)
  return {
    id: skillId,
    name: skill?.name || skillId,
    source_type: 'odl_builtin',
    content_hash: 'sha256:7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069',
    content: `# ${skill?.name || skillId}\n\n目标: ${skill?.goal || '认知执行与规划'}\n能力清单:\n${(skill?.capabilities || []).map((c) => `- ${c}`).join('\n')}\n`,
  }
}

export async function importSkill(
  name: string,
  content: string,
  knownCapabilities: string[],
): Promise<SkillDto> {
  const skill: SkillDto = {
    id: `skill-${Date.now().toString(36)}`,
    name,
    version: '1.0.0',
    skill_type: 'custom',
    status: 'Ready',
    goal: `基于导入内容: ${content.slice(0, 40)}...`,
    capabilities: knownCapabilities.length > 0 ? knownCapabilities : ['custom.execute'],
  }
  STORE.skills.unshift(skill)
  return skill
}

export async function getPlugins(): Promise<PluginDto[]> {
  return [...STORE.plugins]
}

export async function getWorkers(): Promise<WorkerDto[]> {
  return [...STORE.workers]
}

export async function getSessions(): Promise<SessionSummaryDto[]> {
  return [...STORE.sessions]
}

export async function getSession(sessionId: string): Promise<SessionDto> {
  return {
    session_id: sessionId,
    events: [],
  }
}

export async function createSession(sessionId: string): Promise<SessionDto> {
  const sess: SessionSummaryDto = {
    session_id: sessionId,
    events: 0,
  }
  STORE.sessions.unshift(sess)
  return {
    session_id: sessionId,
    events: [],
  }
}
