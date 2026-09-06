/**
 * M6 B 组语料（任务书 §6）：OMML 公式（B1–B7）/ ruby（B8）/ OLE w:object（B9）/
 * 墨迹（B10–B11）/ 新图片与 replaceImage 的保存用例（B12–B14）。
 * 写法照 hostile.export.test.ts：real.buildDocx 造字节 → record 落 golden；
 * 保存用例先录源文档，saveDocx 的产物再 record 成新的解析语料
 *（.save.<k>.json 由 src-index 包装按源字节哈希自动挂到源 stem 上）。
 */
import JSZip from 'jszip'
import { describe, expect, it } from 'vitest'
import * as real from '../tests/helpers/build-docx'
import { record } from './record'
import {
  parseDocx,
  saveDocx,
  type ImageWrap,
  type NewImage,
  type NewInkImage,
  type SaveBlock,
} from '../src/index'

type Parsed = Awaited<ReturnType<typeof parseDocx>>

const counters: Record<string, number> = {}
/** 每个前缀独立编号：m6-omml__001、m6-ink__001 … */
function stem(prefix: string): string {
  counters[prefix] = (counters[prefix] ?? 0) + 1
  return `${prefix}__${String(counters[prefix]).padStart(3, '0')}`
}

/** 构造 + 录制 + 解析一份文档 */
async function recordAndParse(
  prefix: string,
  options: real.BuildDocxOptions,
): Promise<{ bytes: Uint8Array; stem: string; parsed: Parsed }> {
  const name = stem(prefix)
  // 与 embedded-graphics 同一做法：正文开头一条带 stem 的 XML 注释，让每份源文档字节唯一——
  // 否则与 genoffice 自己测试里同形的文档会被录制器按哈希去重、抢走对方的 stem（ink__001 等曾因此消失）。
  // TS 与 Rust 两边的 bodyInnerStart / elements 都跳过注释，已核对无差异。
  const bytes = await real.buildDocx({ ...options, bodyXml: `<!--${name}-->${options.bodyXml}` })
  const s = await record(bytes, 'buildDocx', name)
  return { bytes, stem: s, parsed: await parseDocx(bytes) }
}

/** 全部可见块原样保留（无编辑保存的 blocks 形态） */
function asOriginal(parsed: Parsed): SaveBlock[] {
  return parsed.blocks
    .filter((b) => !b.hidden)
    .map((b) => ({ kind: 'original' as const, docxIndex: b.docxIndex! }))
}

/** 保存产物再录制成一份新的解析语料，返回其解析结果 */
async function recordOutput(prefix: string, bytes: Uint8Array): Promise<Parsed> {
  await record(bytes, 'saveDocx-output', stem(prefix))
  return parseDocx(bytes)
}

async function zipOf(bytes: Uint8Array): Promise<JSZip> {
  return JSZip.loadAsync(bytes)
}

async function mediaPaths(bytes: Uint8Array): Promise<string[]> {
  const zip = await zipOf(bytes)
  return Object.keys(zip.files).filter((p) => /^word\/media\/[^/]+$/.test(p) && !zip.files[p].dir)
}

async function documentXmlOf(bytes: Uint8Array): Promise<string> {
  return (await zipOf(bytes)).file('word/document.xml')!.async('string')
}

async function documentRelsOf(bytes: Uint8Array): Promise<string> {
  return (await zipOf(bytes)).file('word/_rels/document.xml.rels')!.async('string')
}

/** 单段普通文字 */
const P = (t: string) => `<w:p><w:r><w:t>${t}</w:t></w:r></w:p>`
/** 一个 m:r 文本 run */
const mr = (t: string) => `<m:r><m:t>${t}</m:t></m:r>`
/** 纯公式段 */
const MP = (inner: string) => `<w:p><m:oMath>${inner}</m:oMath></w:p>`

const FRAC_AB = `<m:f><m:num>${mr('a')}</m:num><m:den>${mr('b')}</m:den></m:f>`
const FRAC_CD = `<m:f><m:num>${mr('c')}</m:num><m:den>${mr('d')}</m:den></m:f>`

const HYPERLINK_REL =
  '<Relationship Id="rId20" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/>'

// ---------------------------------------------------------------- B1–B7 OMML

