/**
 * `fixtures/fieldgen/`：TS 的生成器对固定输入的输出（`spec/18` 7.0④）。生成器在 `apps/docs`
 * 的功能区调用、不经 `docx-engine` 的测试，语料里没有，所以单独落盘一份对照件。
 *
 * 只跑这一个文件：`tools/export-golden/try.sh fieldgen.export.test.ts <out>`。
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { latexToOmml, mathParagraphXml } from '../src/math'

const OUT = process.env.EXPORT_GOLDEN_HOSTILE_OUT
if (!OUT) throw new Error('EXPORT_GOLDEN_HOSTILE_OUT is not set')
// 写进仓库的 `fixtures/fieldgen`（`EXPORT_GOLDEN_HOSTILE_OUT` 是 `<repo>/corpus/hostile`）。
// `try.sh` 下它落在临时目录旁边，照 README 拷过来即可
const DIR = join(OUT, '..', '..', 'fixtures', 'fieldgen')
mkdirSync(DIR, { recursive: true })

/** 覆盖 latexToOmml 的每一条分支 */
const LATEX: string[] = [
  'x + 1',
  'a^2 + b^2 = c^2',
  'ab^2',
  'x_i^{n+1}',
  '\\frac{a}{b}',
  '\\dfrac{1}{2} + \\tfrac{3}{4}',
  '\\binom{n}{k}',
  '\\sqrt{x}',
  '\\sqrt[3]{x+1}',
  '\\overline{AB} \\underline{CD}',
  '\\underbrace{a+b} \\overbrace{c+d}',
  '\\sum_{i=1}^{n} i',
  '\\int_0^1 {x^2}',
  '\\oint {f}',
  '\\prod_{k} {k}',
  '\\hat{a} \\bar{b} \\vec{c} \\dot{d} \\ddot{e} \\tilde{f} \\check{g} \\breve{h}',
  '\\sin x + \\cos y + \\tan z',
  '\\lim_{x \\to 0} \\frac{\\sin x}{x}',
  '\\lim x',
  '\\text{hello world}',
  '\\mathrm{d}x',
  '\\operatorname{tr}(A)',
  '\\left( \\frac{a}{b} \\right)',
  '\\left[ x \\right]',
  '\\left\\{ x \\right\\}',
  '\\left| x \\right|',
  '\\left\\langle x \\right\\rangle',
  '\\left. x \\right)',
  '\\begin{matrix} a & b \\\\ c & d \\end{matrix}',
  '\\begin{pmatrix} 1 & 0 \\\\ 0 & 1 \\end{pmatrix}',
  '\\begin{bmatrix} a \\end{bmatrix}',
  '\\begin{Bmatrix} a \\end{Bmatrix}',
  '\\begin{vmatrix} a \\end{vmatrix}',
  '\\begin{Vmatrix} a \\end{Vmatrix}',
  '\\begin{cases} x & x > 0 \\\\ -x & x \\le 0 \\end{cases}',
  '\\alpha\\beta\\gamma \\Delta \\infty \\le \\ge \\ne \\approx',
  'a \\, b \\; c \\quad d \\qquad e',
  '\\{ x \\}',
  '\\% \\& \\$ \\# \\_ \\^',
  '{a+b}^2',
  'f(x) = \\frac{1}{\\sqrt{2\\pi}} e^{-\\frac{x^2}{2}}',
  '5 < 6 > 7',
]

/** 解析不了的输入：记下错误信息 */
const LATEX_ERRORS: string[] = [
  '\\unknowncmd{x}',
  '\\frac{a}',
  '{a',
  'a}',
  '\\left( x',
  '\\begin{unknownenv} a \\end{unknownenv}',
  '\\begin{matrix} a',
  '\\sqrt[3{x}',
  '\\\\',
  '^',
  '5 & 6',
]

describe('fieldgen fixtures', () => {
  it('latexToOmml + mathParagraphXml', () => {
    const omml: Record<string, string> = {}
    for (const src of LATEX) omml[src] = latexToOmml(src)
    const errors: Record<string, string> = {}
    for (const src of LATEX_ERRORS) {
      try {
        latexToOmml(src)
        errors[src] = ''
      } catch (e) {
        errors[src] = (e as Error).message
      }
    }
    const paras: Record<string, string> = {}
    for (const align of ['left', 'center', 'right'] as const) {
      paras[align] = mathParagraphXml(latexToOmml('a^2'), align)
    }
    writeFileSync(
      join(DIR, 'latex.json'),
      `${JSON.stringify({ omml, errors, paragraphs: paras }, null, 2)}\n`,
    )
    expect(Object.keys(omml).length).toBe(LATEX.length)
  })
})
