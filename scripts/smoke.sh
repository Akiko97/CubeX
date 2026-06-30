#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cleanup() {
  rm -f examples/alice-bob/received.txt \
    examples/file-flow/output.txt \
    examples/bytes-flow/output.bin \
    examples/record-store/events.bin \
    examples/record-store/records.bin \
    examples/wasm-file-flow/output.txt \
    examples/wasm-bytes-flow/output.bin \
    examples/wasm-record-store/events.bin \
    examples/wasm-record-store/records.bin
}
trap cleanup EXIT
cleanup

run_and_expect() {
  local config="$1"
  local expected="$2"
  local output
  output="$(cargo run -q -p cubex-cli -- run --strict -c "$config")"
  grep -Fq "$expected" <<<"$output"
}

check_toml_configs() {
  local config
  for config in examples/*/*.toml; do
    [[ "$(basename "$config")" == "Cargo.toml" ]] && continue
    cargo run -q -p cubex-cli -- check -c "$config" >/dev/null
    cargo run -q -p cubex-cli -- check --strict -c "$config" >/dev/null
  done
}

check_strategy_configs() {
  local strategy
  for strategy in examples/*/*.cx; do
    cargo run -q -p cubex-cli -- check -c "$strategy" >/dev/null
    cargo run -q -p cubex-cli -- check --strict -c "$strategy" >/dev/null
    cargo run -q -p cubex-cli -- compile "$strategy" >/dev/null
  done
}

run_record_store_suite() {
  local ext="$1"

  run_and_expect "examples/record-store/cubex${ext}" "print: demo-record"
  test -s examples/record-store/events.bin
  cargo run -q -p cubex-cli -- events examples/record-store/events.bin | grep -Fq $'record-source\trecord.put'
  test -s examples/record-store/records.bin
  cargo run -q -p cubex-cli -- records examples/record-store/records.bin | grep -Fq $'demo-record\t'
  run_and_expect "examples/record-store/get${ext}" "print: record demo-record found from record-source"
  run_and_expect "examples/record-store/list${ext}" "print: records: demo-record"
  run_and_expect "examples/record-store/delete${ext}" "print: record demo-record deleted"
  run_and_expect "examples/record-store/replay${ext}" "replayed=2"
  cargo run -q -p cubex-cli -- records examples/record-store/records.bin | grep -Fq $'demo-record\t'
}

run_example_suite() {
  local ext="$1"

  run_and_expect "examples/hello/cubex${ext}" "print: Hello, CubeX!"
  if [[ "$ext" == ".cx" ]]; then
    run_and_expect examples/strategy/hello.cx "print: Hello, CubeX!"
  fi
  run_and_expect "examples/access-control/cubex${ext}" '"decision": String("allowed")'
  run_and_expect "examples/access-control/cubex${ext}" '"decision": String("denied")'
  run_and_expect "examples/blp/cubex${ext}" '"reason": String("read-allowed")'
  run_and_expect "examples/blp/cubex${ext}" '"reason": String("read-up")'
  run_and_expect "examples/blp/cubex${ext}" '"reason": String("write-down")'
  run_and_expect "examples/blp/cubex${ext}" '"reason": String("write-allowed")'
  run_and_expect "examples/record-route/cubex${ext}" 'print: Record({"active": Bool(true), "message": String("from record file"), "priority": I64(7), "user": String("alice")})'
  run_and_expect "examples/register-bank/cubex${ext}" 'print: Record({"address": U64(7), "value": U64(42)})'
  run_and_expect "examples/timer/cubex${ext}" 'print: Record({"count": U64(3), "index": U64(2)})'
  run_and_expect "examples/crypto/cubex${ext}" '9be26ffe0395269a8e85bd7a3278ef853d86138eb11354308dfdfb2a63b8d85a'
  run_and_expect "examples/crypto/cubex${ext}" '7f4617f80e9020429b94e1ec86c8b0631d36f1ff76efa95920430f793477fd4a'
  run_and_expect "examples/network/cubex${ext}" "print: network ping"
  run_and_expect "examples/plugin-project/cubex${ext}" "print: hello from a real plugin project"
  run_and_expect "examples/alice-bob/cubex${ext}" "print: received.txt"
  run_and_expect "examples/alice-bob/cubex${ext}" "cb906c4ac6482bf714776a5373a198b06adc14fc30eaa83e3990034d18029c7d"
  cmp -s examples/alice-bob/message.txt examples/alice-bob/received.txt
  run_and_expect "examples/wasm-hello/cubex${ext}" "wasm-print: Hello, CubeX!"
  run_and_expect "examples/wasm-process-hello/cubex${ext}" "print: Hello, CubeX!"
  run_and_expect "examples/wasm-crypto/cubex${ext}" '9be26ffe0395269a8e85bd7a3278ef853d86138eb11354308dfdfb2a63b8d85a'
  run_and_expect "examples/wasm-access-control/cubex${ext}" '"decision": String("allowed")'
  run_and_expect "examples/wasm-access-control/cubex${ext}" '"decision": String("denied")'
  run_and_expect "examples/wasm-register-bank/cubex${ext}" 'wasm-print: Record({"address": U64(7), "value": U64(42)})'
  run_and_expect "examples/wasm-timer/cubex${ext}" 'wasm-print: Record({"count": U64(3), "index": U64(2)})'
  run_and_expect "examples/wasm-random/cubex${ext}" "wasm-print: random bytes: "
  run_and_expect "examples/wasm-network/cubex${ext}" "wasm-print: wasm network ping"
  run_and_expect "examples/wasm-record-store/cubex${ext}" "wasm-print: demo-record"

  run_and_expect "examples/file-flow/cubex${ext}" "print: output.txt"
  cmp -s examples/file-flow/input.txt examples/file-flow/output.txt
  run_and_expect "examples/bytes-flow/cubex${ext}" "print: output.bin"
  cmp -s examples/bytes-flow/input.bin examples/bytes-flow/output.bin
  run_and_expect "examples/wasm-file-flow/cubex${ext}" "wasm-print: output.txt"
  cmp -s examples/wasm-file-flow/input.txt examples/wasm-file-flow/output.txt
  run_and_expect "examples/wasm-bytes-flow/cubex${ext}" "wasm-print: output.bin"
  cmp -s examples/wasm-bytes-flow/input.bin examples/wasm-bytes-flow/output.bin

  run_record_store_suite "$ext"
}

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo build --target wasm32-unknown-unknown \
  -p cubex-wasm-access-policy-plugin \
  -p cubex-wasm-echo-plugin \
  -p cubex-wasm-file-sink-plugin \
  -p cubex-wasm-file-source-plugin \
  -p cubex-wasm-hello-plugin \
  -p cubex-wasm-print-plugin \
  -p cubex-wasm-random-plugin \
  -p cubex-wasm-record-source-plugin \
  -p cubex-wasm-record-store-plugin \
  -p cubex-wasm-register-bank-plugin \
  -p cubex-wasm-register-client-plugin \
  -p cubex-wasm-sha256-plugin \
  -p cubex-wasm-tcp-client-plugin \
  -p cubex-wasm-tcp-echo-plugin \
  -p cubex-wasm-timer-plugin

check_toml_configs
check_strategy_configs
run_example_suite ".toml"
cleanup
run_example_suite ".cx"

echo "smoke ok"