/** B1：每种 OMML 元素一份纯公式段；latex 缺失（TS 子集外）也照录 */
const OMML_ELEMENTS: Array<{
  name: string
  omml: string
  tokens: string[]
  mathml: string[]
  mathmlNot?: string[]
  /** string：latex 应包含该片段；null：latex 应缺失；undefined：不断言 */
  latex?: string | null
  latexNot?: string
}> = [
  {
    name: 'm:f 隐藏分数线 noBar',
    omml: `<m:f><m:fPr><m:type m:val="noBar"/></m:fPr><m:num>${mr('a')}</m:num><m:den>${mr('b')}</m:den></m:f>`,
    tokens: ['a', 'b'],
    mathml: ['<mfrac linethickness="0">'],
    latex: null, // 顶层 noBar 在 TS LaTeX 子集外（只接受 \\binom 的 m:d 包装形态）
  },
  {
    name: 'm:f 普通分数线',
    omml: FRAC_AB,
    tokens: ['a', 'b'],
    mathml: ['<mfrac>'],
    latex: '\\frac{a}{b}',
  },
  {
    name: 'm:rad degHide 隐藏次数',
    omml: `<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg>${mr('3')}</m:deg><m:e>${mr('x')}</m:e></m:rad>`,
    tokens: ['3', 'x'],
    mathml: ['<msqrt>'],
    latex: '\\sqrt{x}',
  },
  {
    name: 'm:rad 带次数',
    omml: `<m:rad><m:deg>${mr('3')}</m:deg><m:e>${mr('x')}</m:e></m:rad>`,
    tokens: ['3', 'x'],
    mathml: ['<mroot>'],
    latex: '\\sqrt[3]{x}',
  },
  {
    name: 'm:sSup 上标',
    omml: `<m:sSup><m:e>${mr('x')}</m:e><m:sup>${mr('2')}</m:sup></m:sSup>`,
    tokens: ['x', '2'],
    mathml: ['<msup>'],
    latex: '{x}^{2}',
  },
  {
    name: 'm:sSub 下标',
    omml: `<m:sSub><m:e>${mr('a')}</m:e><m:sub>${mr('i')}</m:sub></m:sSub>`,
    tokens: ['a', 'i'],
    mathml: ['<msub>'],
    latex: '{a}_{i}',
  },
  {
    name: 'm:sSubSup 上下标',
    omml: `<m:sSubSup><m:e>${mr('x')}</m:e><m:sub>${mr('i')}</m:sub><m:sup>${mr('2')}</m:sup></m:sSubSup>`,
    tokens: ['x', 'i', '2'],
    mathml: ['<msubsup>'],
    latex: '{x}_{i}^{2}',
  },
  {
    name: 'm:sPre 前标',
    omml: `<m:sPre><m:sub>${mr('a')}</m:sub><m:sup>${mr('b')}</m:sup><m:e>${mr('X')}</m:e></m:sPre>`,
    tokens: ['a', 'b', 'X'],
    mathml: ['<mmultiscripts>'],
    latex: null, // 子集外
  },
  {
    name: 'm:d 缺省括号',
    omml: `<m:d><m:e>${mr('x')}</m:e></m:d>`,
    tokens: ['x'],
    mathml: ['<mo stretchy="true">(</mo>'],
    latex: '\\left( x \\right)',
  },
  {
    name: 'm:d 方括号 begChr/endChr',
    omml: `<m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/></m:dPr><m:e>${mr('x')}</m:e></m:d>`,
    tokens: ['x'],
    mathml: ['<mo stretchy="true">[</mo>'],
    latex: '\\left[ x \\right]',
  },
  {
    name: 'm:d 多个 m:e 带 sepChr',
    omml: `<m:d><m:dPr><m:sepChr m:val=","/></m:dPr><m:e>${mr('x')}</m:e><m:e>${mr('y')}</m:e></m:d>`,
    tokens: ['x', 'y'],
    mathml: ['<mo>,</mo>'],
    latex: null, // 多槽 m:d 子集外
  },
  {
    name: 'm:nary ∑ undOvr',
    omml:
      `<m:nary><m:naryPr><m:chr m:val="∑"/><m:limLoc m:val="undOvr"/></m:naryPr>` +
      `<m:sub>${mr('k=0')}</m:sub><m:sup>${mr('n')}</m:sup><m:e>${mr('k')}</m:e></m:nary>`,
    tokens: ['k=0', 'n', 'k'],
    mathml: ['<munderover>'],
    latex: '\\sum_{k=0}^{n}',
  },
  {
    name: 'm:nary ∫ subSup',
    omml:
      `<m:nary><m:naryPr><m:chr m:val="∫"/><m:limLoc m:val="subSup"/></m:naryPr>` +
      `<m:sub>${mr('0')}</m:sub><m:sup>${mr('1')}</m:sup><m:e>${mr('x')}</m:e></m:nary>`,
    tokens: ['0', '1', 'x'],
    mathml: ['<msubsup>'],
    latex: '\\int_{0}^{1}',
  },
  {
    name: 'm:nary supHide',
    omml:
      `<m:nary><m:naryPr><m:chr m:val="∑"/><m:supHide m:val="1"/></m:naryPr>` +
      `<m:sub>${mr('k=0')}</m:sub><m:sup>${mr('n')}</m:sup><m:e>${mr('k')}</m:e></m:nary>`,
    tokens: ['k=0', 'n', 'k'],
    mathml: ['<munder>'],
    latex: '\\sum_{k=0}',
    latexNot: '^{',
  },
  {
    name: 'm:func sin',
    omml: `<m:func><m:fName>${mr('sin')}</m:fName><m:e>${mr('x')}</m:e></m:func>`,
    tokens: ['sin', 'x'],
    mathml: ['⁡'], // 函数应用不可见字符 U+2061
    latex: '\\sin',
  },
  {
    name: 'm:func lim',
    omml: `<m:func><m:fName>${mr('lim')}</m:fName><m:e>${mr('x')}</m:e></m:func>`,
    tokens: ['lim', 'x'],
    mathml: ['⁡'],
    latex: '\\lim',
  },
  {
    name: 'm:limLow',
    omml: `<m:limLow><m:e>${mr('lim')}</m:e><m:lim>${mr('x→0')}</m:lim></m:limLow>`,
    tokens: ['lim', 'x→0'],
    mathml: ['<munder>'],
    latex: '\\lim_{',
  },
  {
    name: 'm:limUpp',
    omml: `<m:limUpp><m:e>${mr('max')}</m:e><m:lim>${mr('n')}</m:lim></m:limUpp>`,
    tokens: ['max', 'n'],
    mathml: ['<mover>'],
    latex: null, // 子集外
  },
  {
    name: 'm:acc 缺省 hat',
    omml: `<m:acc><m:e>${mr('x')}</m:e></m:acc>`,
    tokens: ['x'],
    mathml: ['<mover accent="true">'],
    latex: '\\hat{x}',
  },
  {
    name: 'm:bar 缺省下横线',
    omml: `<m:bar><m:e>${mr('z')}</m:e></m:bar>`,
    tokens: ['z'],
    mathml: ['<munder>'],
    latex: '\\underline{z}',
  },
  {
    name: 'm:box',
    omml: `<m:box><m:boxPr><m:opEmu m:val="1"/></m:boxPr><m:e>${mr('x')}</m:e></m:box>`,
    tokens: ['x'],
    mathml: ['<mi>x</mi>'],
    mathmlNot: ['<menclose'], // box 不进 MathML 子集，只透传内容
    latex: 'x',
  },
  {
    name: 'm:borderBox',
    omml: `<m:borderBox><m:borderBoxPr><m:hideTop m:val="1"/><m:strikeBLTR m:val="1"/></m:borderBoxPr><m:e>${mr('x')}</m:e></m:borderBox>`,
    tokens: ['x'],
    mathml: ['<mi>x</mi>'],
    mathmlNot: ['<menclose'],
    latex: 'x',
  },
  {
    name: 'm:groupChr 缺省下花括号',
    omml: `<m:groupChr><m:e>${mr('a+b')}</m:e></m:groupChr>`,
    tokens: ['a+b'],
    mathml: ['<munder>', '⏟'],
    latex: '\\underbrace',
  },
  {
    name: 'm:eqArr 方程组',
    omml: `<m:eqArr><m:e>${mr('x=1')}</m:e><m:e>${mr('y=2')}</m:e></m:eqArr>`,
    tokens: ['x=1', 'y=2'],
    mathml: ['<mtable>'],
    latex: null, // 顶层 eqArr 子集外
  },
  {
    name: 'm:m 2×2 矩阵',
    omml:
      `<m:m><m:mr><m:e>${mr('1')}</m:e><m:e>${mr('0')}</m:e></m:mr>` +
      `<m:mr><m:e>${mr('0')}</m:e><m:e>${mr('1')}</m:e></m:mr></m:m>`,
    tokens: ['1', '0', '0', '1'],
    mathml: ['<mtable>'],
    latex: '\\begin{matrix}',
  },
  {
    name: 'm:d 套 m:m（pmatrix）',
    omml:
      `<m:d><m:e><m:m><m:mr><m:e>${mr('1')}</m:e><m:e>${mr('0')}</m:e></m:mr>` +
      `<m:mr><m:e>${mr('0')}</m:e><m:e>${mr('1')}</m:e></m:mr></m:m></m:e></m:d>`,
    tokens: ['1', '0', '0', '1'],
    mathml: ['<mo stretchy="true">(</mo>', '<mtable>'],
    latex: '\\begin{pmatrix}',
  },
  {
    name: 'm:phant 幻影',
    omml: `<m:phant><m:phantPr><m:show m:val="0"/></m:phantPr><m:e>${mr('x')}</m:e></m:phant>`,
    tokens: ['x'],
    mathml: ['<mi>x</mi>'],
    mathmlNot: ['<mphantom'], // phant 不进 MathML 子集，只透传内容
    latex: 'x',
  },
]

