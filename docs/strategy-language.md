# CubeX Strategy Language

CubeX Strategy is an experimental, strongly functional front end for CubeX
configuration and policy routing. A `.cx` file compiles to the existing
`cubex_core::Config` model, so the runtime still executes the same plugin graph,
message protocol, route dispatch, event log, and strict file checks as TOML.

The first version is intentionally a compilable subset rather than a new
runtime. The compiler accepts pure route predicates that can be lowered to
CubeX's current static route fields: `source`, `topic`, `payload`, and exact
`record.<field>` matches.

## Design

The language is designed around strong functional constraints:

- Top-level declarations are immutable bindings: `plugin`, `let`, `fn`, and
  `route` names cannot be rebound.
- `let` and `fn` declarations define pure predicates. A predicate has no IO, no
  mutation, no time dependency, and no access to plugin state.
- `route name = predicate -> [targets]` is treated as a pure function from a
  message to a fixed list of plugin targets.
- Composition is explicit with `&&` and predicate references. The first compiler
  release rejects expressions that cannot be normalized into a single static
  CubeX route.
- Effects remain inside plugins. The strategy compiler only builds the topology,
  capability declarations, and policy routing table.

This keeps CubeX's core scheduler small while making higher-level policy intent
auditable before execution.

## Syntax

```cx
strategy "hello-example" {
  engine {
    max_messages = 32
  }

  plugin hello = process("../../target/debug/cubex-hello-plugin") {
    args = ["CubeX"]
    autostart = true
  }

  plugin print = process("../../target/debug/cubex-print-plugin")

  let greetings =
    source == hello && topic == "hello.greeting" && payload == text

  route greeting-to-print = greetings -> [print]
}
```

The strategy name becomes `engine.name` unless the optional engine block sets
`name` explicitly:

```cx
strategy "display-name" {
  engine {
    name = "runtime-name"
    max_messages = 128
  }
}
```

Strategy files may include shared declarations before the `strategy` block:

```cx
include "../common/plugins.cx"
include "../common/predicates.cx"

strategy "hello-example" {
  route greeting-to-print =
    from_topic(hello, "hello.greeting", text) -> [print]
}
```

Included files may be declaration fragments:

```cx
plugin print = process("../../target/debug/cubex-print-plugin")

fn from_topic(src, t, kind) =
  source == src && topic == t && payload == kind
```

Includes are expanded in order before the including file's declarations. Include
paths resolve relative to the file that contains the `include` statement. Paths
declared inside an included file, such as plugin commands, Wasm paths,
`working_dir`, store paths, and file capabilities, resolve relative to that
included file.

Store configuration maps directly to `[store]`:

```cx
store {
  path = "events.bin"
  replay_on_start = true
}
```

Process plugins use `process(path)`. Wasm plugins use `wasm(path)` and may
declare host capabilities:

```cx
plugin file_source = wasm("../../target/wasm32-unknown-unknown/debug/cubex_wasm_file_source_plugin.wasm") {
  working_dir = "."
  args = ["input.txt", "file.read", "text"]
  autostart = true
  capability file_read("input.txt")
}
```

Supported capability forms are:

- `capability file_read("path")`
- `capability file_write("path")`
- `capability tcp_connect("127.0.0.1:9000")`
- `capability tcp_listen("127.0.0.1:9000")`
- `capability timer`
- `capability record_store("records.bin")`

## Predicates

First-version predicates support equality and conjunction:

```cx
let alice_records =
  topic == "record.put" &&
  payload == record &&
  record.user == "alice" &&
  record.priority == 7 &&
  record.active == true
```

Parameterized predicates use `fn` declarations. Parameters stand for comparison
values, so shared policy can be expressed once and called from routes or other
predicate functions:

```cx
fn from_topic(src, t, kind) =
  source == src && topic == t && payload == kind

route greeting-to-print =
  from_topic(hello, "hello.greeting", text) -> [print]
```

Fields:

- `source == hello`
- `source == "hello"`
- `topic == "hello.greeting"`
- `payload == text`
- `payload == bytes`
- `payload == record`
- `payload == control`
- `record.<field> == "string"`
- `record.<field> == 7`
- `record.<field> == true`

Record predicates imply `payload == record` when no payload predicate is present.
If a route combines `record.<field>` with a non-record payload, compilation
fails.

## Static Checks

The strategy compiler performs checks before handing the config to CubeX:

- duplicate top-level bindings;
- duplicate engine/store fields;
- process plugins declaring Wasm-only capabilities;
- unknown route targets;
- unknown `source` references;
- unknown `let` predicate references;
- unknown `fn` predicate calls;
- predicate function argument count mismatches;
- duplicate predicate function parameter names;
- cycles between predicates;
- conflicting predicates such as `payload == text && payload == record`;
- duplicate or conflicting record field predicates;
- empty target lists.
- missing include files and include cycles.

The generated `Config` is still validated by `cubex-core`, and `cubex check
--strict` continues to verify runtime files.

## CLI

Compile a strategy to TOML:

```sh
cargo run -p cubex-cli -- compile examples/strategy/hello.cx
```

Write the generated TOML:

```sh
cargo run -p cubex-cli -- compile examples/strategy/hello.cx -o generated.toml
```

Run or check a strategy directly:

```sh
cargo run -p cubex-cli -- check -c examples/strategy/hello.cx
cargo run -p cubex-cli -- run --strict -c examples/strategy/hello.cx
```

Paths inside `.cx` files resolve relative to the file where they are declared,
matching TOML configuration behavior for single-file strategies.
