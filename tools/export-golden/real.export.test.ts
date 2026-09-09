/**
 * 真实 Word 语料（corpus/real/**\/*.docx，`docs/07`）的 TS 参考输出：对每份 docx 跑 parseDocx，
 * 把规范化 JSON 写到同目录的 <stem>.expected.json（解析失败写 <stem>.error.json）。**不改写 docx**、
 * 不走去重 / manifest（源文件不是 buildDocx 造的）。运行：
 *   EXPORT_GOLDEN_REAL_ROOT=<仓库>/corpus/real tools/export-golden/try.sh real.export.test.ts
 */
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parseDocx } from '../src/index.ts'
import { normalize } from './record'

const ROOT = process.env.EXPORT_GOLDEN_REAL_ROOT
if (!ROOT) throw new Error('EXPORT_GOLDEN_REAL_ROOT is not set (repo corpus/real)')

function walk(dir: string, out: string[]): void {
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) {
      if (name !== 'edited') walk(p, out) // edited/ 是引擎产物，不录
    }
    else if (name.endsWith('.docx') && !name.startsWith('~$')) out.push(p)
  }
}

describe('real word corpus', () => {
  const files: string[] = []
  walk(ROOT, files)
  it(`records ${files.length} documents`, async () => {
    let ok = 0
    let failed = 0
    for (const f of files) {
      const bytes = new Uint8Array(readFileSync(f))
      const stem = f.replace(/\.docx$/, '')
      try {
        const parsed = await parseDocx(bytes)
        writeFileSync(`${stem}.expected.json`, JSON.stringify(normalize(parsed)))
        ok++
      } catch (e) {
        const message = e instanceof Error ? `${e.message}\n${e.stack ?? ''}` : String(e)
        writeFileSync(`${stem}.error.json`, JSON.stringify({ message }, null, 2))
        failed++
      }
    }
    // eslint-disable-next-line no-console
    console.log(`real corpus: ${ok} parsed, ${failed} failed`)
    expect(ok + failed).toBe(files.length)
  }, 600_000)
})