describe('M6-B OMML 公式（B1–B7）', () => {
  for (const c of OMML_ELEMENTS) {
    it(`B1 纯公式段：${c.name}`, async () => {
      const { parsed } = await recordAndParse('m6-omml', { bodyXml: MP(c.omml) })
      const block = parsed.blocks[0]
      expect(block.type).toBe('passthrough')
      expect(block.label).toBe('Equation')
      const fd = block.formulaDisplay
      expect(fd).toBeDefined()
      expect(fd!.tokens).toEqual(c.tokens)
      for (const frag of c.mathml) expect(fd!.mathml).toContain(frag)
      for (const frag of c.mathmlNot ?? []) expect(fd!.mathml ?? '').not.toContain(frag)
      expect(fd!.omml).toContain('<m:oMath>')
      if (c.latex === null) expect(fd!.latex).toBeUndefined()
      else if (typeof c.latex === 'string') expect(fd!.latex).toContain(c.latex)
      if (c.latexNot !== undefined) expect(fd!.latex ?? '').not.toContain(c.latexNot)
    })
  }

  it('B2 oMathPara 包两个 oMath（带 oMathParaPr/jc），仍是 Equation 块', async () => {
    const bodyXml =
      '<w:p><m:oMathPara><m:oMathParaPr><m:jc m:val="center"/></m:oMathParaPr>' +
      `<m:oMath>${FRAC_AB}</m:oMath><m:oMath>${FRAC_CD}</m:oMath>` +
      '</m:oMathPara></w:p>'
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.type).toBe('passthrough')
    expect(block.label).toBe('Equation')
    const fd = block.formulaDisplay
    expect(fd?.tokens).toEqual(['a', 'b', 'c', 'd'])
    expect(fd?.mathml?.match(/<math display="block">/g)).toHaveLength(2)
    // ommlToLatex 只接受单个 oMath 片段，两个片段 latex 缺失
    expect(fd?.latex).toBeUndefined()
  })

  it('B3 oMathPara 与普通文字同段：mathml 缺失（TS 只给纯公式 2D）', async () => {
    const bodyXml =
      `<w:p><m:oMathPara><m:oMath>${FRAC_AB}</m:oMath></m:oMathPara>` +
      '<w:r><w:t xml:space="preserve"> 其中 a≠0</w:t></w:r></w:p>'
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.label).toBe('Equation')
    const fd = block.formulaDisplay
    expect(fd?.mathml).toBeUndefined()
    expect(fd?.tokens).toEqual(['a', 'b'])
    // 单 oMath 片段：latex 仍可反解（照录）
    expect(fd?.latex).toContain('\\frac')
  })

  it('B4 正文夹公式：see <oMath> here', async () => {
    const bodyXml =
      '<w:p><w:r><w:t xml:space="preserve">see </w:t></w:r>' +
      `<m:oMath>${FRAC_AB}</m:oMath>` +
      '<w:r><w:t xml:space="preserve"> here</w:t></w:r></w:p>'
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.type).toBe('paragraph')
    expect(block.runs?.map((r) => r.text)).toEqual(['see ', 'ab', ' here'])
    expect(block.runs?.[1].math?.omml).toBe(`<m:oMath>${FRAC_AB}</m:oMath>`)
    expect(block.formulaDisplay).toBeUndefined()
  })

  it('B4 一段两处公式', async () => {
    const bodyXml =
      '<w:p><w:r><w:t xml:space="preserve">A=</w:t></w:r>' +
      `<m:oMath>${FRAC_AB}</m:oMath>` +
      '<w:r><w:t xml:space="preserve">, B=</w:t></w:r>' +
      `<m:oMath>${FRAC_CD}</m:oMath></w:p>`
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.type).toBe('paragraph')
    expect(block.runs?.map((r) => r.text)).toEqual(['A=', 'ab', ', B=', 'cd'])
    expect(block.runs?.[1].math?.omml).toContain('<m:num>')
    expect(block.runs?.[3].math?.omml).toContain('<m:den>')
  })

  it('B4 公式在 w:hyperlink 里', async () => {
    const bodyXml =
      '<w:p><w:hyperlink r:id="rId20"><w:r><w:t>推导见</w:t></w:r>' +
      `<m:oMath>${FRAC_AB}</m:oMath></w:hyperlink></w:p>`
    const { parsed } = await recordAndParse('m6-omml', { bodyXml, extraRels: HYPERLINK_REL })
    const block = parsed.blocks[0]
    expect(block.type).toBe('paragraph')
    expect(block.runs?.[0].text).toBe('推导见')
    expect(block.runs?.[0].link?.href).toBe('https://example.com/')
    expect(block.runs?.[1].math?.omml).toBe(`<m:oMath>${FRAC_AB}</m:oMath>`)
    // 照录：数学 run 不带 link（walk 的 m:oMath 分支不挂超链接）
    expect(block.runs?.[1].link).toBeUndefined()
  })

  it('B4 公式在表格单元格里', async () => {
    const tbl =
      '<w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc>' +
      `<w:p><w:r><w:t>cell</w:t></w:r><m:oMath>${FRAC_AB}</m:oMath></w:p>` +
      '</w:tc></w:tr></w:tbl>'
    const { parsed } = await recordAndParse('m6-omml', { bodyXml: tbl })
    const block = parsed.blocks[0]
    expect(block.type).toBe('table')
    const cell = block.table!.rows[0][0]
    // textOf 递归收集，cell.paras 含公式字符
    expect(cell.paras[0]).toBe('cellab')
    // 疑似 TS 缺陷：extractRuns 在单元格里拿不到 mathFragments，m:oMath 整个从 richParas 丢弃
    expect(cell.richParas?.[0].runs.map((r) => r.text)).toEqual(['cell'])
    expect(cell.richParas?.[0].runs.some((r) => r.math)).toBe(false)
  })

  it('B5 Word 风格属性包：oMathParaPr + ctrlPr 里的 w:rPr', async () => {
    const bodyXml =
      '<w:p><m:oMathPara><m:oMathParaPr><m:jc m:val="center"/></m:oMathParaPr><m:oMath>' +
      '<m:sSup><m:sSupPr><m:ctrlPr><w:rPr><w:b/><w:sz w:val="36"/></w:rPr></m:ctrlPr></m:sSupPr>' +
      `<m:e>${mr('x')}</m:e><m:sup>${mr('2')}</m:sup></m:sSup>` +
      '</m:oMath></m:oMathPara></w:p>'
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const fd = parsed.blocks[0].formulaDisplay
    expect(parsed.blocks[0].label).toBe('Equation')
    expect(fd?.tokens).toEqual(['x', '2'])
    expect(fd?.mathml).toContain('<msup>')
  })

  it('B5 m:rPr/m:sty val="p" + w:rPr；m:t xml:space="preserve" 带空格', async () => {
    const bodyXml = MP(
      '<m:r><m:rPr><m:sty m:val="p"/></m:rPr><w:rPr><w:b/></w:rPr><m:t>sin</m:t></m:r>' +
        '<m:r><m:t xml:space="preserve"> x </m:t></m:r>',
    )
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const fd = parsed.blocks[0].formulaDisplay
    expect(fd?.tokens).toEqual(['sin', ' x '])
    // sty="p" 的 run 整段一个 mi，不按字符分类
    expect(fd?.mathml).toContain('<mi>sin</mi>')
  })

  it('B5 m:t 里的实体 &lt; &amp; 解码', async () => {
    const { parsed } = await recordAndParse('m6-omml', {
      bodyXml: MP('<m:r><m:t>a&lt;b&amp;c</m:t></m:r>'),
    })
    const fd = parsed.blocks[0].formulaDisplay
    expect(fd?.tokens).toEqual(['a<b&c'])
    expect(fd?.mathml).toContain('<mo>&lt;</mo>')
    expect(fd?.mathml).toContain('<mo>&amp;</mo>')
    expect(fd?.latex).toContain('\\&')
  })

  it('B6 运算符与符号 ±×÷≤≥≠→∞∂∇ 的 mo 分类', async () => {
    const { parsed } = await recordAndParse('m6-omml', {
      bodyXml: MP('<m:r><m:t>±×÷≤≥≠→∞∂∇</m:t></m:r>'),
    })
    const fd = parsed.blocks[0].formulaDisplay
    expect(fd?.tokens).toEqual(['±×÷≤≥≠→∞∂∇'])
    expect(fd?.mathml?.match(/<mo>/g)).toHaveLength(10)
  })

  it('B6 希腊字母 mi 分类；∑ 落 mtext', async () => {
    const { parsed } = await recordAndParse('m6-omml', {
      bodyXml: MP('<m:r><m:t>αβγδεπσφω</m:t></m:r><m:r><m:t>∑</m:t></m:r>'),
    })
    const fd = parsed.blocks[0].formulaDisplay
    expect(fd?.tokens).toEqual(['αβγδεπσφω', '∑'])
    expect(fd?.mathml?.match(/<mi>/g)).toHaveLength(9)
    expect(fd?.mathml).toContain('<mtext>∑</mtext>')
  })

  it('B6 m:r 里多字符混合 2x+1 的 mn/mi/mo 分类', async () => {
    const { parsed } = await recordAndParse('m6-omml', { bodyXml: MP(mr('2x+1')) })
    const mathml = parsed.blocks[0].formulaDisplay?.mathml ?? ''
    expect(mathml).toContain('<mn>2</mn>')
    expect(mathml).toContain('<mi>x</mi>')
    expect(mathml).toContain('<mo>+</mo>')
    expect(mathml).toContain('<mn>1</mn>')
  })

  it('B7 公式段带 w:pPr（居中、样式）', async () => {
    const bodyXml =
      '<w:p><w:pPr><w:pStyle w:val="Normal"/><w:jc w:val="center"/></w:pPr>' +
      `<m:oMath>${FRAC_AB}</m:oMath></w:p>`
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.label).toBe('Equation')
    expect(block.formulaDisplay?.tokens).toEqual(['a', 'b'])
    expect(block.originalXml).toContain('<w:jc w:val="center"/>')
  })

  it('B7 公式段带书签与批注范围标记', async () => {
    const bodyXml =
      '<w:p><w:bookmarkStart w:id="1" w:name="eq1"/>' +
      `<m:oMath>${FRAC_AB}</m:oMath>` +
      '<w:bookmarkEnd w:id="1"/><w:commentRangeStart w:id="5"/><w:commentRangeEnd w:id="5"/></w:p>'
    const { parsed } = await recordAndParse('m6-omml', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.label).toBe('Equation')
    expect(block.formulaDisplay?.tokens).toEqual(['a', 'b'])
    expect(block.originalXml).toContain('w:bookmarkStart')
  })
})

