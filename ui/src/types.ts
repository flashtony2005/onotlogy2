// 与 src/trace.rs / docs/05_接口文档/api-agent-trace.md 4.1 同源（camelCase，单一真相源）

export type ViewKind =
  | 'overview'
  | 'business'
  | 'world'
  | 'runs'
  | 'workers'
  | 'learning'
  | 'governance'
  | 'fabric'
  | 'projects'
  | 'knowledge'
  | 'developer'
  | 'user-management'
  | 'business-packs'
  | 'skills'
  // 旧视图保留给后端 timeline 视图钳制与兼容（新菜单不再直接暴露）。
  | 'executive'
  | 'operator'
  | 'v2'

/// V0.7 认证用户（对应 server `GET /api/v1/wb/auth/me` / 登录响应 user）。
export interface AuthUserDto {
  username: string
  role: string
}

export interface UserDto {
  id: string
  username: string
  role: string
  displayName: string
  disabled: boolean
}

export interface TraceEventDto {
  id: string
  kind: string
  eventType: string
  objectRefs?: string[]
  runId?: string
  agentId?: string
  reasoning?: string
  input?: unknown
  output?: unknown
  confidence?: number
  timestamp: string
}

export interface TimelineEvent {
  id: string
  eventType: string
  timestamp: string
  confidence?: number
  objectRefs?: string[]
  reasoning?: string
  input?: unknown
  output?: unknown
}

export interface DecisionSummary {
  mission: string
  decision: string
  risk: string
}

export interface RunTimeline {
  runId: string
  agentId: string
  status: string
  progress: number
  humanApproval: string
  events: TimelineEvent[]
  decision?: DecisionSummary
}

export interface RunStartDto {
  runId: string
  status: string
  streamUrl: string
}

export interface ApiResponse<T> {
  code: number
  msg: string
  data?: T
}


/// 已导入 Skill（V2 面板数据源：/api/v1/v2/skills）。
export interface SkillDto {
  id: string
  name: string
  version: string
  skill_type: string
  status: string
  goal: string
  capabilities: string[]
}

export interface DomainPackRuntimeDto {
  pack_id: string
  enabled: boolean
  source_path: string
  watch_enabled: boolean
  poll_interval_seconds: number
  auto_sync: boolean
  auto_analyze: boolean
  auto_backtest: boolean
  auto_review: boolean
  strategy_ids: string[]
  updated_by: string
  updated_at: string
}

export interface DomainPackRuntimeUpdateDto {
  enabled: boolean
  source_path: string
  watch_enabled: boolean
  poll_interval_seconds: number
  auto_sync: boolean
  auto_analyze: boolean
  auto_backtest: boolean
  auto_review: boolean
  strategy_ids: string[]
}

export interface DomainPackPluginDto {
  id: string
  name: string
  status: string
  capabilities: string[]
}

export interface StrategyRegistryDto {
  id: string
  code: string
  name: string
  family: string
  mode: string
  description: string
  enabled: boolean
  version: string
  parameters: Record<string, unknown>
  training_window?: number | null
  backtest_count: number
  mean_red_hits?: number | null
  mean_blue_hits?: number | null
  random_baseline_red_hits?: number | null
  random_baseline_blue_hits?: number | null
  publishable: boolean
}

export interface DomainPackOperationsDto {
  pack_id: string
  runtime: DomainPackRuntimeDto
  plugins: DomainPackPluginDto[]
  strategies: StrategyRegistryDto[]
}

export type AutomationJobStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled' | string

export type DomainPackOperation = 'sync' | 'import' | 'analyze' | 'backtest' | 'review'

export interface AutomationJobDto {
  id: string
  pack_id: string
  job_type: DomainPackOperation | string
  dedupe_key: string
  payload: Record<string, unknown>
  status: AutomationJobStatus
  attempt: number
  max_attempts: number
  available_at: string
  started_at?: string | null
  finished_at?: string | null
  last_error?: string | null
  result?: Record<string, unknown> | null
  created_at: string
  updated_at: string
}

export interface AutomationStatusDto {
  running: boolean
  jobs: AutomationJobDto[]
}

export interface AutomationScanErrorDto {
  code: string
  message: string
  source_path: string
  timestamp: string
  trace_id: string
}

export interface AutomationScanHistoryDto {
  status: string
  event_type: string
  code?: string | null
  message?: string | null
  fingerprint?: string | null
  source_path?: string | null
  timestamp: string
  trace_id: string
  job_id?: string | null
}

export interface AutomationScanSummaryDto {
  source_rows?: number | null
  new_issue_count?: number | null
  duplicate_issues?: number | null
  fingerprint?: string | null
  timestamp: string
  trace_id?: string | null
}

