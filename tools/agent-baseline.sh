#!/usr/bin/env bash
# M9' 9.0 / TEST-10：实测当前协议；outline/text 仅记录未实现，不伪造接口结果。
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$root/target"
probe=$(mktemp -d "$root/target/agent-baseline.XXXXXX")
trap 'rm -rf "$probe"' EXIT
mkdir "$probe/src"
cp "$root/Cargo.lock" "$probe/Cargo.lock"
cat > "$probe/Cargo.toml" <<TOML
[package]
name = "rsword-agent-baseline"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
rsword = { path = "$root/crates/rsword" }
TOML
cat > "$probe/src/main.rs" <<'RS'
use rsword::bind::native::SessionTable;
fn main() {
    let path = std::env::args().nth(1).expect("input docx");
    let mut table = SessionTable::default();
    let id = table.open(&std::fs::read(path).unwrap(), None).unwrap();
    print!("{}", table.document(&id, Some(r#"{"display":false}"#)).unwrap());
}
RS
if [ "$#" -gt 0 ]; then
    input=$1
else
    input=$(ruby - "$root" <<'RUBY_INPUT'
paths = Dir["#{ARGV.fetch(0)}/corpus/real/**/*.docx"].reject { |p| p.split('/').include?('edited') }
abort 'real corpus count drifted' unless paths.size == 266
puts paths.max_by { |p| [File.size(p), p] }
RUBY_INPUT
)
fi
CARGO_TARGET_DIR="$root/target/native-default-probe" cargo run --quiet --manifest-path "$probe/Cargo.toml" -- "$input" > "$probe/document.json"
ruby -rjson -rdigest - "$input" "$probe/document.json" <<'RUBY'
input, output = ARGV
raw = File.read(output)
j = JSON.parse(raw)
# 这是可重复的字节预算代理，非某个模型 tokenizer 的实测 token 数或硬上界。
def metrics(s)
  { bytes: s.bytesize, unicode_scalars: s.length, utf16_units: s.encode('UTF-16LE').bytesize / 2,
    estimated_tokens_bytes_div_4: (s.bytesize + 3) / 4 }
end
headings = j.fetch('main').each_with_index.map do |b, i|
  next unless b.dig('textKind', 'kind') == 'heading'
  [i, b.fetch('node'), b.dig('textKind', 'level'), b.fetch('inlines').map { |r| r['text'] }.join]
end.compact
puts JSON.pretty_generate({input: input, zip_bytes: File.size(input), input_sha256: Digest::SHA256.file(input).hexdigest,
  document: metrics(raw).merge(sha256: Digest::SHA256.hexdigest(raw)), total_blocks: j.fetch('totalBlocks'),
  truncated: j.fetch('truncated'), heading_inventory: headings,
  outline: 'not implemented; not measured', text: 'not implemented; not measured'})
RUBY