// ---------------------------------------------------------------- B8 ruby

/** 一个 w:ruby 片段；prExtra 是 w:rubyPr 的子元素 */
function rubyXml(base: string, rt: string, prExtra = ''): string {
  return (
    `<w:ruby><w:rubyPr>${prExtra}</w:rubyPr>` +
    `<w:rt><w:r><w:t>${rt}</w:t></w:r></w:rt>` +
    `<w:rubyBase><w:r><w:t>${base}</w:t></w:r></w:rubyBase></w:ruby>`
  )
}

describe('M6-B ruby（B8）', () => {
  it('B8 rubyAlign 各值 + hps/hpsRaise/hpsBaseText/lid 属性包', async () => {
    const aligns = ['center', 'distributeLetter', 'distributeSpace', 'left', 'right', 'rightVertical']
    const rubies = aligns.map((al, i) =>
      rubyXml(
        `基${i}`,
        `on${i}`,
        `<w:rubyAlign w:val="${al}"/><w:hps w:val="${10 + i}"/><w:hpsRaise w:val="${20 + i}"/>` +
          `<w:hpsBaseText w:val="${22 + i}"/><w:lid w:val="zh-CN"/>`,
      ),
    )
    const bodyXml = `<w:p>${rubies.map((r) => `<w:r>${r}</w:r>`).join('')}</w:p>`
    const { parsed } = await recordAndParse('m6-ruby', { bodyXml })
    const block = parsed.blocks[0]
    expect(block.type).toBe('paragraph')
    expect(block.runs).toHaveLength(6)
    for (const [i, run] of block.runs!.entries()) {
      expect(run.text).toBe(`基${i}`)
      expect(run.ruby?.rt).toBe(`on${i}`)
      expect(run.ruby?.xml).toBe(rubies[i])
      expect(run.ruby?.xml).toContain(`w:rubyAlign w:val="${aligns[i]}"`)
      expect(run.ruby?.xml).toContain(`w:hps w:val="${10 + i}"`)
    }
  })

  it('B8 rt 两个 run、rubyBase 两个 run 且带 w:rPr', async () => {
    const ruby =
      '<w:ruby><w:rubyPr><w:rubyAlign w:val="center"/><w:hps w:val="12"/></w:rubyPr>' +
      '<w:rt><w:r><w:rPr><w:sz w:val="12"/></w:rPr><w:t>gù</w:t></w:r><w:r><w:t>xiāng</w:t></w:r></w:rt>' +
      '<w:rubyBase><w:r><w:rPr><w:b/></w:rPr><w:t>故</w:t></w:r><w:r><w:t>乡</w:t></w:r></w:rubyBase></w:ruby>'
    const { parsed } = await recordAndParse('m6-ruby', { bodyXml: `<w:p><w:r>${ruby}</w:r></w:p>` })
    const run = parsed.blocks[0].runs?.[0]
    expect(run?.text).toBe('故乡')
    expect(run?.ruby?.rt).toBe('gùxiāng')
    expect(run?.ruby?.xml).toBe(ruby)
  })

  it('B8 ruby 前后有普通文字；一段三个 ruby', async () => {
    const bodyXml =
      '<w:p><w:r><w:t>床前</w:t></w:r>' +
      `<w:r>${rubyXml('明', 'míng')}</w:r>` +
      `<w:r>${rubyXml('月', 'yuè')}</w:r>` +
      `<w:r>${rubyXml('光', 'guāng')}</w:r>` +
      '<w:r><w:t>，</w:t></w:r></w:p>'
    const { parsed } = await recordAndParse('m6-ruby', { bodyXml })
    const runs = parsed.blocks[0].runs!
    expect(runs.map((r) => r.text)).toEqual(['床前', '明', '月', '光', '，'])
    expect(runs[0].ruby).toBeUndefined()
    expect(runs[1].ruby?.rt).toBe('míng')
    expect(runs[2].ruby?.rt).toBe('yuè')
    expect(runs[3].ruby?.rt).toBe('guāng')
    expect(runs[4].ruby).toBeUndefined()
  })

  it('B8 ruby 在 w:hyperlink 里', async () => {
    const ruby = rubyXml('床', 'chuáng')
    const bodyXml = `<w:p><w:hyperlink r:id="rId20"><w:r>${ruby}</w:r></w:hyperlink></w:p>`
    const { parsed } = await recordAndParse('m6-ruby', { bodyXml, extraRels: HYPERLINK_REL })
    const run = parsed.blocks[0].runs?.[0]
    expect(run?.text).toBe('床')
    expect(run?.ruby?.rt).toBe('chuáng')
    // 照录：ruby run 不带 link（handleRun 的 ruby 分支不挂超链接）
    expect(run?.link).toBeUndefined()
  })

  it('B8 ruby 在表格单元格里', async () => {
    const tbl =
      '<w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc>' +
      `<w:p><w:r>${rubyXml('床', 'chuáng')}</w:r></w:p>` +
      '</w:tc></w:tr></w:tbl>'
    const { parsed } = await recordAndParse('m6-ruby', { bodyXml: tbl })
    const cell = parsed.blocks[0].table!.rows[0][0]
    // textOf 递归收集：cell.paras 把注音和基底字拼在一起
    expect(cell.paras[0]).toBe('chuáng床')
    // 疑似 TS 缺陷：单元格 extractRuns 拿不到 rubyFragments，注音信息丢失只剩基底字
    expect(cell.richParas?.[0].runs[0].text).toBe('床')
    expect(cell.richParas?.[0].runs[0].ruby).toBeUndefined()
  })
})

// ---------------------------------------------------------------- B9 OLE

const V_NS = 'urn:schemas-microsoft-com:vml'
const O_NS = 'urn:schemas-microsoft-com:office:office'
const IMAGE_REL_TYPE = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/image'

/** 一个 w:object（OLE）run 内容：VML 预览图 + o:OLEObject */
function oleObjectXml(opts: {
  style?: string | null
  type?: string
  objAttrs?: string
  oleAttrs?: string
  rId?: string
} = {}): string {
  const style = opts.style === null ? '' : ` style="${opts.style ?? 'width:32pt;height:32pt'}"`
  return (
    `<w:object${opts.objAttrs ?? ''}>` +
    `<v:shape xmlns:v="${V_NS}" id="_x0000_i1025"${style}>` +
    `<v:imagedata r:id="${opts.rId ?? 'rId10'}" o:title=""/></v:shape>` +
    `<o:OLEObject xmlns:o="${O_NS}" Type="${opts.type ?? 'Embed'}" ProgID="Excel.Sheet.12" ` +
    `ShapeID="_x0000_i1025" DrawAspect="Content" ObjectID="_1"${opts.oleAttrs ?? ''}/>` +
    '</w:object>'
  )
}

