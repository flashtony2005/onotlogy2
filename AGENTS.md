# AGENTS.md

核心原则

1. 不保留向后兼容性
   直接移除废弃路径，不要添加兼容层、回退逻辑或迁移方案。
2. 选择最简单的实现
   在满足当前需求的前提下，选择最简单的方案。避免投机性的抽象、配置和间接层。
3. 分层构建系统
   从最小可用版本开始，确保端到端跑通，再在此基础上逐层叠加新能力。永远不要用未完成的复杂度去换一个已经能跑的产品。
4. 保持模块化
   组件之间职责清晰分离。
5. 优先使用成熟依赖
   当成熟、维护良好的库能降低整体复杂度或提升可靠性时，优先选用。没有充分理由不要重复造轮子。
6. 先查已有依赖
   在写自己的实现或引入新包之前，先看看项目已有的依赖能不能用。不要假设某个库没有某能力——先查文档和类型定义。
7. 做长期决策
   不要接受"暂时能用、以后再说"的权宜之计。每个架构决策都应为长期服务。
8. 参考成熟方案
   设计解决方案之前，先到 GitHub 查找成熟实现与同类开源项目，研究它们是怎么解决同类问题的。采用它们验证过的模式和约定，而不是从零发明一套方法。

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->
