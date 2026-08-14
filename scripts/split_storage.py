#!/usr/bin/env python3
"""storage.rs → storage/ 目录切分（纯机械搬移，行为零变化）。"""
import os, shutil

SRC = "src/storage.rs"
OUT = "src/storage"
lines = open(SRC).read().split("\n")

def seg(a, b):  # [a, b) 1-based
    block = lines[a - 1 : b - 1]
    while block and block[-1].strip() == "":
        block.pop()
    return "\n".join(block)

# 1) mod.rs：核心类型 + trait 定义 + struct + 初始化 impl + row_to_* helpers
mod_parts = [
    seg(17, 145),      # REL_SEQ…impl TraceQuery
    seg(145, 185),  # #[async_trait] + ObjectEngine / RelationEngine trait 定义
    seg(185, 280) + "\n}",  # LibSQLStorage struct + SCHEMA_VERSION + impl(open…from_connection) + 闭合
    seg(828, 893),     # row_to_pattern / row_to_proposal / row_to_learned（#[async_trait] 归 objects）
    seg(1327, 1385),   # row_to_object / row_to_relation / row_to_memory / row_to_trace
    seg(1574, len(lines) + 1),  # tests
    seg(822, 828),  # escape_like（LIKE 通配符转义，供 events/evolution 使用）
]

# 2) objects.rs：列表/渲染 + ObjectEngine impl + hard_delete
objects_parts = [
    seg(410, 457),     # list_all_objects / list_all_relations / render_object_list
    seg(893, 1004),    # #[async_trait] + impl ObjectEngine for LibSQLStorage + hard_delete
]

# 3) events.rs：trace 事件查询
events_parts = [seg(283, 408)]

# 4) memory.rs
memory_parts = [seg(460, 512)]

# 5) evolution.rs：pattern/proposal/learned/audit + escape_like
evolution_parts = [seg(513, 820)]

# 6) runs.rs：run/approval 持久化 + recover
runs_parts = [seg(1005, 1030), seg(1030, 1202), seg(1203, 1234)]  # RunRecord/ApprovalRecord + impl + row_to_*

# 7) relations.rs
relations_parts = [seg(1234, 1326)]

# 8) users.rs：user/session/graph + user_from_row
users_parts = [seg(1385, 1562), seg(1562, 1573)]

DOC = """//! 存储层 —— ObjectEngine 抽象 + LibSQLStorage（Turso 落地）（对应 REQ-004 / REQ-005）
//!
//! 对象统一存 `wb_ontology_object`（语义ID）；TraceEvent 统一存 `wb_trace_event`。
//! 使用 libSQL 0.9 异步 API。列名与 sql-script.md 权威 DDL 一致（type / source / relation / target）。
//! 另用 `wb_meta.schema_version` 记录 schema 版本，open 据此拦截旧库（WB2101）。
//!
//! 模块拆分（长文件模块化）：核心类型/初始化在 `mod`，对象在 `objects`，关系在 `relations`，
//! 事件在 `events`，记忆在 `memory`，演化在 `evolution`，运行态在 `runs`，账号会话在 `users`。

"""

MOD_HEAD = """pub mod events;
pub mod evolution;
pub mod memory;
pub mod objects;
pub mod relations;
pub mod runs;
pub mod users;

pub(crate) use crate::runtime::TraceEvent;
pub use runs::{ApprovalRecord, RunRecord};
use crate::codegen;
use async_trait::async_trait;
use libsql::params;
use libsql::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

"""

OBJECTS_HEAD = """use async_trait::async_trait;
use serde_json::Value;

use super::*;

"""

EVENTS_HEAD = """use libsql::params;
use libsql::Connection;

use super::*;

"""

MEMORY_HEAD = """use super::*;

"""

EVOLUTION_HEAD = """use crate::evolution::{LearnedKnowledge, Pattern, Proposal};
use libsql::params;
use libsql::Connection;

use super::*;

"""

RUNS_HEAD = """use libsql::params;
use libsql::Connection;

use super::*;

"""

RELATIONS_HEAD = """use async_trait::async_trait;
use serde_json::Value;

use super::*;

"""

USERS_HEAD = """use super::*;

"""

# 每个"段"若原代码不是独立 impl 块，需要重包 impl 头
def wrap_impl(body, head="impl LibSQLStorage {", tail_closed=False):
    """截断段重包 impl 头；tail_closed=True 表示段尾已是原 impl 闭合 `}`，不重复追加。"""
    if body.lstrip().startswith("impl "):
        return body
    stripped = body.rstrip()
    if tail_closed:
        return head + "\n" + stripped
    return head + "\n" + stripped + "\n}"

os.makedirs(OUT, exist_ok=True)

with open(f"{OUT}/mod.rs", "w") as f:
    f.write(DOC + MOD_HEAD + "\n\n".join(mod_parts) + "\n")

with open(f"{OUT}/objects.rs", "w") as f:
    f.write(OBJECTS_HEAD + wrap_impl(objects_parts[0], tail_closed=False) + "\n\n" + objects_parts[1] + "\n")

with open(f"{OUT}/events.rs", "w") as f:
    f.write(EVENTS_HEAD + wrap_impl("\n\n".join(events_parts), tail_closed=False) + "\n")

with open(f"{OUT}/memory.rs", "w") as f:
    f.write(MEMORY_HEAD + wrap_impl("\n\n".join(memory_parts), tail_closed=False) + "\n")

with open(f"{OUT}/evolution.rs", "w") as f:
    f.write(EVOLUTION_HEAD + wrap_impl("\n\n".join(evolution_parts), tail_closed=True) + "\n")

with open(f"{OUT}/runs.rs", "w") as f:
    f.write(RUNS_HEAD + "\n\n".join(runs_parts) + "\n")  # struct 段 + 完整 impl 段，无需重包

with open(f"{OUT}/relations.rs", "w") as f:
    f.write(RELATIONS_HEAD + "\n\n".join(relations_parts) + "\n")

with open(f"{OUT}/users.rs", "w") as f:
    f.write(USERS_HEAD + "\n\n".join(users_parts) + "\n")

shutil.move(SRC, SRC + ".bak")
print("storage split done")
