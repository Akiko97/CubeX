use crate::{RouteConfig, RouteValue};
use cubex_protocol::{Message, Payload, Value};
use std::collections::BTreeMap;

impl RouteConfig {
    pub(crate) fn matches(&self, message: &Message) -> bool {
        self.source.as_ref().is_none_or(|s| s == &message.source)
            && self.topic.as_ref().is_none_or(|t| t == &message.topic)
            && self.payload.is_none_or(|p| p == message.payload_kind())
            && record_matches(&self.record, &message.payload)
    }
}

fn record_matches(expected: &BTreeMap<String, RouteValue>, payload: &Payload) -> bool {
    if expected.is_empty() {
        return true;
    }
    let Payload::Record(record) = payload else {
        return false;
    };
    expected.iter().all(|(key, expected)| {
        record
            .get(key)
            .is_some_and(|actual| expected.matches(actual))
    })
}

impl RouteValue {
    fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Bool(expected), Value::Bool(actual)) => expected == actual,
            (Self::I64(expected), Value::I64(actual)) => expected == actual,
            (Self::I64(expected), Value::U64(actual)) => {
                *expected >= 0 && u64::try_from(*expected).is_ok_and(|expected| expected == *actual)
            }
            (Self::String(expected), Value::String(actual)) => expected == actual,
            _ => false,
        }
    }
}