describe('M6-B OLE w:object（B9）', () => {
  it('B9 一段两个 w:object（无文字）→ Embedded object 芯片', async () => {
    const { parsed } = await recordAndParse('m6-ole', {
      bodyXml: `<w:p><w:r>${oleObjectXml()}</w:r><w:r>${oleObjectXml()}</w:r></w:p>`,
      withImage: true,
    })
    const block = parsed.blocks[0]
    expect(block.type).toBe('passthrough')
    expect(block.label).toBe('Embedded object')
    expect(block.oleProgId).toBe('Excel.Sheet.12')
    expect(block.imageDataUrl).toMatch(/^data:image\/png;base64,/)
    // 预览尺寸来自 v:shape style 的 pt：32pt → 43px
    expect(block.imageWidthPx).toBe(43)
    expect(block.imageHeightPx).toBe(43)
    expect(block.originalXml!.match(/<w:object>/g)).toHaveLength(2)
  })

  it('B9 w:object 所在 run 带 w:rPr（颜色 / 加粗）且段里有文字 → run 级图片', async () => {
    const bodyXml =
      '<w:p><w:r><w:rPr><w:color w:val="0000FF"/></w:rPr><w:t xml:space="preserve">公告 </w:t></w:r>' +
      `<w:r><w:rPr><w:b/><w:color w:val="FF0000"/></w:rPr>${oleObjectXml()}</w:r></w:p>`
    const { parsed } = await recordAndParse('m6-ole', { bodyXml, withImage: true })
    const block = parsed.blocks[0]
    expect(block.type).toBe('paragraph')
    expect(block.runs?.[0].color).toBe('0000FF')
    const imgRun = block.runs?.find((r) => r.image)
    expect(imgRun?.bold).toBe(true)
    expect(imgRun?.image?.dataUrl).toMatch(/^data:image\/png;base64,/)
    expect(imgRun?.image?.xml).toContain('<o:OLEObject')
    expect(imgRun?.image?.widthPx).toBe(43)
  })

  it('B9 Link 型 OLEObject（r:id 外部关系 + UpdateMode）', async () => {
    const bodyXml = `<w:p><w:r>${oleObjectXml({
      type: 'Link',
      oleAttrs: ' r:id="rId50" UpdateMode="OnCall"',
    })}</w:r></w:p>`
    const { parsed } = await recordAndParse('m6-ole', {
      bodyXml,
      withImage: true,
      extraRels:
        '<Relationship Id="rId50" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject" Target="file:///C:\\fake\\book.xlsx" TargetMode="External"/>',
    })
    const block = parsed.blocks[0]
    expect(block.type).toBe('passthrough')
    expect(block.label).toBe('Embedded object')
    expect(block.oleProgId).toBe('Excel.Sheet.12')
    expect(block.originalXml).toContain('Type="Link"')
    expect(block.originalXml).toContain('UpdateMode="OnCall"')
    // 预览图仍在文档内（v:imagedata rId10）
    expect(block.imageDataUrl).toMatch(/^data:image\/png;base64,/)
  })

  it('B9 v:shape 无 style：回退 w:object dxaOrig/dyaOrig（twips）', async () => {
    const { parsed } = await recordAndParse('m6-ole', {
      bodyXml: `<w:p><w:r>${oleObjectXml({ style: null, objAttrs: ' w:dxaOrig="1365" w:dyaOrig="765"' })}</w:r></w:p>`,
      withImage: true,
    })
    const block = parsed.blocks[0]
    expect(block.label).toBe('Embedded object')
    // 1365/15 = 91，765/15 = 51
    expect(block.imageWidthPx).toBe(91)
    expect(block.imageHeightPx).toBe(51)
  })

  it('B9 预览 r:id 悬空 + 段落有文字：保字节不保预览', async () => {
    const bodyXml =
      '<w:p><w:r><w:t xml:space="preserve">对象失效: </w:t></w:r>' +
      `<w:r>${oleObjectXml({ rId: 'rIdGone' })}</w:r></w:p>`
    const { parsed } = await recordAndParse('m6-ole', { bodyXml, withImage: true })
    const block = parsed.blocks[0]
    expect(block.type).toBe('passthrough')
    expect(block.label).toBe('Embedded object')
    expect(block.previewText).toContain('对象失效')
    expect(block.imageDataUrl).toBeUndefined()
    expect(block.originalXml).toContain('<o:OLEObject')
  })

  it('B9 w:object 在表格单元格里且格里有文字', async () => {
    const tbl =
      '<w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc>' +
      `<w:p><w:r><w:t>格内 </w:t></w:r><w:r>${oleObjectXml()}</w:r></w:p>` +
      '</w:tc></w:tr></w:tbl>'
    const { parsed } = await recordAndParse('m6-ole', { bodyXml: tbl, withImage: true })
    const block = parsed.blocks[0]
    expect(block.type).toBe('table')
    const cell = block.table!.rows[0][0]
    expect(cell.paras[0]).toBe('格内 ')
    const imgRun = cell.richParas?.[0].runs.find((r) => r.image)
    expect(imgRun?.image?.xml).toContain('<o:OLEObject')
    expect(imgRun?.image?.dataUrl).toMatch(/^data:image\/png;base64,/)
    expect(imgRun?.image?.widthPx).toBe(43)
  })

  it('B9 EMBED 字段包着 w:object 且字段后还有文字', async () => {
    const bodyXml =
      '<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>' +
      '<w:r><w:instrText xml:space="preserve"> EMBED Excel.Sheet.12 </w:instrText></w:r>' +
      '<w:r><w:fldChar w:fldCharType="separate"/></w:r>' +
      `<w:r>${oleObjectXml()}</w:r>` +
      '<w:r><w:fldChar w:fldCharType="end"/></w:r>' +
      '<w:r><w:t xml:space="preserve"> 字段后的文字</w:t></w:r></w:p>'
    const { parsed } = await recordAndParse('m6-ole', { bodyXml, withImage: true })
    const block = parsed.blocks[0]
    expect(block.type).toBe('passthrough')
    expect(block.label).toBe('Embedded object')
    expect(block.oleProgId).toBe('Excel.Sheet.12')
    expect(block.previewText).toContain('字段后的文字')
    expect(block.imageDataUrl).toMatch(/^data:image\/png;base64,/)
  })

  it('B9 w:object 与 w:drawing 图片同段 → 两个 run 级图片', async () => {
    const drawingRun =
      '<w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/>' +
      '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">' +
      '<pic:pic><pic:blipFill><a:blip r:embed="rId11"/></pic:blipFill></pic:pic>' +
      '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>'
    const { parsed } = await recordAndParse('m6-ole', {
      bodyXml: `<w:p><w:r>${oleObjectXml()}</w:r>${drawingRun}</w:p>`,
      extraRels:
        `<Relationship Id="rId10" Type="${IMAGE_REL_TYPE}" Target="media/image1.png"/>` +
        `<Relationship Id="rId11" Type="${IMAGE_REL_TYPE}" Target="media/image2.png"/>`,
      binaryParts: [
        { path: 'word/media/image1.png', base64: real.TINY_PNG_BASE64, extension: 'png', contentType: 'image/png' },
        { path: 'word/media/image2.png', base64: real.TINY_PNG_BASE64, extension: 'png', contentType: 'image/png' },
      ],
    })
    const block = parsed.blocks[0]
    expect(block.type).toBe('paragraph')
    const imgRuns = (block.runs ?? []).filter((r) => r.image)
    expect(imgRuns).toHaveLength(2)
    expect(imgRuns[0].image?.xml).toContain('<o:OLEObject')
    expect(imgRuns[1].image?.xml).toContain('<pic:pic')
    expect(imgRuns[1].image?.widthPx).toBe(96) // wp:extent 914400 EMU = 1 英寸 = 96px
  })
})

// ---------------------------------------------------------------- B10–B11 墨迹

const TWO_PARAS = P('第一段') + P('第二段')
const THREE_PARAS = P('第一段') + P('第二段') + P('第三段')

