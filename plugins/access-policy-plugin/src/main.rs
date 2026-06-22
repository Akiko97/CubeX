use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct AccessPolicyPlugin {
    rules: BTreeMap<String, BTreeSet<String>>,
}

impl Plugin for AccessPolicyPlugin {
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
                let payload = checked_payload(payload, decision, &topic);
                let message = Message::new(request.plugin, format!("access.{decision}"), payload);
                Ok(PluginResponse {
                    messages: vec![message],
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
    if user.trim().is_empty() {
        anyhow::bail!("payload user must not be empty");
    }
    if user.trim() != user {
        anyhow::bail!("payload user must not be padded");
    }
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
        let (user, topic_text) = rule
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("access rule must be `user=topic[,topic]`: {rule}"))?;
        if user.trim().is_empty() {
            anyhow::bail!("access rule user must not be empty: {rule}");
        }
        if user.trim() != user {
            anyhow::bail!("access rule user must not be padded: {rule}");
        }
        let mut topics = BTreeSet::new();
        for topic in topic_text.split(',') {
            if topic.trim().is_empty() {
                anyhow::bail!("access rule topic must not be empty: {rule}");
            }
            if topic.trim() != topic {
                anyhow::bail!("access rule topic must not be padded: {rule}");
            }
            if !topics.insert(topic.to_string()) {
                anyhow::bail!("duplicate access topic `{topic}` for user `{user}`");
            }
        }
        if topics.is_empty() {
            anyhow::bail!("access rule needs at least one topic: {rule}");
        }
        if rules.insert(user.to_string(), topics).is_some() {
            anyhow::bail!("duplicate access rule for user `{user}`");
        }
    }
    Ok(rules)
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(AccessPolicyPlugin::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_topic_rules() {
        let rules = parse_rules(&["alice=record.put,*".into(), "bob=timer.tick".into()]).unwrap();

        assert!(rules["alice"].contains("record.put"));
        assert!(rules["alice"].contains("*"));
        assert!(rules["bob"].contains("timer.tick"));
    }

    #[test]
    fn policy_reads_user_from_record_payload() {
        let mut plugin = AccessPolicyPlugin {
            rules: parse_rules(&["alice=record.put".into()]).unwrap(),
        };
        let payload = Payload::Record(BTreeMap::from([(
            "user".into(),
            Value::String("alice".into()),
        )]));

        let response = plugin
            .handle(PluginRequest {
                plugin: "policy".into(),
                message: Message::new("source", "record.put", payload),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "access.allowed");
    }

    #[test]
    fn denied_records_include_decision_fields() {
        let mut plugin = AccessPolicyPlugin {
            rules: parse_rules(&["alice=record.put".into()]).unwrap(),
        };
        let payload = Payload::Record(BTreeMap::from([(
            "user".into(),
            Value::String("bob".into()),
        )]));

        let response = plugin
            .handle(PluginRequest {
                plugin: "policy".into(),
                message: Message::new("source", "record.put", payload),
            })
            .unwrap();

        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(response.messages[0].topic, "access.denied");
        assert_eq!(fields["decision"], Value::String("denied".into()));
        assert_eq!(fields["checked_topic"], Value::String("record.put".into()));
    }

    #[test]
    fn rejects_invalid_payload_users() {
        let mut plugin = AccessPolicyPlugin::default();
        for (user, message) in [
            ("", "payload user must not be empty"),
            (" alice", "payload user must not be padded"),
        ] {
            let payload = Payload::Record(BTreeMap::from([(
                "user".into(),
                Value::String(user.into()),
            )]));

            let error = plugin
                .handle(PluginRequest {
                    plugin: "policy".into(),
                    message: Message::new("source", "record.put", payload),
                })
                .unwrap_err()
                .to_string();

            assert_eq!(error, message);
        }

        let payload = Payload::Record(BTreeMap::from([("user".into(), Value::U64(7))]));
        let error = plugin
            .handle(PluginRequest {
                plugin: "policy".into(),
                message: Message::new("source", "record.put", payload),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "payload user must be a string");
    }

    #[test]
    fn ignores_stop_control() {
        let mut plugin = AccessPolicyPlugin::default();

        let response = plugin
            .handle(PluginRequest {
                plugin: "policy".into(),
                message: Message::new("engine", "system.stop", Payload::Control(Control::Stop)),
            })
            .unwrap();

        assert!(response.messages.is_empty());
        assert!(response.logs.is_empty());
    }

    #[test]
    fn rejects_malformed_rules() {
        assert!(parse_rules(&["alice".into()]).is_err());
        assert!(parse_rules(&["=record.put".into()]).is_err());
        assert!(parse_rules(&[" alice=record.put".into()]).is_err());
        assert!(parse_rules(&["alice=".into()]).is_err());
        assert!(parse_rules(&["alice= record.put".into()]).is_err());
        assert!(parse_rules(&["alice=record.put,record.put".into()]).is_err());
        assert!(parse_rules(&["alice=record.put".into(), "alice=timer.tick".into()]).is_err());
    }
}