export interface AutomationManualRetryDto {
  actor: string
  retry_number: number
  status: string
  submitted_jobs: number
  timestamp: string
  trace_id: string
}

export interface AutomationQualityDto {
  total_jobs: number
  succeeded_jobs: number
  failed_jobs: number
  queued_jobs: number
  running_jobs: number
  cancelled_jobs: number
  retry_jobs: number
  success_rate: number
  last_review_status?: string | null
  last_review_at?: string | null
}

export interface AutomationPackStatusDto {
  pack_id: string
  runtime: DomainPackRuntimeDto
  fingerprint?: string | null
  last_success_at?: string | null
  last_scan_error?: AutomationScanErrorDto | null
  active_scan_error?: AutomationScanErrorDto | null
  scan_state: 'never' | 'healthy' | 'failed' | 'recovered' | string
  scan_history: AutomationScanHistoryDto[]
  last_scan_summary?: AutomationScanSummaryDto | null
  quality: AutomationQualityDto
  manual_retry_count: number
  last_manual_retry?: AutomationManualRetryDto | null
}

export interface DomainPackDto {
  id: string
  name: string
  version: string
  minimum_runtime?: string | null
  skill_ids: string[]
  status: 'available' | 'installed' | 'invalid' | string
  valid: boolean
  installed: boolean
  ontology_types: number
  bootstrap_objects: number
  bootstrap_relations: number
  enabled: boolean
  plugin_ids: string[]
  plugin_capabilities: string[]
  error?: string | null
}

export interface DomainPackInstallDto {
  pack: DomainPackDto
  project_id: string
  objects_created: number
  objects_skipped: number
  relations_created: number
  relations_skipped: number
}

export interface PluginDto {
  id: string
  name: string
  status: string
  runtime: string
  endpoint?: string | null
  capabilities: string[]
}

export interface SessionEventDto {
  seq: number
  event_type: string
  timestamp: number
  data: Record<string, unknown>
}

export interface WorkerDto {
  id: string
  worker_type: string
  capabilities: string[]
  execution: string
  quality: number
  cost: number
  status: string
}

export interface RawSkillDto {
  id: string
  name: string
  source_type: string
  content_hash: string
  content: string
}

export interface SessionSummaryDto {
  session_id: string
  events: number
}

export interface SessionDto {
  session_id: string
  events: SessionEventDto[]
}

// ── World 视图（GET /api/v1/wb/graph）───────────────────────────
export interface WorldNodeDto {
  id: string
  type: string
  data?: unknown
}

export interface WorldEdgeDto {
  id: string
  source: string
  target: string
  type: string
  data?: unknown
}

export interface WorldGraphDto {
  nodes: WorldNodeDto[]
  edges: WorldEdgeDto[]
}


// ── Phase 7 Facade DTO（对应 server /api/v1/facade/*；camelCase wire）──

export interface WorldRelationDto {
  source: string
  predicate: string
  target: string
  validFrom: number
  validUntil: number | null
}

export interface WorldViewDto {
  relations: WorldRelationDto[]
  total: number
  at: number
}

export interface WorldQueryDto {
  projectId?: string
  objectRef?: string
  predicate?: string
  limit?: number
}

export interface AuditEventDto {
  id: string
  eventType: string
  refs: string[]
  payload: unknown
  timestamp: string
}

export interface AuditViewDto {
  events: AuditEventDto[]
  total: number
}

export interface AuditQueryDto {
  projectId?: string
  runId?: string
  agentId?: string
  eventType?: string
  limit?: number
}

export interface ExecutionContextDto {
  workspace: {
    root: string
    mode: 'isolated'
  }
  cwd?: string
  env?: Record<string, string>
  timeout_ms?: number
}

export interface FabricRequestDto {
  sessionId: string
  sender: string
  intent: string
  payload: unknown
  executionContext?: ExecutionContextDto
  projectId?: string
}

export interface WorkspaceDiffDto {
  available: boolean
  added: string[]
  modified: string[]
  deleted: string[]
  counts: { added: number; modified: number; deleted: number }
}

export interface TestSummaryDto {
  detected: boolean
  passed?: number
  failed?: number
  skipped?: number
  total?: number
  status?: string
}

export interface FabricTaskResultPayload {
  status?: string
  stdout?: string
  stderr?: string | null
  exit_code?: number | null
  progress_count?: number
  duration_ms?: number
  structured_output?: unknown
  task_spec?: Record<string, unknown> | null
  workspace_diff?: WorkspaceDiffDto
  test_summary?: TestSummaryDto
  failure_category?: string | null
  correlation_id?: string
  error?: string | null
}

