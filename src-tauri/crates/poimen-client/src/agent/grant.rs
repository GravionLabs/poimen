//! The requests of the scoped write grant (ADR 0018, #1211).

use std::time::Duration;

use poimen_protocol::{
    AgentGrantAnswer, AgentGrantCheck, AgentGrantRequest, AgentGrantRevoke, AgentGrants,
    GrantResult, GrantVerdict,
};
use serde_json::Value;

use crate::{Client, ClientError, REQUEST_TIMEOUT, to_value};

fn parse<T: serde::de::DeserializeOwned>(method: &str, value: Value) -> Result<T, ClientError> {
    serde_json::from_value(value).map_err(|e| ClientError::Protocol(format!("{method}: {e}")))
}

impl Client {
    /// `agent.grant_request`: asks the user, through a connected app, for a grant. Waits for the
    /// answer, at most `wait` plus a margin; a timeout answers `granted: false`.
    pub fn request_agent_grant(
        &self,
        params: &AgentGrantRequest,
        wait: Duration,
    ) -> Result<GrantResult, ClientError> {
        let mut params = params.clone();
        params.wait_ms = Some(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX));
        let value = self.request_within(
            "agent.grant_request",
            to_value(&params),
            wait + REQUEST_TIMEOUT,
        )?;
        parse("agent.grant_request", value)
    }

    /// `agent.grant_answer`: the app's answer to a grant request.
    pub fn answer_agent_grant(&self, answer: &AgentGrantAnswer) -> Result<(), ClientError> {
        self.request("agent.grant_answer", to_value(answer))
            .map(drop)
    }

    /// `agent.grant_revoke`: whether a grant was revoked.
    pub fn revoke_agent_grant(&self, revoke: &AgentGrantRevoke) -> Result<bool, ClientError> {
        let value = self.request("agent.grant_revoke", to_value(revoke))?;
        Ok(value.get("revoked").and_then(Value::as_bool) == Some(true))
    }

    /// `agent.grants`: the active grants.
    pub fn agent_grants(&self) -> Result<AgentGrants, ClientError> {
        let value = self.request("agent.grants", Value::Null)?;
        parse("agent.grants", value)
    }

    /// `agent.grant_check`: whether a grant covers the write about to be made.
    pub fn check_agent_grant(&self, check: &AgentGrantCheck) -> Result<GrantVerdict, ClientError> {
        let value = self.request("agent.grant_check", to_value(check))?;
        parse("agent.grant_check", value)
    }
}
