use libsql::params;

use super::*;

impl LibSQLStorage {
    pub async fn append_trace(&self, ev: &TraceEvent) -> Result<(), StorageError> {
        if ev.object_refs.is_empty() {
            return Err(StorageError::Constraint {
                code: "WB2006",
                message: "TraceEvent 缺 object_refs，禁止落库".into(),
            });
        }
        let obj_refs = serde_json::to_string(&ev.object_refs)?;
        let input = ev.input.as_ref().map(serde_json::to_string).transpose()?;
        let output = ev.output.as_ref().map(serde_json::to_string).transpose()?;
        self.conn
            .execute(
                "INSERT INTO wb_trace_event \
                 (id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    ev.id.clone(),
                    ev.kind.as_str(),
                    ev.event_type.clone(),
                    obj_refs,
                    ev.run_id.clone(),
                    ev.agent_id.clone(),
                    ev.reasoning.clone(),
                    input,
                    output,
                    ev.confidence,
                    ev.timestamp.clone()
                ],
            )
            .await?;
        Ok(())
    }

    /// 按条件查询 Trace 事件（认知/世界同源）。
    /// [强制] after_id 游标用于 WebSocket 断线重连补齐：返回该事件之后的全部匹配事件（按 timestamp,id 升序）。
    pub async fn list_events(&self, q: TraceQuery) -> Result<Vec<TraceEvent>, StorageError> {
        let mut sql = String::from(
            "SELECT id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp \
             FROM wb_trace_event WHERE 1=1",
        );
        let mut params: Vec<libsql::Value> = Vec::new();

        if let Some(v) = &q.run_id {
            sql.push_str(" AND run_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.agent_id {
            sql.push_str(" AND agent_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.event_type {
            sql.push_str(" AND event_type = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.object_ref {
            // object_refs 存 JSON 数组文本，按 `"id"` 子串匹配（反查经 wb_ontology_object.id）。
            // M6：输入经 LIKE 通配符转义，防止 %/_ 被当通配符放大匹配。
            sql.push_str(" AND object_refs LIKE ? ESCAPE '\\'");
            params.push(libsql::Value::Text(format!("%\"{}\"%", escape_like(v))));
        }
        if let Some(after) = &q.after_id {
            // 解析游标事件 (timestamp, id)，返回其后事件（断线重连补齐）。
            let mut marker = self
                .conn
                .query(
                    "SELECT timestamp, id FROM wb_trace_event WHERE id = ?",
                    libsql::params![after.clone()],
                )
                .await?;
            if let Some(row) = marker.next().await? {
                let ts: String = row.get(0)?;
                let id: String = row.get(1)?;
                sql.push_str(" AND (timestamp > ? OR (timestamp = ? AND id > ?))");
                params.push(libsql::Value::Text(ts.clone()));
                params.push(libsql::Value::Text(ts));
                params.push(libsql::Value::Text(id));
            }
        }
        sql.push_str(" ORDER BY timestamp ASC, id ASC");
        sql.push_str(&format!(" LIMIT {}", q.limit.clamp(1, 1000)));

        let mut rows = self
            .conn
            .query(&sql, libsql::params_from_iter(params))
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }

    /// 按 ID 取单条 Trace 事件（供 `GET /trace/{id}`，缺失返 None → WB2002）。
    pub async fn get_trace(&self, id: &str) -> Result<Option<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event WHERE id = ?",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_trace(&row)?))
        } else {
            Ok(None)
        }
    }

    /// 列出 world 事件（按时间升序），供 `oaip event list` 与 TC-008 校验。
    pub async fn list_world_events(&self) -> Result<Vec<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event WHERE kind='world' ORDER BY timestamp ASC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }
}