export interface FabricSubmissionDto {
  // AgentMessage wire 为 snake_case（契约示例）；此处照实际字段声明
  result: {
    id: string
    session_id: string
    sender: string
    receiver: string
    kind: string
    intent: string | null
    payload: FabricTaskResultPayload
    capability: string | null
    correlation_id: string | null
    causation_id: string | null
    timestamp: number
  }
}

// ── T1 Bridge 审批裁决（GET /run/:run_id/approvals + POST /run/:run_id/approval/:req_id）──

/// 挂起审批列表项（后端 PendingApprovalDto，camelCase wire）。
export interface PendingApprovalDto {
  reqId: string
  runId: string
  capability: string
  action: string
  args: unknown
  state: string
}

/// 裁决结果（后端 resolve_approval 成功体）。
export interface ApprovalDecisionDto {
  reqId: string
  decision: string
  status?: string
  sender?: string
}

// ── Phase 8 Project（项目 = 世界/事件/运营的顶层组织单元）──

export interface ProjectDto {
  id: string
  name: string
  description: string
  ontology: string
  status: string
  createdAt: string
}

export interface ProjectCreateDto {
  id: string
  name: string
  description?: string
  ontology?: string
}

/// 给项目世界加对象（`oaip facade project event` 的 HTTP 等价）。
export interface ProjectEventDto {
  objectType: string
  id: string
  project: string
  /// 对象载荷（任意 JSON）。
  input: unknown
  /// 可选：root --edge--> 此对象（如 contains）。
  edge?: string
}

// ── Knowledge 视图（GET /api/v1/knowledge/*）─────────────────────────────
export interface KnowledgeVersionDto {
  revision: number
  id: string
  parent_id?: string | null
  ontology: { axioms: unknown[] }
  runtime_ir?: unknown | null
  created_at: string
}

export interface ReasonedJustificationDto {
  axiom_indices: number[]
  axioms: unknown[]
}

export interface UnsatisfiableClassDto {
  class: string
  justification: ReasonedJustificationDto
}

export interface KnowledgeDiagnosisDto {
  consistent: boolean
  unsatisfiableClasses: string[]
  unsatisfiable: UnsatisfiableClassDto[]
  inconsistency?: ReasonedJustificationDto | null
}

export type KnowledgeReviewDecision = 'Pending' | 'Approved' | 'Rejected'

export interface OntologyImpactDto {
  currentConsistent: boolean
  targetConsistent: boolean
  newlyUnsatisfiable: string[]
  resolvedUnsatisfiable: string[]
  addedSubsumptions: [string, string][]
  removedSubsumptions: [string, string][]
}

export interface OntologyReviewDto {
  proposal_id: string
  impact: OntologyImpactDto
  decision: KnowledgeReviewDecision
  reviewer?: string | null
  reason?: string | null
}

export interface OntologyProposalDto {
  id: string
  changes: Array<Record<string, unknown>>
}

export interface OntologyEvolutionRecordDto {
  id: string
  source_version_id: string
  proposal: OntologyProposalDto
  review: OntologyReviewDto
  candidate_runtime_ir?: unknown | null
  applied_version_id?: string | null
  created_at: string
  reviewed_at?: string | null
}

export interface BusinessGoalDto {
  id: string
  targetMetric: string
  targetValue: number
  currentValue: number
  gap: number
  status: string
}

export type BusinessActor = 'agent' | 'human'

export interface BusinessCandidateDto {
  action: string
  actor: BusinessActor
  requiresApproval: boolean
  params: unknown[]
  expectedBenefit: number
  cost: number
  confidence: number
  netValue: number
  outcomeRuns: number
  outcomeSuccesses: number
  resources?: Record<string, number>
  plugin?: string
  worker?: string
  capability?: string
}

export interface BusinessRecommendationDto {
  actor: BusinessActor
  projectId: string
  opportunityId: string
  subject: string
  action: string
  score: number
  confidence: number
  reason: string
  candidates: BusinessCandidateDto[]
}

export interface BusinessPathDto {
  projectId: string
  opportunityId: string
  subject: string
  actions: string[]
  requiresHuman: boolean
  requiresApproval: boolean
  projectedCost: number
  projectedNetValue: number
  resources?: Record<string, number>
  goalMetric?: string
  confidence: number
  score: number
  reason: string
}

export interface ExecutionLeaseDto {
  id: string
  projectId: string
  opportunityId: string
  owner: string
  status: string
  expiresAt: string
}

export interface FollowUpItemDto {
  id: string
  objectType: string
  subject: string
  status: string
  decisionId?: string
  action?: string
}

export interface BusinessRunDto {
  executed: boolean
  projectId: string
  expiredExecutionLeases?: ExecutionLeaseDto[]
  recommendation?: BusinessRecommendationDto | null
  decision?: unknown
  execution?: unknown
  goals: BusinessGoalDto[]
}
