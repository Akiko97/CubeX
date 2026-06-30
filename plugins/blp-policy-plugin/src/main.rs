use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct BlpPolicyPlugin;

impl Plugin for BlpPolicyPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        match request.message.payload {
            Payload::Control(Control::Start { .. }) | Payload::Control(Control::Stop) => {
                Ok(PluginResponse::default())
            }
            Payload::Record(mut fields) => {
                let decision = evaluate_request(&fields)?;
                let decision_text = decision.text();
                fields.insert("decision".into(), Value::String(decision_text.into()));
                fields.insert("reason".into(), Value::String(decision.reason));
                fields.insert("policy".into(), Value::String("bell-lapadula".into()));

                Ok(PluginResponse {
                    messages: vec![Message::new(
                        request.plugin,
                        format!("blp.{decision_text}"),
                        Payload::Record(fields),
                    )],
                    logs: Vec::new(),
                    error: None,
                })
            }
            _ => Ok(PluginResponse::default()),
        }
    }
}

#[derive(Debug)]
struct Decision {
    allowed: bool,
    reason: String,
}

impl Decision {
    fn allowed(reason: impl Into<String>) -> Self {
        Self {
            allowed: true,
            reason: reason.into(),
        }
    }

    fn denied(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: reason.into(),
        }
    }

    fn text(&self) -> &'static str {
        if self.allowed { "allowed" } else { "denied" }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Label {
    level: u64,
    compartments: BTreeSet<String>,
}

fn evaluate_request(fields: &BTreeMap<String, Value>) -> anyhow::Result<Decision> {
    let action = required_string(fields, "action")?;
    let subject = Label {
        level: required_level(fields, "subject_level")?,
        compartments: required_compartments(fields, "subject_compartments")?,
    };
    let object = Label {
        level: required_level(fields, "object_level")?,
        compartments: required_compartments(fields, "object_compartments")?,
    };

    match action.as_str() {
        "read" => {
            if subject_dominates_object(&subject, &object) {
                Ok(Decision::allowed("read-allowed"))
            } else if subject.level < object.level {
                Ok(Decision::denied("read-up"))
            } else {
                Ok(Decision::denied("missing-compartment"))
            }
        }
        "write" => {
            if subject_dominates_object(&object, &subject) {
                Ok(Decision::allowed("write-allowed"))
            } else if object.level < subject.level {
                Ok(Decision::denied("write-down"))
            } else {
                Ok(Decision::denied("object-missing-compartment"))
            }
        }
        other => anyhow::bail!("blp action must be `read` or `write`, got `{other}`"),
    }
}

fn subject_dominates_object(subject: &Label, object: &Label) -> bool {
    subject.level >= object.level && object.compartments.is_subset(&subject.compartments)
}

fn required_string(fields: &BTreeMap<String, Value>, key: &str) -> anyhow::Result<String> {
    let value = fields
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("blp record missing `{key}`"))?;
    let Value::String(text) = value else {
        anyhow::bail!("blp record `{key}` must be a string");
    };
    validate_text(key, text)?;
    Ok(text.clone())
}

fn required_level(fields: &BTreeMap<String, Value>, key: &str) -> anyhow::Result<u64> {
    let value = fields
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("blp record missing `{key}`"))?;
    match value {
        Value::I64(level) if *level >= 0 => Ok(*level as u64),
        Value::U64(level) => Ok(*level),
        Value::I64(_) => anyhow::bail!("blp record `{key}` must not be negative"),
        _ => anyhow::bail!("blp record `{key}` must be an integer"),
    }
}

fn required_compartments(
    fields: &BTreeMap<String, Value>,
    key: &str,
) -> anyhow::Result<BTreeSet<String>> {
    let text = required_string(fields, key)?;
    if text == "none" {
        return Ok(BTreeSet::new());
    }

    let mut compartments = BTreeSet::new();
    for compartment in text.split(',') {
        validate_text(key, compartment)?;
        if !compartments.insert(compartment.to_string()) {
            anyhow::bail!("blp record `{key}` has duplicate compartment `{compartment}`");
        }
    }
    Ok(compartments)
}

