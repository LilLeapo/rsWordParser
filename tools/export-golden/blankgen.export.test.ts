/**
 * `fixtures/fieldgen/{blank,generators}.json`：TS 空白模板的每个 part、以及三个块字段生成器
 * 对固定输入的输出（`spec/18` 7.8）。这些函数在 `apps/docs` 的功能区调用、不经 `docx-engine`
 * 的测试，语料里没有，所以单独落盘一份对照件。
 *
 * 只跑这一个文件：`tools/export-golden/try.sh blankgen.export.test.ts <out>`。
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import JSZip from 'jszip'
import { describe, expect, it } from 'vitest'
import { buildBlankDocx, BLANK_BULLET_NUM_ID, BLANK_ORDERED_NUM_ID } from '../src/blank'
import {
  generateCaptionXml,
  generateIndexFieldXml,
  generateTocFieldXml,
  type TocEntry,
} from '../src/generate'

const OUT = process.env.EXPORT_GOLDEN_HOSTILE_OUT
if (!OUT) throw new Error('EXPORT_GOLDEN_HOSTILE_OUT is not set')
// 写进仓库的 `fixtures/fieldgen`（`EXPORT_GOLDEN_HOSTILE_OUT` 是 `<repo>/corpus/hostile`）。
// `try.sh` 下它落在临时目录旁边，照 README 拷过来即可
const DIR = join(OUT, '..', '..', 'fixtures', 'fieldgen')
mkdirSync(DIR, { recursive: true })

async function parts(bytes: Uint8Array): Promise<Record<string, string>> {
  const zip = await JSZip.loadAsync(bytes)
  const out: Record<string, string> = {}
  for (const name of Object.keys(zip.files).sort()) {
    if (zip.files[name].dir) continue
    out[name] = await zip.files[name].async('string')
  }
  return out
}

const TOC: TocEntry[][] = [
  [
    { level: 1, text: '第一章', pageNo: 1 },
    { level: 2, text: '第一节 概述', pageNo: 2 },
    { level: 3, text: 'Details & <notes>', pageNo: 3 },
  ],
  [{ level: 1, text: '只有一条' }],
  [
    { level: 4, text: 'deep', pageNo: 10 },
    { level: 1, text: 'top' },
  ],
]

const CAPTIONS: Array<[string, number, string]> = [
  ['图', 1, '系统架构'],
  ['Figure', 12, ''],
]

const INDEX: string[][] = [
  ['banana', ' apple ', 'Apple', '', 'apple', '中文', 'Ähnlich'],
  ['only'],
]

describe('blank template + block field generators', () => {
  it('writes blank.json and generators.json', async () => {
    const blank = {
      default: await parts(await buildBlankDocx()),
      eastAsia: await parts(await buildBlankDocx({ eastAsiaFont: '等线' })),
      bulletNumId: BLANK_BULLET_NUM_ID,
      orderedNumId: BLANK_ORDERED_NUM_ID,
    }
    writeFileSync(join(DIR, 'blank.json'), `${JSON.stringify(blank, null, 2)}\n`)

    const generators = {
      toc: TOC.map((entries) => ({ entries, xml: generateTocFieldXml(entries) })),
      tocEmpty: generateTocFieldXml([]),
      caption: CAPTIONS.map(([label, n, text]) => ({
        label,
        number: n,
        text,
        xml: generateCaptionXml(label, n, text),
      })),
      index: INDEX.map((terms) => ({ terms, xml: generateIndexFieldXml(terms) })),
      indexEmpty: generateIndexFieldXml(['', '  ']),
    }
    writeFileSync(join(DIR, 'generators.json'), `${JSON.stringify(generators, null, 2)}\n`)
    expect(Object.keys(blank.default)).toContain('word/styles.xml')
  })
})
