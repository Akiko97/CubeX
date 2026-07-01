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

  let alice = source == source && topic == "record.put" && record.user == "alice" && record.priority == 7

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
fn compiles_parameterized_predicates() {
    let config = compile_str(
        r#"
strategy "parameterized" {
  plugin hello = process("hello")
  plugin print = process("print")

  fn from_topic(src, t, kind) =
    source == src && topic == t && payload == kind

  route greeting-to-print = from_topic(hello, "hello.greeting", text) -> [print]
}
"#,
    )
    .unwrap();

    let route = &config.routes[0];
    assert_eq!(route.source.as_deref(), Some("hello"));
    assert_eq!(route.topic.as_deref(), Some("hello.greeting"));
    assert_eq!(route.payload, Some(PayloadKind::Text));
}

#[test]
fn compiles_nested_parameterized_predicates() {
    let config = compile_str(
        r#"
strategy "nested" {
  plugin source = process("source")
  plugin print = process("print")

  fn from_topic(src, t, kind) =
    source == src && topic == t && payload == kind

  fn text_from(src, t) = from_topic(src, t, text)

  route source-to-print = text_from(source, "source.ready") -> [print]
}
"#,
    )
    .unwrap();

    let route = &config.routes[0];
    assert_eq!(route.source.as_deref(), Some("source"));
    assert_eq!(route.topic.as_deref(), Some("source.ready"));
    assert_eq!(route.payload, Some(PayloadKind::Text));
}

#[test]
fn rejects_parameterized_predicate_arity_mismatch() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")

  fn from_topic(src, t) = source == src && topic == t

  route bad-route = from_topic(source) -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("expects 2 arguments but got 1"));
}

#[test]
fn rejects_unknown_parameterized_predicate() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")

  route bad-route = missing(source) -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unknown predicate function `missing`"));
}

#[test]
fn rejects_duplicate_parameter_names() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")

  fn bad(src, src) = source == src

  route bad-route = bad(source, source) -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("declares parameter `src` more than once"));
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

    assert!(error.contains("route `bad-route` is unreachable"));
    assert!(error.contains("record predicates require payload `record`, but payload is `text`"));
}

#[test]
fn rejects_unreachable_routes_with_conflicting_payload_predicates() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin print = process("print")

  route bad-route = payload == text && payload == record -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("route `bad-route` is unreachable"));
    assert!(error.contains("conflicting `payload` predicates: `text` vs `record`"));
}

#[test]
fn rejects_unreachable_routes_with_conflicting_source_predicates() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin other = process("other")
  plugin print = process("print")

  route bad-route = source == source && source == other -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("route `bad-route` is unreachable"));
    assert!(error.contains("conflicting `source` predicates: `source` vs `other`"));
}

#[test]
fn rejects_unreachable_routes_with_conflicting_record_field_predicates() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin print = process("print")

  route bad-route =
    record.user == "alice" && record.user == "bob" -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("route `bad-route` is unreachable"));
    assert!(error.contains("conflicting `record.user` predicates: `\"alice\"` vs `\"bob\"`"));
}

#[test]
fn rejects_unreachable_routes_after_predicate_function_expansion() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin print = process("print")

  fn has_payload(kind) = payload == kind

  route bad-route = has_payload(text) && payload == record -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("route `bad-route` is unreachable"));
    assert!(error.contains("conflicting `payload` predicates: `text` vs `record`"));
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

  route source-loop = source == source -> [source]
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

#[test]
fn compiles_file_level_includes() {
    let temp = test_temp_dir("include");
    let common = temp.join("common");
    std::fs::create_dir_all(&common).unwrap();

    let main = temp.join("cubex.cx");
    let plugins = common.join("plugins.cx");
    let predicates = common.join("predicates.cx");

    std::fs::write(
        &plugins,
        r#"
plugin hello = process("bin/hello") {
  working_dir = "."
  autostart = true
}

plugin print = process("bin/print")
"#,
    )
    .unwrap();
    std::fs::write(
        &predicates,
        r#"
fn from_topic(src, t, kind) =
  source == src && topic == t && payload == kind
"#,
    )
    .unwrap();
    std::fs::write(
        &main,
        r#"
include "common/plugins.cx"
include "common/predicates.cx"

strategy "included" {
  route greeting-to-print =
    from_topic(hello, "hello.greeting", text) -> [print]
}
"#,
    )
    .unwrap();

    let config = compile_file(&main).unwrap();

    assert_eq!(config.engine.name, "included");
    assert_eq!(config.plugins.len(), 2);
    assert_eq!(config.plugins[0].name, "hello");
    assert_eq!(config.plugins[0].command, common.join("bin/hello"));
    assert_eq!(config.plugins[0].working_dir, Some(common.clone()));
    assert!(config.plugins[0].autostart);
    assert_eq!(config.routes.len(), 1);
    assert_eq!(config.routes[0].source.as_deref(), Some("hello"));
    assert_eq!(config.routes[0].topic.as_deref(), Some("hello.greeting"));
    assert_eq!(config.routes[0].payload, Some(PayloadKind::Text));
    assert_eq!(config.routes[0].to, vec!["print"]);

    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn rejects_unused_plugins() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")
  plugin debug-sink = process("debug")

  route source-to-print = source == source -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unused plugin `debug-sink`"));
}