function makeInk(blockIndex: number, extra: Partial<NewInkImage> = {}): NewInkImage {
  return {
    blockIndex,
    base64: real.TINY_PNG_BASE64,
    widthPx: 200,
    heightPx: 80,
    offsetXPx: 40,
    offsetYPx: -10,
    payload: '{"strokes":[{"tool":"pen","color":"C00000"}]}',
    ...extra,
  }
}

/** 保存一份"第 2 段带一条墨迹"的产物并录制，供 B11 二次编辑 */
async function inkedTwoParas(): Promise<{ saved1: Uint8Array; parsed1: Parsed }> {
  const { parsed } = await recordAndParse('m6-ink', { bodyXml: TWO_PARAS })
  const saved1 = await saveDocx(parsed, asOriginal(parsed), { inks: [makeInk(1)] })
  // 产物先录制成解析语料：二次保存的 .save.k.json 才能按字节哈希挂到它的 stem
  const parsed1 = await recordOutput('m6-ink', saved1)
  return { saved1, parsed1 }
}

describe('M6-B 墨迹（B10–B11，解析侧 golden 只能经 TS 保存产生）', () => {
  it('B10 一条墨迹锚在第 2 段', async () => {
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: TWO_PARAS })
    const saved = await saveDocx(parsed, asOriginal(parsed), { inks: [makeInk(1)] })
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(1)
    expect(out.inks[0].payload).toBe('{"strokes":[{"tool":"pen","color":"C00000"}]}')
    expect(out.inks[0].anchorIndex).toBe(1)
    expect(out.inks[0].offsetXPx).toBeCloseTo(40, 1)
    expect(out.inks[0].offsetYPx).toBeCloseTo(-10, 1)
    expect(out.inks[0].dataUrl).toContain('data:image/png;base64,')
    const anchor = out.blocks.find((b) => b.docxIndex === out.inks[0].anchorIndex)
    expect(anchor?.type).toBe('paragraph')
    expect(anchor?.runs?.map((r) => r.text).join('')).toBe('第二段')
  })

  it('B10 两条墨迹锚同一段', async () => {
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: TWO_PARAS })
    const saved = await saveDocx(parsed, asOriginal(parsed), {
      inks: [makeInk(1), makeInk(1, { offsetXPx: 90, payload: '{"strokes":[]}' })],
    })
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(2)
    expect(out.inks.map((i) => i.payload)).toEqual([
      '{"strokes":[{"tool":"pen","color":"C00000"}]}',
      '{"strokes":[]}',
    ])
    expect(out.inks[1].offsetXPx).toBeCloseTo(90, 1)
  })

  it('B10 两条墨迹锚不同段', async () => {
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: THREE_PARAS })
    const saved = await saveDocx(parsed, asOriginal(parsed), {
      inks: [makeInk(0), makeInk(2, { payload: '{"strokes":[{"tool":"hl"}]}' })],
    })
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(2)
    expect(out.inks.map((i) => i.anchorIndex)).toEqual([0, 2])
    for (const ink of out.inks) {
      const anchor = out.blocks.find((b) => b.docxIndex === ink.anchorIndex)
      expect(anchor?.type).toBe('paragraph')
    }
  })

  it('B10 负偏移', async () => {
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: TWO_PARAS })
    const saved = await saveDocx(parsed, asOriginal(parsed), {
      inks: [makeInk(0, { offsetXPx: -50, offsetYPx: -30 })],
    })
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(1)
    expect(out.inks[0].offsetXPx).toBeCloseTo(-50, 1)
    expect(out.inks[0].offsetYPx).toBeCloseTo(-30, 1)
  })

  it('B10 payload 含引号 / & / 中文 / 换行', async () => {
    const payload = '{"strokes":[{"label":"引号"与&符"},{"备注":"中文\n换行"}]}'
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: TWO_PARAS })
    const saved = await saveDocx(parsed, asOriginal(parsed), { inks: [makeInk(1, { payload })] })
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(1)
    expect(out.inks[0].payload).toBe(payload)
  })

  it('B10 空段（自闭合 <w:p/>）为锚', async () => {
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: '<w:p/>' + P('尾段') })
    const saved = await saveDocx(parsed, asOriginal(parsed), { inks: [makeInk(0)] })
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(1)
    expect(out.inks[0].anchorIndex).toBe(0)
    expect(out.blocks[0].type).toBe('paragraph')
    // 自闭合被展开注入，不再是 <w:p/>
    expect(await documentXmlOf(saved)).not.toContain('<w:p/>')
  })

  it('B10 锚点是表格块：应被跳过且无孤儿媒体', async () => {
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: real.TABLE_XML })
    const saved = await saveDocx(parsed, asOriginal(parsed), { inks: [makeInk(0)] })
    expect(await documentXmlOf(saved)).not.toContain('aidocs-ink')
    expect((await mediaPaths(saved)).some((p) => p.includes('aidocsink'))).toBe(false)
    expect(await documentRelsOf(saved)).not.toContain('aidocsink')
    const out = await recordOutput('m6-ink', saved)
    expect(out.inks).toHaveLength(0)
  })

  it('B10 锚点段在 w:sdt 里：originalXml 以 <w:sdt 开头，墨迹注入被跳过（照录）', async () => {
    const sdt =
      '<w:sdt><w:sdtPr><w:alias w:val="墨迹区"/><w:tag w:val="inkzone"/></w:sdtPr>' +
      '<w:sdtContent><w:p><w:r><w:t>sdt内段落</w:t></w:r></w:p></w:sdtContent></w:sdt>'
    const { parsed } = await recordAndParse('m6-ink', { bodyXml: sdt })
    expect(parsed.blocks[0].sdtShell).toBeDefined()
    const saved = await saveDocx(parsed, asOriginal(parsed), { inks: [makeInk(0)] })
    const out = await recordOutput('m6-ink', saved)
    // 疑似 TS 缺陷：锚在 sdt 内段落时墨迹被静默丢弃（inject 只认 <w:p 开头的片段）
    expect(out.inks).toHaveLength(0)
    expect(await documentXmlOf(saved)).not.toContain('aidocs-ink')
  })

  it('B11 二次保存 inks: [] 清除墨迹，媒体与关系不残留', async () => {
    const { parsed1 } = await inkedTwoParas()
    const saved2 = await saveDocx(parsed1, asOriginal(parsed1), { inks: [] })
    const out = await recordOutput('m6-ink', saved2)
    expect(out.inks).toHaveLength(0)
    expect(await documentXmlOf(saved2)).not.toContain('aidocs-ink')
    expect((await mediaPaths(saved2)).some((p) => p.includes('aidocsink'))).toBe(false)
    expect(await documentRelsOf(saved2)).not.toContain('aidocsink')
  })

  it('B11 二次保存：同一条墨迹改锚点（第 2 段 → 第 1 段）', async () => {
    const { parsed1 } = await inkedTwoParas()
    const saved2 = await saveDocx(parsed1, asOriginal(parsed1), { inks: [makeInk(0)] })
    const out = await recordOutput('m6-ink', saved2)
    expect(out.inks).toHaveLength(1)
    expect(out.inks[0].anchorIndex).toBe(0)
    expect(out.inks[0].payload).toBe('{"strokes":[{"tool":"pen","color":"C00000"}]}')
    // 媒体不累积：旧 aidocsink part 被回收，只留新的一份
    expect((await mediaPaths(saved2)).filter((p) => p.includes('aidocsink'))).toHaveLength(1)
  })

  it('B11 二次保存：再加一条墨迹', async () => {
    const { parsed1 } = await inkedTwoParas()
    const saved2 = await saveDocx(parsed1, asOriginal(parsed1), {
      inks: [makeInk(1), makeInk(1, { offsetXPx: 120, payload: '{"strokes":[{"tool":"hl"}]}' })],
    })
    const out = await recordOutput('m6-ink', saved2)
    expect(out.inks).toHaveLength(2)
    expect(out.inks.map((i) => i.payload)).toEqual([
      '{"strokes":[{"tool":"pen","color":"C00000"}]}',
      '{"strokes":[{"tool":"hl"}]}',
    ])
    const inkMedia = (await mediaPaths(saved2)).filter((p) => p.includes('aidocsink'))
    expect(inkMedia).toHaveLength(2)
    const rels = await documentRelsOf(saved2)
    expect(rels.match(/aidocsink\d+\.png/g)).toHaveLength(2)
  })

  it('B11 二次保存：inks 不传 = no-op，字节与源一致', async () => {
    const { saved1, parsed1 } = await inkedTwoParas()
    const saved2 = await saveDocx(parsed1, asOriginal(parsed1))
    expect(saved2).toEqual(saved1)
    // no-op 产物与源同字节：record 记 duplicate_of，不重复落盘
    await recordOutput('m6-ink', saved2)
  })
})

