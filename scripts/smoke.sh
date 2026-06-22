#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cleanup() {
  rm -f examples/file-flow/output.txt \
    examples/bytes-flow/output.bin \
    examples/record-store/events.bin \
    examples/record-store/records.bin
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

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace

for config in examples/*/*.toml; do
  [[ "$(basename "$config")" == "Cargo.toml" ]] && continue
  cargo run -q -p cubex-cli -- check -c "$config" >/dev/null
  cargo run -q -p cubex-cli -- check --strict -c "$config" >/dev/null
done

run_and_expect examples/hello/cubex.toml "print: Hello, CubeX!"
run_and_expect examples/access-control/cubex.toml '"decision": String("allowed")'
run_and_expect examples/access-control/cubex.toml '"decision": String("denied")'
run_and_expect examples/record-route/cubex.toml 'print: Record({"active": Bool(true), "message": String("from record file"), "priority": I64(7), "user": String("alice")})'
run_and_expect examples/register-bank/cubex.toml 'print: Record({"address": U64(7), "value": U64(42)})'
run_and_expect examples/timer/cubex.toml 'print: Record({"count": U64(3), "index": U64(2)})'
run_and_expect examples/crypto/cubex.toml '9be26ffe0395269a8e85bd7a3278ef853d86138eb11354308dfdfb2a63b8d85a'
run_and_expect examples/crypto/cubex.toml '7f4617f80e9020429b94e1ec86c8b0631d36f1ff76efa95920430f793477fd4a'
run_and_expect examples/network/cubex.toml "print: network ping"
run_and_expect examples/plugin-project/cubex.toml "print: hello from a real plugin project"

run_and_expect examples/file-flow/cubex.toml "print: output.txt"
cmp -s examples/file-flow/input.txt examples/file-flow/output.txt
run_and_expect examples/bytes-flow/cubex.toml "print: output.bin"
cmp -s examples/bytes-flow/input.bin examples/bytes-flow/output.bin

run_and_expect examples/record-store/cubex.toml "print: demo-record"
test -s examples/record-store/events.bin
cargo run -q -p cubex-cli -- events examples/record-store/events.bin | grep -Fq $'record-source\trecord.put'
test -s examples/record-store/records.bin
cargo run -q -p cubex-cli -- records examples/record-store/records.bin | grep -Fq $'demo-record\t'
run_and_expect examples/record-store/get.toml "print: record demo-record found from record-source"
run_and_expect examples/record-store/list.toml "print: records: demo-record"
run_and_expect examples/record-store/delete.toml "print: record demo-record deleted"
run_and_expect examples/record-store/replay.toml "replayed=2"
cargo run -q -p cubex-cli -- records examples/record-store/records.bin | grep -Fq $'demo-record\t'

echo "smoke ok"
