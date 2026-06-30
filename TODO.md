# TODO

## Strategy Language Evolution

- [x] Add source-position diagnostics for parse and compile errors.

  Today, many strategy errors are readable but not anchored to the exact source
  location that caused them. The compiler should report the file, line, column,
  source snippet, and caret indicator.

  Example target output:

  ```text
  strategy compile error at examples/foo/cubex.cx:18:12
    route foo = source == missing -> [print]
                       ^^^^^^^
    unknown plugin or engine `missing`
  ```

- [x] Stabilize CLI error output for `cubex compile` and `cubex check`.

  Once diagnostics include source locations, the CLI should print them in a
  stable format so CI logs are useful and tests can assert exact failure modes.
  Human-readable output should remain the default, with room for a future
  machine-readable mode.

  Example command:

  ```sh
  cargo run -p cubex-cli -- check -c examples/bad/cubex.cx
  ```

- [x] Add parameterized predicates.

  Current `let` bindings define only zero-argument predicates. Parameterized
  predicates would reduce repeated route conditions and make shared policy
  logic easier to read.

  Example target syntax:

  ```cx
  fn from_topic(src, t) = source == src && topic == t

  route greeting-to-print =
    from_topic(hello, "hello.greeting") && payload == text -> [print]
  ```

- [x] Add file-level `include` support.

  Large strategy sets will repeat plugin declarations, common predicates, and
  capability templates. A simple file-level include is enough for the first
  version; avoid designing a full module system too early.

  Example target syntax:

  ```cx
  include "../common/plugins.cx"
  include "../common/predicates.cx"
  ```

- [x] Add static checks for unused plugins and unused predicate bindings.

  The compiler should warn or fail when declarations are never referenced. This
  catches stale topology after route edits.

  Example:

  ```cx
  plugin debug_sink = process("../../target/debug/cubex-print-plugin")
  let unused_records = topic == "record.put" && payload == record
  ```

  Neither declaration should silently remain unused.

- [ ] Add static checks for unreachable routes.

  A route can be syntactically valid but impossible to match because its
  predicate is contradictory or shadowed by impossible conditions.

  Example:

  ```cx
  route impossible =
    payload == text && payload == record -> [print]
  ```

  Some contradictions are already caught; this check should become systematic
  and easier to explain.

- [ ] Add static checks for route coverage conflicts or duplicate equivalent route predicates.

  Two routes may compile to identical match conditions. That may be intentional,
  but it is often accidental duplication.

  Example:

  ```cx
  route a = source == hello && topic == "hello.greeting" -> [print]
  route b = topic == "hello.greeting" && source == hello -> [audit]
  ```

  The compiler should report that `a` and `b` have equivalent predicates.

- [ ] Add capability hints.

  Wasm plugins use explicit host capabilities. The compiler can infer likely
  mistakes from plugin args and capability declarations, even if it cannot prove
  every plugin's behavior.

  Example:

  ```cx
  plugin source = wasm("cubex_wasm_file_source_plugin.wasm") {
    args = ["input.txt", "file.read", "text"]
  }
  ```

  This should suggest adding:

  ```cx
  capability file_read("input.txt")
  ```

- [ ] Add cross-route record field type conflict checks.

  A record field used as different types across routes may indicate a policy
  modeling error.

  Example:

  ```cx
  route by-priority-text = record.priority == "high" -> [print]
  route by-priority-int = record.priority == 7 -> [audit]
  ```

  The compiler should identify that `record.priority` is used as both string
  and integer.

- [ ] Extend predicate expressions with carefully bounded forms.

  Current predicates support equality and conjunction. More expressive forms
  would reduce route duplication, but each form should either lower cleanly to
  the current route model or be rejected with a precise explanation.

  Example target syntax:

  ```cx
  source in [alice, bob]
  topic starts_with "record."
  record.priority >= 5
  not record.active == false
  ```

- [ ] Keep non-lowerable expressions as compile-time errors until explicitly supported.

  The runtime still executes static `RouteConfig` values. Expressions that
  cannot lower to that model should fail at compile time instead of introducing
  hidden runtime behavior.

  Example error case:

  ```cx
  route dynamic = topic starts_with "audit." -> [print]
  ```

  Until prefix matching is supported by the compiler or runtime, this should
  fail with a clear "not lowerable" diagnostic.

- [ ] Add first-class access-control policy declarations.

  Access-control rules currently live in plugin args and routes. First-class
  policy declarations would let the compiler generate the plugin args and route
  wiring.

  Example target syntax:

  ```cx
  policy access {
    allow user "alice" topic "record.put"
    deny user "bob" topic "record.put"
  }
  ```

- [ ] Add first-class BLP policy declarations.

  Bell-LaPadula examples currently rely on structured input records and a policy
  plugin. A first-class declaration could make labels, compartments, and action
  rules auditable in the strategy file.

  Example target syntax:

  ```cx
  policy blp {
    level public < finance < vault
    subject alice clearance finance compartments ["runbook"]
    object report classification public compartments []
  }
  ```

- [ ] Add `cubex fmt` support for `.cx` strategy files.

  Formatting should make generated and handwritten strategies consistent. This
  is especially useful once includes and parameterized predicates exist.

  Example command:

  ```sh
  cargo run -p cubex-cli -- fmt examples/hello/cubex.cx
  ```

- [ ] Add `cubex lint` support for `.cx` strategy files.

  Linting should run non-fatal checks such as unused declarations, style issues,
  and policy modeling warnings without necessarily compiling or executing the
  strategy.

  Example command:

  ```sh
  cargo run -p cubex-cli -- lint examples/hello/cubex.cx
  ```

- [ ] Add `cubex explain` support.

  Explain mode should show the compiled topology and static-check summary in a
  reader-friendly form without running plugins.

  Example command:

  ```sh
  cargo run -p cubex-cli -- explain examples/hello/cubex.cx
  ```

  Example output sections:

  ```text
  Plugins:
    hello: process ../../target/debug/cubex-hello-plugin, autostart
    print: process ../../target/debug/cubex-print-plugin

  Routes:
    greeting-to-print: hello hello.greeting text -> print
  ```

- [ ] Add golden tests for every `.cx` example.

  Each example strategy should have a snapshot of the generated TOML or typed
  config output. Compiler changes should then produce obvious diffs when the IR
  changes.

  Example layout:

  ```text
  examples/hello/cubex.cx
  tests/golden/hello.toml
  ```
