// AGENT-07/10：执行 docs/17 的原始请求，不另维护一份样例表。
import {readFileSync, writeFileSync, mkdtempSync, mkdirSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {resolve, join} from 'node:path';
import {spawnSync} from 'node:child_process';
const root = fileURLToPath(new URL('../', import.meta.url));
const [action, binary = join(root, 'target/debug/rsword')] = process.argv.slice(2);
// Windows checkout 可使用 CRLF；统一换行后再提取 Markdown 代码块。
const docs = readFileSync(join(root, 'docs/17-agent-edit.md'), 'utf8').replaceAll('\r\n', '\n');
const examples = [...docs.matchAll(/<!-- agent-example (\w+) ([\w/.-]+) (\w+) -->\n```json\n([\s\S]*?)\n```/g)];
const example = examples.find(m => m[1] === action);
if (!example) throw new Error(`选择 action：${examples.map(m => m[1]).join(', ')}`);
const base = join(root, 'target/agent-edit-examples');
mkdirSync(base, {recursive:true});
const dir = mkdtempSync(join(base, `${action}-`));
function run(args) {
    const result = spawnSync(resolve(binary), args, {cwd:root, encoding:'utf8'});
    if (result.error) throw result.error;
    return result;
}
if (action === 'replaceImage') {
    const exported = run(['media', 'corpus/real/image/image-svg.docx', '--id', '0', '--output', join(dir,'replacement.png'), '--json']);
    if (exported.status !== 0) throw new Error(exported.stdout + exported.stderr);
}
const request = join(dir,'request.json');
writeFileSync(request, example[4] + '\n');
const result = run(['ops', `corpus/real/${example[2]}`, '--ops', request, '--output', join(dir,'output.docx'), '--json']);
const value = JSON.parse(result.stdout);
if (example[3] === 'ok' ? result.status !== 0 : result.status === 0 || value.code !== example[3]) {
    throw new Error(`样例 ${action} 与预期不符：${result.stdout} ${result.stderr}`);
}
process.stdout.write(result.stdout + '\n');
