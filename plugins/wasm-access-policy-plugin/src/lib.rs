use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct WasmAccessPolicyPlugin {
    rules: BTreeMap<String, BTreeSet<String>>,
}

impl Plugin for WasmAccessPolicyPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        match request.message.payload {
            Payload::Control(Control::Start { args }) => {
                self.rules = parse_rules(&args)?;
                Ok(PluginResponse {
                    messages: Vec::new(),
                    logs: vec![format!("loaded {} access rule(s)", self.rules.len())],
                    error: None,
                })
            }
            Payload::Control(Control::Stop) => Ok(PluginResponse::default()),
            payload => {
                let topic = request.message.topic;
                let user = payload_user(&payload)?.unwrap_or_else(|| "anonymous".into());
                let allowed = self
                    .rules
                    .get(&user)
                    .is_some_and(|topics| topics.contains(&topic) || topics.contains("*"));
                let decision = if allowed { "allowed" } else { "denied" };
                Ok(PluginResponse {
                    messages: vec![Message::new(
                        request.plugin,
                        format!("access.{decision}"),
                        checked_payload(payload, decision, &topic),
                    )],
                    logs: Vec::new(),
                    error: None,
                })
            }
        }
    }
}

fn payload_user(payload: &Payload) -> anyhow::Result<Option<String>> {
    let Payload::Record(fields) = payload else {
        return Ok(None);
    };
    let Some(value) = fields.get("user") else {
        return Ok(None);
    };
    let Value::String(user) = value else {
        anyhow::bail!("payload user must be a string");
    };
    validate_text(user, "payload user")?;
    Ok(Some(user.clone()))
}

fn checked_payload(payload: Payload, decision: &str, topic: &str) -> Payload {
    match payload {
        Payload::Record(mut fields) => {
            fields.insert("decision".into(), Value::String(decision.into()));
            fields.insert("checked_topic".into(), Value::String(topic.into()));
            Payload::Record(fields)
        }
        other => other,
    }
}

fn parse_rules(args: &[String]) -> anyhow::Result<BTreeMap<String, BTreeSet<String>>> {
    let mut rules = BTreeMap::new();
    for rule in args {
        let (user, topics) = rule
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("access rule must be `user=topic[,topic]`: {rule}"))?;
        validate_text(user, "access rule user")?;
        let mut parsed = BTreeSet::new();
        for topic in topics.split(',') {
            validate_text(topic, "access rule topic")?;
            if !parsed.insert(topic.to_string()) {
                anyhow::bail!("duplicate access topic `{topic}` for user `{user}`");
            }
        }
        if rules.insert(user.to_string(), parsed).is_some() {
            anyhow::bail!("duplicate access rule for user `{user}`");
        }
    }
    Ok(rules)
}

fn validate_text(value: &str, label: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{label} must not be empty");
    }
    if value.trim() != value {
        anyhow::bail!("{label} must not be padded");
    }
    Ok(())
}

cubex_wasm_plugin_sdk::export_plugin!(WasmAccessPolicyPlugin::default());
