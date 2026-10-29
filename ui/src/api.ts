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
      name: 'AuraGlobal 全球跨境多仓供应链智能调度',
      description: '面向北美/欧洲海外仓网的黑五备货、跨仓空运平衡与断货防御协同引擎。',
      ontology: 'crossborder_supply.odl',
      knowledgeVersionId: 'v2.1.0',
      status: 'Active',
      createdAt: '2026-03-15T08:00:00Z',
    },
    {
      id: 'proj-auraglobal-pricing',
      name: 'AuraGlobal 欧洲/北美动态汇率与 VAT 财税对冲',
      description: '基于欧元/美元汇率波动、欧盟一站式增值税申报 (OSS) 与底线毛利守恒不变式。',
      ontology: 'crossborder_pricing.odl',
      knowledgeVersionId: 'v1.5.0',
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
      id: 'skill-crossborder-rebalance',
      name: '跨境海外仓智能调拨与安全库存守恒',
      version: '2.1.0',
      skill_type: 'cognitive',
      status: 'Ready',
      goal: '实时监控美西/美东/汉堡海外仓库存，推导断货损失并规划最优海空运平衡方案',
      capabilities: ['stockout.forecast', 'airfreight.plan', 'margin.preserve'],
    },
    {
      id: 'skill-vat-currency-hedging',
      name: '欧盟 VAT 关税与动态汇率毛利对冲',
      version: '1.5.0',
      skill_type: 'analytics',
      status: 'Ready',
      goal: '计算欧盟多国 VAT 税率与汇率对冲下的底线保全定价区间',
      capabilities: ['vat.calculate', 'fx.hedge', 'promo.discount.bound'],
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
      id: 'plugin-shopify-global',
      name: 'Shopify 全球多站点订单与履约网关',
      status: 'Active',
      runtime: 'node-v20',
      capabilities: ['shopify.orders.read', 'shopify.inventory.sync', 'shopify.price.update'],
      endpoint: 'https://api.shopify.auraglobal.internal/v2',
    },
    {
      id: 'plugin-fba-multi-warehouse',
      name: 'Amazon FBA / 4PL 区域海外仓 WMS 适配器',
      status: 'Active',
      runtime: 'mcp-std',
      capabilities: ['wms.inventory.probe', 'fba.shipment.create', 'customs.declaration'],
      endpoint: 'mcp://wms-gateway.us.auraglobal:9001',
    },
    {
      id: 'plugin-fedex-air-freight',
      name: 'FedEx / DHL 跨境特快货运专机专线接口',
      status: 'Active',
      runtime: 'http-proxy',
      capabilities: ['carrier.rate.query', 'carrier.booking.dispatch', 'carrier.airwaybill'],
      endpoint: 'https://freight.fedex.enterprise.internal/dispatch',
    },
  ],
  workers: [
    {
      id: 'worker-global-logistics-01',
      worker_type: 'GlobalLogisticsDispatcher',
      capabilities: ['airfreight.plan', 'carrier.booking.dispatch', 'fba.shipment.create'],
      execution: 'fiber-pool',
      quality: 0.995,
      cost: 0.04,
      status: 'Active',
    },
    {
      id: 'worker-pricing-engine-02',
      worker_type: 'PricingAndVATHedger',
      capabilities: ['vat.calculate', 'fx.hedge', 'shopify.price.update'],
      execution: 'async-worker',
      quality: 0.989,
      cost: 0.02,
      status: 'Idle',
    },
    {
      id: 'worker-inventory-monitor-03',
      worker_type: 'InventoryWatermarkProbe',
      capabilities: ['wms.inventory.probe', 'stockout.forecast'],
      execution: 'compute-cluster',
      quality: 0.992,
      cost: 0.03,
      status: 'Active',
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
      id: 'auraglobal-supply-pack',
      name: 'AuraGlobal 跨境全球多仓供应链协同业务包',
      version: '2.1.0',
      skill_ids: ['skill-crossborder-rebalance'],
      status: 'installed',
      valid: true,
      installed: true,
      enabled: true,
      ontology_types: 12,
      bootstrap_objects: 26,
      bootstrap_relations: 38,
      plugin_ids: ['plugin-shopify-global', 'plugin-fba-multi-warehouse'],
      plugin_capabilities: ['shopify.orders.read', 'wms.inventory.probe', 'fba.shipment.create'],
    },
    {
      id: 'auraglobal-tax-hedging-pack',
      name: '跨境电商关税与 VAT 动态汇率对冲业务包',
      version: '1.5.0',
      skill_ids: ['skill-vat-currency-hedging'],
      status: 'installed',
      valid: true,
      installed: true,
      enabled: true,
      ontology_types: 8,
      bootstrap_objects: 18,
      bootstrap_relations: 24,
      plugin_ids: ['plugin-shopify-global'],
      plugin_capabilities: ['shopify.price.update', 'vat.calculate'],
    },
  ],
  domainPackRuntimes: {
    'auraglobal-supply-pack': {
      pack_id: 'auraglobal-supply-pack',
      enabled: true,
      source_path: '/domain-packs/auraglobal-supply-pack',
      watch_enabled: true,
      poll_interval_seconds: 30,
      auto_sync: true,
      auto_analyze: true,
      auto_backtest: true,
      auto_review: false,
      strategy_ids: ['strat-airfreight-roi-01', 'strat-stockout-defense-02'],
      updated_by: 'developer',
      updated_at: '2026-04-14T10:00:00Z',
    },
    'auraglobal-tax-hedging-pack': {
      pack_id: 'auraglobal-tax-hedging-pack',
      enabled: true,
      source_path: '/domain-packs/auraglobal-tax-hedging-pack',
      watch_enabled: false,
      poll_interval_seconds: 60,
      auto_sync: true,
      auto_analyze: false,
      auto_backtest: true,
      auto_review: false,
      strategy_ids: ['strat-vat-margin-floor'],
      updated_by: 'operator',
      updated_at: '2026-04-12T15:30:00Z',
    },
  },
  businessGoals: [
    {
      id: 'bg-1',
      targetMetric: '全球跨境跨国准时履约率 (OTD %)',
      currentValue: 86.4,
      targetValue: 95.0,
      gap: 8.6,
      status: 'In_Progress',
    },
    {
      id: 'bg-2',
      targetMetric: '全渠道税后毛利率 (Gross Margin %)',
      currentValue: 23.5,
      targetValue: 28.0,
      gap: 4.5,
      status: 'In_Progress',
    },
    {
      id: 'bg-3',
      targetMetric: '北美/欧洲海外仓周转天数 (DSI Days)',
      currentValue: 42.0,
      targetValue: 28.0,
      gap: 14.0,
      status: 'In_Progress',
    },
    {
      id: 'bg-4',
      targetMetric: '单次紧急调拨空运成本控制 ($/件)',
      currentValue: 5.0,
      targetValue: 6.2,
      gap: 0,
      status: 'Compliant',
    },
  ],
  businessRecommendations: [
    {
      actor: 'agent',
      projectId: 'proj-auraglobal-supply',
      opportunityId: 'opp-airfreight-01',
      subject: 'wh:Warehouse_US_West',
      action: '启动美东至美西 2,500 件 65W 氮化镓疾充头紧急空运调拨',
      score: 0.96,
      confidence: 0.98,
      reason: '美西长滩仓现存量仅剩 320 件，若断货将导致黑五爆款权重下挫并损失 $142,000 GMV',
      candidates: [
        {
          action: '跨仓空运特快调拨',
          actor: 'agent',
          requiresApproval: true,
          params: ['AuraPro_GaN65W', 2500, 'US_East_To_West'],
          expectedBenefit: 142000,
          cost: 12500,
          confidence: 0.98,
          netValue: 129500,
          outcomeRuns: 46,
          outcomeSuccesses: 44,
        },
      ],
    },
    {
      actor: 'human',
      projectId: 'proj-auraglobal-pricing',
      opportunityId: 'opp-vat-02',
      subject: 'market:EU_Germany',
      action: '为欧洲站 Solar1000 户外电源核准 15% 绿电季 VAT 汇率对冲补贴',
      score: 0.92,
      confidence: 0.94,
      reason: '对冲近期欧元 4.2% 贬值及 19% 德国代扣 VAT，抢占欧洲春季露营季先机',
      candidates: [],
    },
  ],
  businessPortfolios: [
    {
      projectId: 'proj-auraglobal-supply',
      opportunityId: 'opp-airfreight-01',
      subject: 'wh:Warehouse_US_West',
      actions: ['跨仓空运平衡', '承运商舱位锁定'],
      requiresHuman: true,
      requiresApproval: true,
      projectedCost: 12500,
      projectedNetValue: 112460,
      confidence: 0.98,
      explorationBonus: 0.05,
      score: 0.96,
      outcomeRuns: 46,
      outcomeSuccesses: 44,
      reason: '断货保全 ROI 极高 (1:11.3)',
    },
  ],
  executionLeases: [
    {
      id: 'lease-901',
      projectId: 'proj-auraglobal-supply',
      opportunityId: 'opp-airfreight-01',
      owner: 'global-supply-agent',
      expiresAt: '2026-04-18T12:00:00Z',
      status: 'Active',
    },
  ],
  followUps: [
    {
      id: 'fu-1',
      objectType: 'ShipmentTracking',
      subject: '追踪 FedEx 跨境专机 2,500 件 GaN65W 美西长滩仓打托入库进度',
      status: 'Pending',
      action: 'TrackAirwayBill',
    },
    {
      id: 'fu-2',
      objectType: 'TaxAudit',
      subject: '审计德国汉堡仓欧盟一站式 VAT 报税清关回执与关税完税凭证',
      status: 'Done',
      action: 'VerifyCustomsReceipt',
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
      reqId: 'appr-aura-001',
      runId: 'run-aura-901',
      capability: 'logistics.airfreight.transfer',
      action: 'EmergencyAirFreightRebalance',
      args: {
        skuId: 'sku:AuraPro_GaN65W',
        sourceWh: 'wh:Warehouse_US_East',
        targetWh: 'wh:Warehouse_US_West',
        qty: 2500,
        carrier: 'FedEx_CrossBorder_Express',
        freightCost: 12500,
      },
      state: 'Pending',
      targetEntity: {
        id: 'wh:Warehouse_US_West',
        name: '美西长滩海外仓 (FBA-ONT8 · 核心履约枢纽)',
        type: '海外仓 (Warehouse)',
        badgeColor: '#059669',
      },
      intentTitle: '紧急启动美东至美西 2,500 件 65W 氮化镓疾充头跨仓空运平衡，阻断黑五断货降权海啸',
      businessAttribution:
        '美西长滩仓现存量仅剩 320 件（仅够销售 18 小时），原定美森海运补货受巴拿马运河通行配额收紧影响延误 14 天；若断货将导致 Listing 降权并损失黑五核心时段 $142,000 GMV。',
      formalInvariant: {
        ruleId: 'POLICY-CROSSBORDER-LOGISTICS-001',
        ruleName: '紧急空运增量投入产出与断货保全值守恒公理',
        formula: 'Cost_AirFreight ($12,500) < Loss_Stockout ($142,000) × P_Conv (0.88) ∧ UnitMarginPostAir ≥ 22.4%',
        proofConclusion:
          '形式化断言判定：单件空运增量成本 $5.00 挽回单件潜在毛利 $28.50，净收益保全高达 $112,460；但申请调拨总额 $12,500 超出日常自动化放行上限 ($10,000)，命中财务预算风控红线。',
        requiresHumanReviewReason: '空运单项预算溢出 25%，按治理公理需由出海全球供应链总监签署形式化豁免授权。',
      },
      paramTranslations: [
        {
          param: 'skuId',
          label: '目标爆款物料',
          newValue: '65W 氮化镓三口疾充充电头 (sku:AuraPro_GaN65W)',
          explanation: '出海北美站销量第一的明星单品，黑五大促核心主推款',
        },
        {
          param: 'sourceWh',
          label: '调出海外仓',
          oldValue: '美东新泽西海外仓 (当前积压 4,800 件，周转天数 68 天)',
          newValue: '美东新泽西海外仓 (调拨后库存保持 2,300 件，处于安全水位)',
          explanation: '美东仓动销缓慢，调出 2,500 件可有效削减长期仓储滞销费 (LTSF)',
        },
        {
          param: 'targetWh',
          label: '调入海外仓',
          oldValue: '美西长滩海外仓 (现存量 320 件，极度危险)',
          newValue: '美西长滩海外仓 (调入后达 2,820 件，可平稳覆盖黑五网一)',
          explanation: '覆盖美西加州与西雅图核心买家，保障当日达/次日达配送承诺',
        },
        {
          param: 'transferQty',
          label: '紧急调拨件数',
          newValue: '2,500 件 (共 5 个标准打托托盘)',
          explanation: '经销量预测推演，恰好支撑美西海运大货到港前的 12 天真空期',
        },
        {
          param: 'freightCost',
          label: '专机空运总费用',
          oldValue: '常规海运 $0.80/件 (总计 $2,000)',
          newValue: '$12,500 (单件折合 $5.00，溢出预算 $2,500)',
          explanation: '锁定 FedEx 跨国包机舱位，36 小时门到门极速入仓上架',
        },
      ],
      businessImpact: {
        expectedGain: '保全核心 Listing BestSeller 权重，锁定 $142,000 销售额，履约率升至 96.8%',
        budgetCost: '发生一次性增量物流运费 $12,500（占保全 GMV 比例仅 8.8%）',
        roiSummary: '预期 ROI 为 1:11.3（净保全毛利 $112,460）',
        riskLevel: '低风险',
        riskMitigation: '已与 FedEx 签署空运时效延误赔付条款，入库优先通道已预报备。',
      },
    },
    {
      reqId: 'appr-aura-002',
      runId: 'run-aura-902',
      capability: 'pricing.vat.hedge',
      action: 'ApplyVATHedgingDiscount',
      args: {
        skuId: 'sku:AuraPower_Solar1000',
        market: 'market:EU_Germany',
        discountPercent: 15,
        currency: 'EUR',
      },
      state: 'Pending',
      targetEntity: {
        id: 'market:EU_Germany',
        name: 'Shopify 德国站点 (DE · 欧盟核心站点)',
        type: '区域市场 (Market)',
        badgeColor: '#2563eb',
      },
      intentTitle: '申请为 1000Wh 户外储能电源破格发放 15% 绿电季 VAT 汇率对冲促销折扣',
      businessAttribution:
        '近期欧元兑美元汇率走弱贬值 4.2%，加之德国实行 19% VAT 代扣代缴，竞品在欧洲本土大幅打折降价；适度调价对冲能抢占露营旺季市场份额，并盘活德国汉堡仓库存。',
      formalInvariant: {
        ruleId: 'POLICY-VAT-MARGIN-FLOOR-002',
        ruleName: '欧盟站代扣 VAT 与关税后最低利润率保护公理',
        formula: 'GM_post_tax = [Price × (1 - Discount) - COGS - Tariff - VAT] / [Price × (1 - Discount)] ≥ 18.0%',
        proofConclusion:
          '形式化断言判定：在 15% 折扣下，税后实收净毛利率测算为 19.2% (严格 ≥ 18.0% 底线红线)，符合价格守恒规则；但单次促销折扣跨度达 15% (超日常自动化限额 10%)。',
        requiresHumanReviewReason: '折扣力度超过日常自动化策略阈值，需欧洲大区操盘负责人签署确认。',
      },
      paramTranslations: [
        {
          param: 'skuId',
          label: '促销物料对象',
          newValue: '1000Wh 户外储能电源 (sku:AuraPower_Solar1000)',
          explanation: '欧洲绿电与春季露营季主力单品，单台客单价 €899',
        },
        {
          param: 'market',
          label: '目标销售市场',
          newValue: 'Shopify 德国站 (DE)',
          explanation: '德国本土代扣代缴 19% 增值税 (VAT)，采用 OSS 一站式申报',
        },
        {
          param: 'discountPercent',
          label: '拟申请对冲折扣',
          oldValue: '0% (常规限额 ≤ 10%)',
          newValue: '15% 绿电季专属补贴 (折后 €764.15)',
          explanation: '让利 €134.85，有效压制 Anker / Jackery 欧洲竞品',
        },
        {
          param: 'postTaxMargin',
          label: '税后净毛利率',
          newValue: '19.2% (高于 18.0% 止损线)',
          explanation: '已完全计提 €122 德国 VAT、€38 进口关税及 €320 生产采购成本',
        },
      ],
      businessImpact: {
        expectedGain: '预计欧洲大区春季露营季订单量激增 45%，抢占 12% 德语区市场份额',
        budgetCost: '单台让利毛利约 €134.85，但提升汉堡仓库存周转 30 天',
        roiSummary: '通过规模效应摊薄仓储底价，综合利润总额提升 3.8%',
        riskLevel: '中风险',
        riskMitigation: '限时 72 小时且限定德国本地 IP 领券，单 IP 限制 1 台，防跨国转运套利。',
      },
    },
  ],
  auditLogs: [
    {
      id: 'audit-001',
      timestamp: '2026-04-15T11:15:20Z',
      eventType: 'EvaluateOverseasStockoutRisk',
      refs: ['wh:Warehouse_US_West', 'sku:AuraPro_GaN65W'],
      payload: { actor: 'global-supply-agent', result: 'Alert (RemainingStock: 320 pcs, Cover: 18 hours)' },
    },
    {
      id: 'audit-002',
      timestamp: '2026-04-15T11:15:22Z',
      eventType: 'CheckAirFreightROIPolicy',
      refs: ['policy:air_freight_roi_rule'],
      payload: { actor: 'policy-gate', result: 'Pass (AirFreight $12.5k < StockoutLoss $142k * 0.88)' },
    },
    {
      id: 'audit-003',
      timestamp: '2026-04-15T11:15:24Z',
      eventType: 'CheckBudgetPolicy',
      refs: ['policy:logistics_budget_cap'],
      payload: { actor: 'policy-gate', result: 'TriggerApproval (Amount $12,500 > Cap $10,000)' },
    },
  ],
  worldRelations: [
    { source: 'sku:AuraPro_GaN65W', predicate: 'sells_in', target: 'market:US_NorthAmerica', validFrom: 1713170000, validUntil: null },
    { source: 'sku:AuraPro_GaN65W', predicate: 'stocked_at', target: 'wh:Warehouse_US_West', validFrom: 1713170000, validUntil: null },
    { source: 'sku:AuraPro_GaN65W', predicate: 'stocked_at', target: 'wh:Warehouse_US_East', validFrom: 1713170000, validUntil: null },
    { source: 'wh:Warehouse_US_West', predicate: 'has_risk', target: 'alert:Stockout_BlackFriday_US', validFrom: 1713170000, validUntil: null },
    { source: 'alert:Stockout_BlackFriday_US', predicate: 'mitigated_by', target: 'action:Transfer_US_East_To_West', validFrom: 1713170000, validUntil: null },
    { source: 'policy:air_freight_roi_rule', predicate: 'governs', target: 'action:Transfer_US_East_To_West', validFrom: 1713170000, validUntil: null },
    { source: 'sku:AuraPower_Solar1000', predicate: 'sells_in', target: 'market:EU_Germany', validFrom: 1713170000, validUntil: null },
    { source: 'sku:AuraPower_Solar1000', predicate: 'stocked_at', target: 'wh:Warehouse_EU_Hamburg', validFrom: 1713170000, validUntil: null },
    { source: 'market:EU_Germany', predicate: 'enforces', target: 'policy:vat_margin_floor_rule', validFrom: 1713170000, validUntil: null },
    { source: 'agent:global-supply-agent', predicate: 'manages', target: 'wh:Warehouse_US_West', validFrom: 1713170000, validUntil: null },
    { source: 'agent:crossborder-pricing-agent', predicate: 'governs', target: 'market:EU_Germany', validFrom: 1713170000, validUntil: null },
  ],
  runs: [],
  automationJobs: [
    {
      id: 'job-auto-01',
      pack_id: 'auraglobal-supply-pack',
      job_type: 'scan',
      dedupe_key: 'supply-scan-20260415',
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
      pack_id: 'auraglobal-tax-hedging-pack',
      job_type: 'backtest',
      dedupe_key: 'tax-hedge-20260414',
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
      confidence: 0.97,
      output: { intent: 'CrossBorderInventoryRebalance', urgency: 'Critical', scope: ['SupplyChain', 'US_West', 'FBA_ONT8'] },
      reasoning: '语义拆解：识别北美美西长滩海外仓严重断货风险，提取跨仓空运时效与保全预算红线。',
    },
    {
      kind: 'Cognitive',
      eventType: 'OntologyQuery',
      agentId,
      runId,
      confidence: 0.98,
      objectRefs: [
        'wh:Warehouse_US_West',
        'wh:Warehouse_US_East',
        'sku:AuraPro_GaN65W',
        'policy:air_freight_roi_rule',
      ],
      output: { matchedEntities: 4, consistency: true },
      reasoning: '检索本体世界：召回美西现存量 (320件)、美东积压量 (4,800件) 及空运毛利守恒公理。',
    },
    {
      kind: 'Cognitive',
      eventType: 'Reasoning',
      agentId,
      runId,
      confidence: 0.95,
      objectRefs: ['action:Transfer_US_East_To_West'],
      output: { stockoutLossGMV: 142000, airFreightCost: 12500, projectedROI: 11.3 },
      reasoning: '形式化推导：单件增量空运费 $5.00 挽回单件净毛利 $28.50，形式证明收益远大于零。',
    },
    {
      kind: 'Cognitive',
      eventType: 'Planning',
      agentId,
      runId,
      confidence: 0.96,
      output: {
        steps: [
          '1. 锁定 FedEx 跨境特快货运专机舱位 (运费 $12,500)',
          '2. 美东新泽西海外仓分拣打托出库 2,500 件 GaN65W',
          '3. 报备美西长滩 FBA 仓优先收货上架通道 (36小时内可售)',
        ],
      },
      reasoning: '规划 3 阶段跨仓空运平衡管线并绑定承运商延误赔付兜底协议。',
    },
    {
      kind: 'Cognitive',
      eventType: 'PolicyCheck',
      agentId,
      runId,
      confidence: 1.0,
      objectRefs: ['policy:air_freight_roi_rule'],
      input: { appliedCost: 12500, autoApprovalCap: 10000, marginPreserveRatio: '22.4%' },
      output: 'Allow',
      reasoning: '合规治理闸门检查：ROI 不等式成立；因单项预算超过日常自动放行限额已挂起高管凭据。',
    },
    {
      kind: 'Cognitive',
      eventType: 'Action',
      agentId,
      runId,
      confidence: 0.98,
      objectRefs: ['worker:global-logistics-01', 'action:Transfer_US_East_To_West'],
      output: { taskStatus: 'Dispatched', executionId: `exec-${Date.now().toString(36)}` },
      reasoning: '派发执行：调度 worker:global-logistics-01 执行包机舱位预占与 WMS 出库指令。',
    },
    {
      kind: 'Cognitive',
      eventType: 'MemoryUpdate',
      agentId,
      runId,
      confidence: 0.99,
      output: { memoryHits: 1, updatedAxioms: 1, status: 'Success' },
      reasoning: '认知闭环完成：将本次黑五跨仓空运应急决策经验沉淀至跨境供应链知识库。',
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
    { id: 'market:US_NorthAmerica', type: 'Market' },
    { id: 'market:EU_Germany', type: 'Market' },
    { id: 'wh:Warehouse_US_West', type: 'Warehouse' },
    { id: 'wh:Warehouse_US_East', type: 'Warehouse' },
    { id: 'wh:Warehouse_EU_Hamburg', type: 'Warehouse' },
    { id: 'sku:AuraPro_GaN65W', type: 'SKU' },
    { id: 'sku:AuraPower_Solar1000', type: 'SKU' },
    { id: 'alert:Stockout_BlackFriday_US', type: 'RiskAlert' },
    { id: 'action:Transfer_US_East_To_West', type: 'Action' },
    { id: 'policy:air_freight_roi_rule', type: 'Policy' },
    { id: 'policy:vat_margin_floor_rule', type: 'Policy' },
    { id: 'agent:global-supply-agent', type: 'Agent' },
    { id: 'agent:crossborder-pricing-agent', type: 'Agent' },
    { id: 'worker:global-logistics-01', type: 'Worker' },
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