fn validate_text(key: &str, text: &str) -> anyhow::Result<()> {
    if text.trim().is_empty() {
        anyhow::bail!("blp record `{key}` must not be empty");
    }
    if text.trim() != text {
        anyhow::bail!("blp record `{key}` must not be padded");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(BlpPolicyPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_read_when_subject_dominates_object() {
        let response = run_policy(record([
            ("action", Value::String("read".into())),
            ("subject_level", Value::I64(3)),
            ("subject_compartments", Value::String("ops,finance".into())),
            ("object_level", Value::I64(2)),
            ("object_compartments", Value::String("ops".into())),
        ]));

        assert_eq!(response.messages[0].topic, "blp.allowed");
        assert_field(&response, "reason", "read-allowed");
    }

    #[test]
    fn denies_read_up() {
        let response = run_policy(record([
            ("action", Value::String("read".into())),
            ("subject_level", Value::I64(1)),
            ("subject_compartments", Value::String("ops".into())),
            ("object_level", Value::I64(2)),
            ("object_compartments", Value::String("ops".into())),
        ]));

        assert_eq!(response.messages[0].topic, "blp.denied");
        assert_field(&response, "reason", "read-up");
    }

    #[test]
    fn denies_read_without_required_compartment() {
        let response = run_policy(record([
            ("action", Value::String("read".into())),
            ("subject_level", Value::I64(3)),
            ("subject_compartments", Value::String("ops".into())),
            ("object_level", Value::I64(2)),
            ("object_compartments", Value::String("ops,finance".into())),
        ]));

        assert_eq!(response.messages[0].topic, "blp.denied");
        assert_field(&response, "reason", "missing-compartment");
    }

    #[test]
    fn allows_write_up() {
        let response = run_policy(record([
            ("action", Value::String("write".into())),
            ("subject_level", Value::I64(1)),
            ("subject_compartments", Value::String("ops".into())),
            ("object_level", Value::I64(2)),
            ("object_compartments", Value::String("ops,archive".into())),
        ]));

        assert_eq!(response.messages[0].topic, "blp.allowed");
        assert_field(&response, "reason", "write-allowed");
    }

    #[test]
    fn denies_write_down() {
        let response = run_policy(record([
            ("action", Value::String("write".into())),
            ("subject_level", Value::I64(3)),
            ("subject_compartments", Value::String("ops".into())),
            ("object_level", Value::I64(1)),
            ("object_compartments", Value::String("ops".into())),
        ]));

        assert_eq!(response.messages[0].topic, "blp.denied");
        assert_field(&response, "reason", "write-down");
    }

    #[test]
    fn rejects_invalid_action() {
        let error = evaluate_request(&record([
            ("action", Value::String("delete".into())),
            ("subject_level", Value::I64(3)),
            ("subject_compartments", Value::String("ops".into())),
            ("object_level", Value::I64(1)),
            ("object_compartments", Value::String("ops".into())),
        ]))
        .unwrap_err()
        .to_string();

        assert_eq!(error, "blp action must be `read` or `write`, got `delete`");
    }

    #[test]
    fn rejects_malformed_labels() {
        let error = evaluate_request(&record([
            ("action", Value::String("read".into())),
            ("subject_level", Value::I64(-1)),
            ("subject_compartments", Value::String("ops".into())),
            ("object_level", Value::I64(1)),
            ("object_compartments", Value::String("ops".into())),
        ]))
        .unwrap_err()
        .to_string();

        assert_eq!(error, "blp record `subject_level` must not be negative");

        let error = evaluate_request(&record([
            ("action", Value::String("read".into())),
            ("subject_level", Value::I64(1)),
            ("subject_compartments", Value::String("ops, ops".into())),
            ("object_level", Value::I64(1)),
            ("object_compartments", Value::String("ops".into())),
        ]))
        .unwrap_err()
        .to_string();

        assert_eq!(
            error,
            "blp record `subject_compartments` must not be padded"
        );
    }

    #[test]
    fn ignores_non_record_messages() {
        let response = BlpPolicyPlugin
            .handle(PluginRequest {
                plugin: "blp".into(),
                message: Message::new("source", "text", Payload::Text("ignored".into())),
            })
            .unwrap();

        assert!(response.messages.is_empty());
    }

    fn run_policy(fields: BTreeMap<String, Value>) -> PluginResponse {
        BlpPolicyPlugin
            .handle(PluginRequest {
                plugin: "blp".into(),
                message: Message::new("source", "record.put", Payload::Record(fields)),
            })
            .unwrap()
    }

    fn record(fields: impl IntoIterator<Item = (&'static str, Value)>) -> BTreeMap<String, Value> {
        fields
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect()
    }

    fn assert_field(response: &PluginResponse, key: &str, expected: &str) {
        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(fields[key], Value::String(expected.into()));
    }
}