// ---------------------------------------------------------------- B12–B14 新图片与 replaceImage

/** 1×1 GIF（与 TINY_PNG 不同字节，便于区分新旧媒体 part） */
const TINY_GIF_BASE64 = 'R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw=='

function img(extra: Partial<NewImage> = {}): NewImage {
  return {
    base64: real.TINY_PNG_BASE64,
    mime: 'image/png',
    widthPx: 64,
    heightPx: 32,
    ...extra,
  }
}

/** 源文档：一段文字（文字因用例而异，避免与别的源撞哈希） */
async function textSource(tag: string): Promise<Parsed> {
  const { parsed } = await recordAndParse('m6-image', { bodyXml: P(`图片用例 ${tag}`) })
  return parsed
}

/** 一个带 a:srcRect 裁剪的内嵌图片段 */
const CROPPED_IMAGE_P =
  '<w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/>' +
  '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">' +
  '<pic:pic><pic:blipFill><a:blip r:embed="rId10"/>' +
  '<a:srcRect l="10000" t="20000" r="30000" b="40000"/>' +
  '<a:stretch><a:fillRect/></a:stretch></pic:blipFill></pic:pic>' +
  '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'

const LINKED_IMAGE_URL = 'https://example.com/m6/pic.png'
/** r:link 外链图片段 */
const LINKED_IMAGE_P =
  '<w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/>' +
  '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">' +
  '<pic:pic><pic:blipFill><a:blip r:link="rId30"/></pic:blipFill></pic:pic>' +
  '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'

/** 带 asvg:svgBlip 扩展的图片段（png 回退 + svg 首选） */
const SVG_IMAGE_P =
  '<w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/>' +
  '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">' +
  '<pic:pic><pic:blipFill><a:blip r:embed="rId10">' +
  '<a:extLst><a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}">' +
  '<asvg:svgBlip xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main" r:embed="rId11"/>' +
  '</a:ext></a:extLst></a:blip>' +
  '<a:stretch><a:fillRect/></a:stretch></pic:blipFill></pic:pic>' +
  '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'

describe('M6-B 新图片保存（B12）', () => {
  it('B12 mime 三种：png / jpeg / gif', async () => {
    const parsed = await textSource('mime')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      { kind: 'image', image: img({ mime: 'image/png' }) },
      { kind: 'image', image: img({ mime: 'image/jpeg' }) },
      { kind: 'image', image: img({ mime: 'image/gif' }) },
    ])
    const out = await recordOutput('m6-image', saved)
    const images = out.blocks.filter((b) => b.type === 'image')
    expect(images).toHaveLength(3)
    expect(images[0].imageDataUrl).toMatch(/^data:image\/png;base64,/)
    expect(images[1].imageDataUrl).toMatch(/^data:image\/jpeg;base64,/)
    expect(images[2].imageDataUrl).toMatch(/^data:image\/gif;base64,/)
    expect(await mediaPaths(saved)).toEqual([
      'word/media/aidocs1.png',
      'word/media/aidocs2.jpg',
      'word/media/aidocs3.gif',
    ])
  })

  it('B12 align 三种', async () => {
    const parsed = await textSource('align')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      { kind: 'image', image: img({ align: 'left' }) },
      { kind: 'image', image: img({ align: 'center' }) },
      { kind: 'image', image: img({ align: 'right' }) },
    ])
    const out = await recordOutput('m6-image', saved)
    const images = out.blocks.filter((b) => b.type === 'image')
    expect(images).toHaveLength(3)
    // left 不写 w:jc（缺省即左），解析回 undefined
    expect(images.map((b) => b.imageAlign)).toEqual([undefined, 'center', 'right'])
  })

  it('B12 wrap 九种', async () => {
    const wraps: ImageWrap[] = [
      'square-left',
      'square-right',
      'tight-left',
      'tight-right',
      'through-left',
      'through-right',
      'topBottom',
      'behind',
      'front',
    ]
    const parsed = await textSource('wrap')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      ...wraps.map((wrap) => ({ kind: 'image' as const, image: img({ wrap }) })),
    ])
    const out = await recordOutput('m6-image', saved)
    const images = out.blocks.filter((b) => b.type === 'image')
    expect(images).toHaveLength(9)
    // 疑似 TS 缺陷：新建锚定图片时 tight-*/through-* 落盘成 wrapSquare（applyImageWrap
    // 只在保留原有 wrap 字节时才写 wrapTight/wrapThrough），再解析读回 square-*
    expect(images.map((b) => b.imageWrap)).toEqual([
      'square-left',
      'square-right',
      'square-left',
      'square-right',
      'square-left',
      'square-right',
      'topBottom',
      'behind',
      'front',
    ])
    expect(await documentXmlOf(saved)).not.toContain('wrapTight')
    expect(await documentXmlOf(saved)).not.toContain('wrapThrough')
  })

  it('B12 posOffsetEmu 与 zOrder', async () => {
    const parsed = await textSource('posOffset+zOrder')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      { kind: 'image', image: img({ wrap: 'square-left', posOffsetEmu: { x: 100000, y: 200000 } }) },
      { kind: 'image', image: img({ wrap: 'square-right', zOrder: 5 }) },
    ])
    const out = await recordOutput('m6-image', saved)
    const images = out.blocks.filter((b) => b.type === 'image')
    expect(images).toHaveLength(2)
    expect(images[0].imageWrap).toBe('square-left')
    expect(images[0].imageOffsetXEmu).toBe(100000)
    expect(images[0].imageOffsetYEmu).toBe(200000)
    expect(images[1].imageZOrder).toBe(5)
  })

  it('B12 rotDeg 90 + flipH', async () => {
    const parsed = await textSource('rot+flip')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      { kind: 'image', image: img({ rotDeg: 90, flipH: true }) },
    ])
    const out = await recordOutput('m6-image', saved)
    const image = out.blocks.find((b) => b.type === 'image')
    expect(image?.imageRotDeg).toBe(90)
    expect(image?.imageFlipH).toBe(true)
    expect(image?.imageWrap).toBeUndefined() // inline
  })

  it('B12 paraSpacing 写进图片所在段的 w:pPr', async () => {
    const parsed = await textSource('paraSpacing')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      {
        kind: 'image',
        image: img({ paraSpacing: { beforeTwips: 240, afterTwips: 120, lineTwips: 360, lineRule: 'exact' } }),
      },
    ])
    const xml = await documentXmlOf(saved)
    expect(xml).toContain('w:before="240"')
    expect(xml).toContain('w:after="120"')
    expect(xml).toContain('w:line="360"')
    expect(xml).toContain('w:lineRule="exact"')
    const out = await recordOutput('m6-image', saved)
    expect(out.blocks.some((b) => b.type === 'image')).toBe(true)
  })

  it('B12 同一字节插两次：去重成一个媒体 part', async () => {
    const parsed = await textSource('dedup')
    const saved = await saveDocx(parsed, [
      ...asOriginal(parsed),
      { kind: 'image', image: img() },
      { kind: 'image', image: img() },
    ])
    const out = await recordOutput('m6-image', saved)
    const images = out.blocks.filter((b) => b.type === 'image')
    expect(images).toHaveLength(2)
    expect(await mediaPaths(saved)).toEqual(['word/media/aidocs1.png'])
    expect(images[0].imageDataUrl).toBe(images[1].imageDataUrl)
    const rels = await documentRelsOf(saved)
    expect(rels.match(/Target="media\/aidocs1\.png"/g)).toHaveLength(1)
  })
})

