/**
 * 录制核心：把字节与 TS parseDocx 的规范化输出写入 EXPORT_GOLDEN_OUT。
 * 命名 <测试文件>__<序号>；同字节内容去重（.hash/<sha256> 标记文件）。
 */
import { createHash } from 'node:crypto'
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import JSZip from 'jszip'
import { expect } from 'vitest'
// 带扩展名的写法不会命中 alias（alias 正则以 `index$` 结尾），拿到的是真实模块
import { parseDocx } from '../src/index.ts'

const OUT = process.env.EXPORT_GOLDEN_OUT
if (!OUT) throw new Error('EXPORT_GOLDEN_OUT is not set (run via tools/export-golden/run.sh)')
const HASH_DIR = join(OUT, '.hash')
mkdirSync(HASH_DIR, { recursive: true })

const counters = new Map<string, number>()

export function sha256(bytes: Uint8Array): string {
  return createHash('sha256').update(bytes).digest('hex')
}

export function currentTest(): { file: string; name: string } {
  const st = expect.getState()
  const file = basename(st.testPath ?? 'unknown').replace(/\.test\.ts$/, '')
  return { file, name: st.currentTestName ?? '' }
}

function nextStem(file: string): string {
  const n = (counters.get(file) ?? 0) + 1
  counters.set(file, n)
  return `${file}__${String(n).padStart(3, '0')}`
}

function manifest(entry: Record<string, unknown>): void {
  appendFileSync(join(OUT!, 'manifest.jsonl'), JSON.stringify(entry) + '\n')
}

type BytesMode = 'omit' | 'base64'

/** TEST-02 步骤 2 的规范化：Map→对象、Uint8Array→省略（或 base64）、undefined→删除、键排序。 */
export function normalize(v: unknown, bytes: BytesMode = 'omit'): unknown {
  if (v === undefined || typeof v === 'function' || typeof v === 'symbol') return undefined
  if (v === null) return null
  if (v instanceof Uint8Array) {
    return bytes === 'omit' ? undefined : { $bytes_base64: Buffer.from(v).toString('base64') }
  }
  if (v instanceof ArrayBuffer) return normalize(new Uint8Array(v), bytes)
  if (v instanceof Map) {
    const o: Record<string, unknown> = {}
    for (const [k, val] of v) o[String(k)] = val
    return normalize(o, bytes)
  }
  if (v instanceof Set) return [...v].map((x) => normalize(x, bytes) ?? null)
  if (Array.isArray(v)) return v.map((x) => normalize(x, bytes) ?? null)
  if (typeof v === 'object') {
    const src = v as Record<string, unknown>
    const o: Record<string, unknown> = {}
    for (const k of Object.keys(src).sort()) {
      const nv = normalize(src[k], bytes)
      if (nv !== undefined) o[k] = nv
    }
    return o
  }
  if (typeof v === 'number' && !Number.isFinite(v)) return null
  if (typeof v === 'bigint') return v.toString()
  return v
}

/** 录制一个合成 docx。返回其 stem（去重时返回首次出现的 stem）。 */
export async function record(bytes: Uint8Array, builder: string, forcedStem?: string): Promise<string> {
  const { file, name } = currentTest()
  const stem = forcedStem ?? nextStem(file)
  const sha = sha256(bytes)
  const marker = join(HASH_DIR, sha)
  if (existsSync(marker)) {
    const original = readFileSync(marker, 'utf8')
    manifest({ stem, test: `${file} > ${name}`, builder, sha256: sha, duplicate_of: original })
    return original
  }
  writeFileSync(join(OUT!, `${stem}.docx`), bytes)
  let status: 'ok' | 'error' = 'ok'
  try {
    const parsed = await parseDocx(bytes)
    writeFileSync(join(OUT!, `${stem}.expected.json`), JSON.stringify(normalize(parsed)))
  } catch (e) {
    status = 'error'
    const message = e instanceof Error ? e.message : String(e)
    writeFileSync(join(OUT!, `${stem}.error.json`), JSON.stringify({ message }, null, 2))
  }
  writeFileSync(marker, stem)
  manifest({ stem, test: `${file} > ${name}`, builder, sha256: sha, parse: status })
  return stem
}

/** 录制一次 saveDocx：按源字节哈希找到 stem，写 <stem>.save.<k>.json。 */
export async function recordSave(
  parsed: { internal?: { originalBytes?: Uint8Array } },
  blocks: unknown,
  options: unknown,
  output: Uint8Array,
): Promise<void> {
  const { file, name } = currentTest()
  const src = parsed.internal?.originalBytes
  if (!src) return
  const marker = join(HASH_DIR, sha256(src))
  if (!existsSync(marker)) {
    // 源文档不是经 buildDocx 录制的（例如上一次 saveDocx 的输出再解析）：只记 manifest
    manifest({ test: `${file} > ${name}`, builder: 'saveDocx', save: 'source_not_recorded' })
    return
  }
  const stem = readFileSync(marker, 'utf8')
  let k = 1
  while (existsSync(join(OUT!, `${stem}.save.${k}.json`))) k++
  let documentXml: string | null = null
  try {
    const zip = await JSZip.loadAsync(output)
    documentXml = (await zip.file('word/document.xml')?.async('string')) ?? null
  } catch {
    documentXml = null
  }
  const payload = {
    test: `${file} > ${name}`,
    blocks: normalize(blocks, 'base64'),
    options: normalize(options ?? {}, 'base64'),
    outputSha256: sha256(output),
    outputIdenticalToSource: sha256(output) === sha256(src),
    documentXml,
  }
  writeFileSync(join(OUT!, `${stem}.save.${k}.json`), JSON.stringify(payload))
  manifest({ stem, test: `${file} > ${name}`, builder: 'saveDocx', save: `${stem}.save.${k}.json` })
}
