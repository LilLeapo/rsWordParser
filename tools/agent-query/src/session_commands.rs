//! AGENT-10：供 CLI/MCP 共用的报告、只读检查及原生调试事务。
use super::*;
use crate::{
    audit::{Audit, canonical, parse as strict_json},
    report::Report,
};
impl Sessions {
    pub fn report(&self, id: &str, report_id: &str) -> Result<Report> {
        Ok(self.get(id)?.reports.get(report_id)?.clone())
    }
    pub fn projection(&self, id: &str) -> Result<Projection> {
        self.get(id)?
            .native
            .inspect(&self.get(id)?.native_id, |s, _| {
                text::project(
                    s.package(),
                    s.document(),
                    Scope::All,
                    &self.snapshot(id).unwrap().to_string(),
                )
            })?
            .map_err(QueryError::from)
    }
    pub fn check(&self, id: &str, original: &[u8]) -> Result<Vec<Value>> {
        let s = self.get(id)?;
        let diagnostics: Value = serde_json::from_str(&s.native.clone().diagnostics(&s.native_id)?)
            .map_err(|e| error("AGENT_INTERNAL", e.to_string()))?;
        let saved = self.save(id, None);
        let mut rows = vec![
            json!({"kind":"evidence","wordOpen":"notVerified","scope":"当前输入的无编辑保存及内核可检查项；不证明任意编辑的保真"}),
        ];
        match saved {
            Ok(bytes)=>rows.push(json!({"kind":"invariant","name":"noEditSaveIdentity","passed":bytes==original,"saveValidation":"passed"})),
            Err(e)=>rows.push(json!({"kind":"invariant","name":"saveValidation","passed":false,"error":e})),
        }
        rows.push(json!({"kind":"diagnostics","value":diagnostics}));
        let package = s.native.inspect(&s.native_id, |s, _| {
            use rsword::bind::native::{ProjCx, ToJson};
            s.package().diagnostics().to_json(&ProjCx { pkg: s.package(), display: false })
        })?;
        rows.push(json!({"kind":"packageDiagnostics","value":package}));
        let checks = s.native.inspect(&s.native_id, |s, _| {
            let mut parts = 0;
            let mut failures = vec![];
            for part in s.package().parts().iter().filter(|p| !p.deleted) {
                if let Some(dom) = part.dom() {
                    parts += 1;
                    if let Err(why) = dom.check_dirty_invariants() {
                        failures.push(json!({"part":part.uri.to_string(),"message":why}));
                    }
                }
            }
            let violations = s.diagnostics().iter().chain(s.package().diagnostics())
                .filter(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation).count();
            vec![json!({"kind":"invariant","name":"dirtyPropagation","partsChecked":parts,"passed":failures.is_empty(),"failures":failures}),
                 json!({"kind":"invariant","name":"engineInvariantDiagnostics","passed":violations==0,"violations":violations})]
        })?;
        rows.extend(checks);
        Ok(rows)
    }
    /// 与 Agent 编译分开，但仍先形成完整审计再交换会话状态。
    pub fn edit_native_report(&mut self, id: &str, version: u64, input: &str) -> Result<Value> {
        self.expected(id, version)?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            operations: Vec<rsword::bind::native::EditOpJson>,
            #[serde(default)]
            context: rsword::EditContext,
        }
        let raw = strict_json(input)?;
        let request: Request = serde_json::from_value(raw.clone())
            .map_err(|e| error("BIND_BAD_ARGUMENT", e.to_string()))?;
        let state = self.get(id)?;
        let mut native = state.native.clone();
        let context = serde_json::to_string(&request.context).unwrap();
        let mut rows = vec![
            json!({"kind":"request","nativeDebug":true,"original":raw,"beforeVersion":version,"afterVersion":version+1}),
        ];
        for (index, op) in request.operations.iter().enumerate() {
            let result = native
                .apply(&state.native_id, &serde_json::to_string(op).unwrap(), Some(&context))
                .map_err(|e| {
                    let mut e = QueryError::from(e);
                    e.details["operationIndex"] = json!(index);
                    e
                })?;
            rows.push(json!({"kind":"mutation","index":index,"value":serde_json::from_str::<Value>(&result).unwrap()}));
        }
        let audit = Audit::capture(&request.operations);
        for (index, op) in audit.operations.iter().enumerate() {
            rows.push(json!({"kind":"editOp","index":index,"value":op}));
        }
        rows.push(json!({"kind":"attachments","bindings":audit.attachments,"executionHash":audit.execution_hash}));
        rows.push(json!({"kind":"diagnostics","value":serde_json::from_str::<Value>(&native.diagnostics(&state.native_id)?).unwrap()}));
        let report_id =
            format!("native-{version}-{}", crate::cursor::hash(&canonical(&request.operations)));
        let mut snapshot = self.snapshot(id)?;
        snapshot["reportId"] = json!(report_id);
        let mut report = Report {
            id: report_id.clone(),
            snapshot: snapshot.to_string(),
            request: raw,
            before_version: version,
            rows,
            audit,
            bytes: 0,
        };
        loop {
            let n = serde_json::to_vec(&report).unwrap().len();
            if n == report.bytes {
                break;
            }
            report.bytes = n;
        }
        if report.bytes > 16 * 1024 * 1024 {
            return Err(error("AGENT_REPORT_TOO_LARGE", "原生调试报告超容量，未提交"));
        }
        let count = request.operations.len();
        let state = self.sessions.get_mut(id).unwrap();
        state.native = native;
        state.version += 1;
        state.reports.insert(report, false);
        Ok(
            json!({"beforeVersion":version,"afterVersion":version+1,"reportId":report_id,"counts":{"nativeOperations":count},"nativeDebug":true}),
        )
    }
}