#[test]
fn permits_autostart_plugins_without_explicit_route_references() {
    let config = compile_str(
        r#"
strategy "autostart" {
  plugin server = process("server") {
    autostart = true
  }
  plugin source = process("source") {
    autostart = true
  }
  plugin print = process("print")

  route source-to-print = source == source && payload == text -> [print]
}
"#,
    )
    .unwrap();

    assert_eq!(config.plugins.len(), 3);
    assert_eq!(config.routes[0].source.as_deref(), Some("source"));
}

#[test]
fn rejects_unused_predicate_bindings() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")

  let used = source == source
  let unused-records = topic == "record.put" && payload == record

  route source-to-print = used -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unused predicate binding `unused-records`"));
}

#[test]
fn rejects_unused_predicate_functions() {
    let error = compile_str(
        r#"
strategy "bad" {
  plugin source = process("source")
  plugin print = process("print")

  fn used(src) = source == src
  fn unused(src) = source == src

  route source-to-print = used(source) -> [print]
}
"#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unused predicate function `unused`"));
}

#[test]
fn reports_compile_errors_in_included_files() {
    let temp = test_temp_dir("include-diagnostic");
    std::fs::create_dir_all(&temp).unwrap();

    let main = temp.join("cubex.cx");
    let predicates = temp.join("predicates.cx");

    std::fs::write(
        &predicates,
        r#"
let bad_source = source == missing
"#,
    )
    .unwrap();
    std::fs::write(
        &main,
        r#"
include "predicates.cx"

strategy "bad-include" {
  plugin print = process("print")
  route bad-route = bad_source -> [print]
}
"#,
    )
    .unwrap();

    let error = compile_file(&main).unwrap_err().to_string();

    assert!(error.contains(&format!(
        "strategy compile error at {}:2:28",
        predicates.display()
    )));
    assert!(error.contains("let bad_source = source == missing"));
    assert!(error.contains("unknown plugin or engine `missing`"));

    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn reports_missing_includes_at_include_site() {
    let temp = test_temp_dir("missing-include");
    std::fs::create_dir_all(&temp).unwrap();

    let main = temp.join("cubex.cx");
    std::fs::write(
        &main,
        r#"
include "missing.cx"

strategy "bad-include" {}
"#,
    )
    .unwrap();

    let error = compile_file(&main).unwrap_err().to_string();

    assert!(error.contains(&format!("strategy compile error at {}:2:9", main.display())));
    assert!(error.contains("include \"missing.cx\""));
    assert!(error.contains("failed to read include"));

    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn reports_include_cycles_at_include_site() {
    let temp = test_temp_dir("include-cycle");
    std::fs::create_dir_all(&temp).unwrap();

    let main = temp.join("cubex.cx");
    let a = temp.join("a.cx");
    let b = temp.join("b.cx");

    std::fs::write(
        &main,
        r#"
include "a.cx"

strategy "cycle" {}
"#,
    )
    .unwrap();
    std::fs::write(
        &a,
        r#"
include "b.cx"
"#,
    )
    .unwrap();
    std::fs::write(
        &b,
        r#"
include "a.cx"
"#,
    )
    .unwrap();

    let error = compile_file(&main).unwrap_err().to_string();

    assert!(error.contains(&format!("strategy compile error at {}:2:9", b.display())));
    assert!(error.contains("include \"a.cx\""));
    assert!(error.contains("include cycle detected"));

    let _ = std::fs::remove_dir_all(temp);
}

fn test_temp_dir(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "cubex-strategy-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    path
}
