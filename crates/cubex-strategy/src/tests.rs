use crate::{compile_file, compile_str, compile_str_with_base};
use cubex_core::RouteValue;
use cubex_protocol::PayloadKind;

const HELLO: &str = r#"
strategy "hello-example" {
  engine {
    max_messages = 32
  }

  plugin hello = process("../../target/debug/cubex-hello-plugin") {
    args = ["CubeX"]
    autostart = true
  }

  plugin print = process("../../target/debug/cubex-print-plugin")

  let greetings = source == hello && topic == "hello.greeting" && payload == text

  route greeting-to-print = greetings -> [print]
}
"#;

#[test]
fn compiles_basic_strategy() {
    let config = compile_str(HELLO).unwrap();

    assert_eq!(config.engine.name, "hello-example");
    assert_eq!(config.engine.max_messages, 32);
    assert_eq!(config.plugins.len(), 2);
    assert_eq!(config.routes.len(), 1);
    assert_eq!(config.routes[0].source.as_deref(), Some("hello"));
    assert_eq!(config.routes[0].topic.as_deref(), Some("hello.greeting"));
    assert_eq!(config.routes[0].payload, Some(PayloadKind::Text));
    assert_eq!(config.routes[0].to, vec!["print"]);
}

#[test]
fn record_predicates_imply_record_payload() {
    let config = compile_str(
        r#"
strategy "records" {
  plugin source = process("source")
  plugin print = process("print")

  let alice = topic == "record.put" && record.user == "alice" && record.priority == 7

  route alice-to-print = alice -> [print]
}
"#,
    )
    .unwrap();

    let route = &config.routes[0];
    assert_eq!(route.payload, Some(PayloadKind::Record));
    assert_eq!(
        route.record.get("user"),
        Some(&RouteValue::String("alice".into()))
    );
    assert_eq!(route.record.get("priority"), Some(&RouteValue::I64(7)));
}

#[test]
fn rejects_unknown_route_targets() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  route bad-route = source == source -> [missing]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unknown plugin `missing`"));
}

#[test]
fn reports_parse_errors_with_source_locations() {
    let error = compile_str(
        "strategy \"bad\" {\n  plugin print = process(\"print\")\n  route bad-route = source = print -> [print]\n}\n",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("strategy parse error at <input>:3:21"));
    assert!(error.contains("  route bad-route = source = print -> [print]"));
    assert!(error.contains("^"));
    assert!(error.contains("expected"));
}

#[test]
fn reports_compile_errors_with_source_locations() {
    let temp = std::env::temp_dir().join(format!(
        "cubex-strategy-diagnostic-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir(&temp).unwrap();
    let source = temp.join("cubex.cx");
    std::fs::write(
        &source,
        "strategy \"bad\" {\n  plugin print = process(\"print\")\n  route bad-route = source == missing -> [print]\n}\n",
    )
    .unwrap();

    let error = compile_file(&source).unwrap_err().to_string();

    assert!(error.contains(&format!(
        "strategy compile error at {}:3:31",
        source.display()
    )));
    assert!(error.contains("  route bad-route = source == missing -> [print]"));
    assert!(error.contains("^^^^^^^"));
    assert!(error.contains("unknown plugin or engine `missing`"));

    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn rejects_process_capabilities() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source") {
    capability file_read("input.txt")
  }
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("process plugin `source` cannot declare capabilities"));
}

#[test]
fn rejects_conflicting_predicates() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")
  route bad-route = payload == text && record.user == "alice" -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("record predicates but payload is `text`"));
}

#[test]
fn detects_predicate_cycles() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")
  let a = b
  let b = a
  route bad-route = a -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("predicate reference cycle"));
}

#[test]
fn resolves_paths_relative_to_strategy_file() {
    let temp = std::env::temp_dir().join(format!("cubex-strategy-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir(&temp).unwrap();
    let config = compile_str_with_base(
        r#"
strategy "paths" {
  store {
    path = "events.bin"
  }
  plugin source = process("bin/source") {
    working_dir = "."
  }
}
"#,
        &temp,
    )
    .unwrap();

    assert_eq!(config.store.path, Some(temp.join("events.bin")));
    assert_eq!(config.plugins[0].command, temp.join("bin/source"));
    assert_eq!(config.plugins[0].working_dir, Some(temp.clone()));

    let _ = std::fs::remove_dir_all(temp);
}