describe('M6-B replaceImage 与图片删除（B13–B14）', () => {
  it('B13 源图片带 a:srcRect 裁剪：替换后裁剪被丢弃、旧媒体回收', async () => {
    const { parsed } = await recordAndParse('m6-image', { bodyXml: CROPPED_IMAGE_P, withImage: true })
    const block = parsed.blocks[0]
    expect(block.type).toBe('image')
    expect(block.imageCrop).toEqual({ l: 0.1, t: 0.2, r: 0.3, b: 0.4 })
    const saved = await saveDocx(parsed, [
      {
        kind: 'xml',
        xml: block.originalXml!,
        docxIndex: block.docxIndex!,
        replaceImage: { base64: real.TINY_PNG_BASE64, mime: 'image/png' },
      },
    ])
    const out = await recordOutput('m6-image', saved)
    expect(out.blocks[0].type).toBe('image')
    expect(out.blocks[0].imageCrop).toBeUndefined()
    const xml = await documentXmlOf(saved)
    expect(xml).not.toContain('srcRect')
    const zip = await zipOf(saved)
    expect(zip.file('word/media/image1.png')).toBeNull()
    expect(zip.file('word/media/aidocs1.png')).not.toBeNull()
  })

  it('B13 源是 r:link 外链图：替换后变内嵌', async () => {
    const { parsed } = await recordAndParse('m6-image', {
      bodyXml: LINKED_IMAGE_P,
      extraRels: `<Relationship Id="rId30" Type="${IMAGE_REL_TYPE}" Target="${LINKED_IMAGE_URL}" TargetMode="External"/>`,
    })
    const block = parsed.blocks[0]
    expect(block.type).toBe('image')
    expect(block.imageDataUrl).toBe(LINKED_IMAGE_URL)
    const saved = await saveDocx(parsed, [
      {
        kind: 'xml',
        xml: block.originalXml!,
        docxIndex: block.docxIndex!,
        replaceImage: { base64: real.TINY_PNG_BASE64, mime: 'image/png' },
      },
    ])
    const out = await recordOutput('m6-image', saved)
    expect(out.blocks[0].type).toBe('image')
    expect(out.blocks[0].imageDataUrl).toMatch(/^data:image\/png;base64,/)
    expect(await documentXmlOf(saved)).not.toContain('r:link')
    expect(await documentRelsOf(saved)).not.toContain('example.com')
  })

  it('B13 源带 asvg:svgBlip 扩展：替换后扩展被丢弃', async () => {
    const { parsed } = await recordAndParse('m6-image', {
      bodyXml: SVG_IMAGE_P,
      withImage: true,
      extraRels: `<Relationship Id="rId11" Type="${IMAGE_REL_TYPE}" Target="media/image1.svg"/>`,
      binaryParts: [
        {
          path: 'word/media/image1.svg',
          base64: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>').toString('base64'),
          extension: 'svg',
          contentType: 'image/svg+xml',
        },
      ],
    })
    const block = parsed.blocks[0]
    expect(block.type).toBe('image')
    const saved = await saveDocx(parsed, [
      {
        kind: 'xml',
        xml: block.originalXml!,
        docxIndex: block.docxIndex!,
        replaceImage: { base64: real.TINY_PNG_BASE64, mime: 'image/png' },
      },
    ])
    const xml = await documentXmlOf(saved)
    expect(xml).not.toContain('asvg')
    expect(xml).not.toContain('extLst')
    const zip = await zipOf(saved)
    expect(zip.file('word/media/aidocs1.png')).not.toBeNull()
    // 旧 png 与 svg part 都不再被引用（照录 TS 的资源回收）
    expect(zip.file('word/media/image1.png')).toBeNull()
    expect(zip.file('word/media/image1.svg')).toBeNull()
    expect(await documentRelsOf(saved)).not.toContain('image1.svg')
    const out = await recordOutput('m6-image', saved)
    expect(out.blocks[0].imageDataUrl).toMatch(/^data:image\/png;base64,/)
  })

  it('B13 连续替换两次：第二次以第一次的产物为源', async () => {
    const { parsed } = await recordAndParse('m6-image', {
      bodyXml: real.IMAGE_PARAGRAPH_XML,
      withImage: true,
    })
    const block = parsed.blocks.find((b) => b.type === 'image')!
    const saved1 = await saveDocx(parsed, [
      {
        kind: 'xml',
        xml: block.originalXml!,
        docxIndex: block.docxIndex!,
        replaceImage: { base64: TINY_GIF_BASE64, mime: 'image/gif' },
      },
    ])
    expect(await mediaPaths(saved1)).toEqual(['word/media/aidocs1.gif'])
    const parsed1 = await recordOutput('m6-image', saved1)
    const block1 = parsed1.blocks.find((b) => b.type === 'image')!
    const saved2 = await saveDocx(parsed1, [
      {
        kind: 'xml',
        xml: block1.originalXml!,
        docxIndex: block1.docxIndex!,
        replaceImage: { base64: real.TINY_PNG_BASE64, mime: 'image/png' },
      },
    ])
    // 每一次替换只留最新一份媒体（与 resource-cleanup 测试同律）
    expect(await mediaPaths(saved2)).toEqual(['word/media/aidocs2.png'])
    const rels = await documentRelsOf(saved2)
    expect(rels.match(new RegExp(`Type="${IMAGE_REL_TYPE.replace(/[/]/g, '\\/')}`, 'g'))).toHaveLength(1)
    const out = await recordOutput('m6-image', saved2)
    expect(out.blocks[0].imageDataUrl).toMatch(/^data:image\/png;base64,/)
  })

  it('B14 删掉一个图片块：媒体 part 与关系被回收', async () => {
    const { parsed } = await recordAndParse('m6-image', {
      bodyXml: P('保留段落') + real.IMAGE_PARAGRAPH_XML,
      withImage: true,
    })
    const keep = parsed.blocks.find((b) => b.type === 'paragraph')!
    const saved = await saveDocx(parsed, [{ kind: 'original', docxIndex: keep.docxIndex! }])
    const zip = await zipOf(saved)
    expect(zip.file('word/media/image1.png')).toBeNull()
    expect(await documentRelsOf(saved)).not.toContain('media/image1.png')
    const out = await recordOutput('m6-image', saved)
    const visible = out.blocks.filter((b) => !b.hidden)
    expect(visible).toHaveLength(1)
    expect(visible[0].type).toBe('paragraph')
  })

  it('B14 删掉带图的表格块：媒体 part 与关系被回收', async () => {
    const tbl =
      '<w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc>' +
      '<w:p><w:r><w:t>格内 </w:t></w:r><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/>' +
      '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">' +
      '<pic:pic><pic:blipFill><a:blip r:embed="rId10"/></pic:blipFill></pic:pic>' +
      '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>' +
      '</w:tc></w:tr></w:tbl>'
    const { parsed } = await recordAndParse('m6-image', { bodyXml: P('表前段') + tbl, withImage: true })
    const keep = parsed.blocks.find((b) => b.type === 'paragraph')!
    const saved = await saveDocx(parsed, [{ kind: 'original', docxIndex: keep.docxIndex! }])
    const zip = await zipOf(saved)
    expect(zip.file('word/media/image1.png')).toBeNull()
    expect(await documentRelsOf(saved)).not.toContain('media/image1.png')
    const out = await recordOutput('m6-image', saved)
    const visible = out.blocks.filter((b) => !b.hidden)
    expect(visible).toHaveLength(1)
    expect(visible[0].type).toBe('paragraph')
  })
})
