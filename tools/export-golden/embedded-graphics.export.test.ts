/**
 * spec/17-m6-plan.md、M6-CORPUS.md A1–A40：嵌入图形的 TS 解析与保存语料。
 * 仅由录制包装生成期望；异常投影照录，不改输入来迁就显示结果。
 */
import JSZip from 'jszip'
import { describe, expect, it } from 'vitest'
import * as real from '../tests/helpers/build-docx'
import { record } from './record'
import { parseDocx, saveDocx, patchChartPartXml } from '../src/index'
import type { NewChart } from '../src/types'
import type { ChartPatch } from '../src/chart'
import type { SaveBlock } from '../src/patch'

const A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
const C = 'http://schemas.openxmlformats.org/drawingml/2006/chart'
const CX = 'http://schemas.microsoft.com/office/drawing/2014/chartex'
const DGM = 'http://schemas.openxmlformats.org/drawingml/2006/diagram'
const DSP = 'http://schemas.microsoft.com/office/drawing/2008/diagram'
const LC = 'http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas'
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
const MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'
const CT = 'application/vnd.openxmlformats-officedocument.'
const CHART_PATH = 'word/charts/chart1.xml'
const WORKBOOK_PATH = 'word/embeddings/Microsoft_Excel_Worksheet.xlsx'
const accents = ['4F81BD', 'C0504D', '9BBB59', '8064A2', '4BACC6', 'F79646']
const defaultAccents = ['4472C4', 'ED7D31', 'A5A5A5', 'FFC000', '5B9BD5', '70AD47']
const n: Record<string, number> = {}
const stem = (prefix: string) => `${prefix}__${String(n[prefix] = (n[prefix] ?? 0) + 1).padStart(3, '0')}`
const esc = (s: string | number) => String(s).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
const textRun = (s: string) => `<w:r><w:t>${esc(s)}</w:t></w:r>`
const paragraph = (s: string) => `<w:p>${s}</w:p>`
const table = (p: string) => '<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="6000"/></w:tblGrid><w:tr><w:tc><w:tcPr/>' + p + '</w:tc></w:tr></w:tbl>'
const rel = (id: string, type: string, target: string, attrs = '') => `<Relationship Id="${id}" Type="${type}" Target="${target}"${attrs}/>`
const rels = (s: string) => `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${s}</Relationships>`
const part = (path: string, xml: string, contentType = 'application/xml') => ({ path, xml, contentType })
const themePart = part('word/theme/theme1.xml', `<a:theme xmlns:a="${A}" name="M6"><a:themeElements><a:clrScheme name="M6">` +
  Object.entries({ dk1: '000000', lt1: 'FFFFFF', dk2: '1F497D', lt2: 'EEECE1', ...Object.fromEntries(accents.map((v, i) => [`accent${i + 1}`, v])), hlink: '0000FF', folHlink: '800080' })
    .map(([k, v]) => `<a:${k}><a:srgbClr val="${v}"/></a:${k}>`).join('') +
  '</a:clrScheme></a:themeElements></a:theme>', `${CT}theme+xml`)
const themeRel = rel('rIdTheme', `${R}/theme`, 'theme/theme1.xml')

async function source(prefix: string, options: real.BuildDocxOptions) {
  const name = stem(prefix)
  // 注释区分重复源形态，避免哈希去重把不同清单行折叠到旧语料。
  const bytes = await real.buildDocx({ ...options, bodyXml: `<!--${name}-->${options.bodyXml}` })
  await record(bytes, 'buildDocx', name)
  return { bytes, parsed: await parseDocx(bytes) }
}
async function output(bytes: Uint8Array) {
  await record(bytes, 'saveDocx-output', stem('m6-chart'))
  return parseDocx(bytes)
}
// 隐藏的末尾 sectPr 由保存器自动追加，不属于编辑器的块列表。
const originals = (p: Awaited<ReturnType<typeof parseDocx>>): SaveBlock[] =>
  p.blocks.filter((b) => !b.hidden).map((b) => ({ kind: 'original', docxIndex: b.docxIndex! }))

function drawing(content: string, uri: string, anchor = '', extent = true, id = 1) {
  const tag = anchor ? 'anchor' : 'inline'
  return `<w:r><w:drawing><wp:${tag}${anchor ? ' simplePos="0" relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"' : ''}>` +
    (anchor ? '<wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>190500</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>285750</wp:posOffset></wp:positionV>' : '') +
    (extent ? '<wp:extent cx="2857500" cy="1905000"/>' : '') +
    (anchor ? `<wp:${anchor}${anchor === 'wrapSquare' ? ' wrapText="bothSides"' : ''}/>` : '') +
    `<wp:docPr id="${id}" name="Graphic ${id}"/><a:graphic><a:graphicData uri="${uri}">${content}</a:graphicData></a:graphic></wp:${tag}></w:drawing></w:r>`
}
const chartRun = (anchor = '', id = 'rIdChart', docPr = 1) => drawing(`<c:chart xmlns:c="${C}" r:id="${id}"/>`, C, anchor, true, docPr)
function cache(tag: string, values: (string | number | null)[], mode = 'strRef', count = values.length, fmt = 'General') {
  const numeric = mode.startsWith('num')
  const points = (numeric ? `<c:formatCode>${fmt}</c:formatCode>` : '') +
    `<c:ptCount val="${count}"/>` + values.map((v, i) => v === null ? '' : `<c:pt idx="${i}"><c:v>${esc(v)}</c:v></c:pt>`).join('')
  // 字面量与引用分别走缓存容器；strLit 没有公式节点。
  const body = mode.endsWith('Ref') ? `<c:${mode}><c:f>S!$A$1</c:f><c:${numeric ? 'numCache' : 'strCache'}>${points}</c:${numeric ? 'numCache' : 'strCache'}></c:${mode}>` : `<c:${mode}>${points}</c:${mode}>`
  return `<c:${tag}>${body}</c:${tag}>`
}
const series = (extra = '', cat = cache('cat', ['A', 'B']), values = cache('val', [2, 4], 'numRef'), id = 0) =>
  `<c:ser><c:idx val="${id}"/><c:order val="${id}"/>${cache('tx', [`Series ${id + 1}`])}${extra}${cat}${values}</c:ser>`
const plot = (tag = 'barChart', extra = '', ser = series()) => `<c:${tag}>${extra}${ser}</c:${tag}>`
const chartXml = (plots = plot(), title = '', pre = '', post = '') =>
  `<c:chartSpace xmlns:c="${C}" xmlns:a="${A}" xmlns:r="${R}" xmlns:mc="${MC}">${pre}<c:chart>${title}<c:plotArea><c:layout/>${plots}</c:plotArea>${post}</c:chart></c:chartSpace>`
const richTitle = '<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Rich &amp; </a:t></a:r><a:r><a:t>title</a:t></a:r></a:p></c:rich></c:tx></c:title>'
const refTitle = `<c:title>${cache('tx', ['Cached title'])}</c:title>`
const autoTitle = '<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></c:rich></c:tx></c:title>'
function chartOptions(xml: string, opts: { theme?: boolean; body?: string; workbook?: boolean } = {}): real.BuildDocxOptions {
  return {
    bodyXml: opts.body ?? paragraph(chartRun()),
    extraRels: rel('rIdChart', `${R}/chart`, 'charts/chart1.xml') + (opts.theme ? themeRel : ''),
    extraParts: [part(CHART_PATH, opts.workbook ? xml.replace('</c:chartSpace>', '<c:externalData r:id="rIdBook"><c:autoUpdate val="0"/></c:externalData></c:chartSpace>') : xml, `${CT}drawingml.chart+xml`),
      ...(opts.theme ? [themePart] : []),
      ...(opts.workbook ? [part('word/charts/_rels/chart1.xml.rels', rels(rel('rIdBook', `${R}/package`, '../embeddings/Microsoft_Excel_Worksheet.xlsx')), 'application/vnd.openxmlformats-package.relationships+xml')] : [])],
    ...(opts.workbook ? { binaryParts: [{ path: WORKBOOK_PATH, base64: Buffer.from('M6 workbook placeholder').toString('base64'), extension: 'xlsx', contentType: `${CT}spreadsheetml.sheet` }] } : {}),
  }
}
async function chartSource(xml = chartXml(), opts: Parameters<typeof chartOptions>[1] = {}) {
  return source('m6-chart', chartOptions(xml, opts))
}
async function display(xml: string, theme = false) {
  const { parsed } = await chartSource(xml, { theme })
  expect(parsed.blocks[0]).toMatchObject({ type: 'passthrough', label: 'Chart' })
  expect(parsed.blocks[0].chartDisplay).toBeDefined()
  return parsed.blocks[0].chartDisplay!
}

describe('M6 classic charts A1-A20', () => {
  it.each([
    ['A1 clustered columns', 'barChart', '<c:barDir val="col"/><c:grouping val="clustered"/>', 'bar', undefined, undefined],
    ['A2 horizontal bars', 'barChart', '<c:barDir val="bar"/>', 'bar', true, undefined],
    ['A3 stacked columns', 'barChart', '<c:grouping val="stacked"/>', 'bar', undefined, 'stacked'],
    ['A3 percent stacked area', 'areaChart', '<c:grouping val="percentStacked"/>', 'area', undefined, 'percentStacked'],
  ] as const)('%s', async (_name, tag, extra, kind, horizontal, grouping) => {
    const d = await display(chartXml(plot(tag, extra)))
    expect(d.kind).toBe(kind)
    expect(d.horizontal).toBe(horizontal)
    expect(d.grouping).toBe(grouping)
  })
  it.each([true, false])('A4 line markers %s', async (markers) => {
    const d = await display(chartXml(plot('lineChart', `<c:marker val="${markers ? 1 : 0}"/>`)))
    expect(d.kind).toBe('line')
    expect(d.markers).toBe(markers || undefined)
  })
  it.each([['pieChart', '', undefined], ['doughnutChart', '<c:holeSize val="30"/>', 30], ['doughnutChart', '', 50], ['pie3DChart', '', undefined]] as const)(
    'A5 %s hole %s', async (tag, extra, hole) => {
      const d = await display(chartXml(plot(tag, extra)))
      expect(d.kind).toBe('pie')
      expect(d.holePct).toBe(hole)
    })
  it.each([['areaChart', 'area'], ['bar3DChart', 'bar'], ['line3DChart', 'line'], ['area3DChart', 'area']])('A6 %s', async (tag, kind) => {
    expect((await display(chartXml(plot(tag)))).kind).toBe(kind)
  })
  it.each([['marker', false], ['lineMarker', false], ['smoothMarker', false], ['lineMarker', true]] as const)('A7 scatter %s noFill %s', async (style, hidden) => {
    const ser = series(hidden ? '<c:spPr><a:ln><a:noFill/></a:ln></c:spPr>' : '', cache('xVal', [1, 2], 'numRef'), cache('yVal', [3, 4], 'numRef'))
    const d = await display(chartXml(plot('scatterChart', `<c:scatterStyle val="${style}"/>`, ser)))
    expect(d).toMatchObject({ kind: 'scatter', markers: true, categories: ['1', '2'] })
    expect(d.series[0].xValues).toEqual([1, 2])
    expect(d.series[0].line).toBe(style !== 'marker' && !hidden ? true : undefined)
  })
  it('A8 bubble sizes', async () => {
    const d = await display(chartXml(plot('bubbleChart', '', series(cache('bubbleSize', [10, 4], 'numRef'), cache('xVal', [1, 2], 'numRef'), cache('yVal', [3, 4], 'numRef')))))
    expect(d).toMatchObject({ kind: 'bubble', series: [{ values: [3, 4], xValues: [1, 2], sizes: [10, 4] }] })
  })
  it.each([['barChart', 'lineChart', 'bar'], ['lineChart', 'barChart', 'line']])('A9 combo %s first', async (first, second, kind) => {
    const d = await display(chartXml(plot(first) + plot(second, '', series('', undefined, undefined, 1))))
    expect(d.kind).toBe(kind)
    expect(d.series).toHaveLength(1)
    expect(d.series[0].name).toBe('Series 1')
  })
  it.each(['radarChart', 'stockChart'])('A10 unsupported %s', async (tag) => {
    expect((await display(chartXml(plot(tag)))).kind).toBe('other')
  })
  it.each([['strRef', 'numRef'], ['strLit', 'numLit'], ['numRef', 'numRef']])('A11 sparse %s / %s caches', async (catMode, valMode) => {
    const d = await display(chartXml(plot('barChart', '', series('', cache('cat', catMode === 'numRef' ? [1, null, 3] : ['A', null, 'C'], catMode, 4), cache('val', [2, null, 'not-a-number'], valMode, 4)))))
    expect(d.categories).toEqual([catMode === 'numRef' ? '1' : 'A', '', catMode === 'numRef' ? '3' : 'C', ''])
    expect(d.series[0].values).toEqual([2, null, null, null])
  })
  it('A12 date serial categories', async () => {
    const d = await display(chartXml(plot('barChart', '', series('', cache('cat', [37377, 37408], 'numRef', 2, 'm/d/yyyy')))))
    expect(d.categories).toEqual(['5/1/2002', '6/1/2002'])
  })
  it('A12 long decimal x values', async () => {
    const d = await display(chartXml(plot('scatterChart', '', series('', cache('xVal', ['0.70000000000000062', '1.23456789'], 'numRef'), cache('yVal', [2, 4], 'numRef')))))
    expect(d.categories).toEqual(['0.7', '1.2346'])
    expect(d.series[0].xValues).toEqual([0.70000000000000062, 1.23456789])
  })
  it.each([
    ['rich', richTitle, false, 'Rich & title'], ['strRef', refTitle, false, 'Cached title'],
    ['empty multiple series', '<c:title/>', true, 'Chart Title'], ['deleted', '<c:title/><c:autoTitleDeleted val="1"/>', false, undefined],
    ['single automatic', autoTitle, false, 'Series 1'],
  ] as const)('A13 title %s', async (_name, title, multiple, expected) => {
    const { parsed } = await chartSource(chartXml(plot('barChart', '', series() + (multiple ? series('', undefined, undefined, 1) : '')), title))
    expect(parsed.blocks[0].chartDisplay?.title).toBe(expected)
    expect(parsed.blocks[0].previewText).toBe(expected ?? '')
  })
  it.each([
    ['srgb', '<a:srgbClr val="FF0000"/>', false, 'FF0000'],
    ['scheme transformed', '<a:schemeClr val="accent1"><a:lumMod val="75000"/><a:lumOff val="25000"/></a:schemeClr>', true, '7BA1CE'],
    ['scheme without theme uses Office default', '<a:schemeClr val="accent1"/>', false, '4472C4'],
    ['system', '<a:sysClr val="windowText" lastClr="123456"/>', false, '123456'],
  ] as const)('A14 color %s', async (_name, color, theme, expected) => {
    const d = await display(chartXml(plot('barChart', '', series(`<c:spPr><a:solidFill>${color}</a:solidFill></c:spPr>`))), theme)
    expect(d.series[0].color).toBe(expected)
  })
  it('A14 pie point colors', async () => {
    const d = await display(chartXml(plot('pieChart', '', series('<c:dPt><c:idx val="1"/><c:spPr><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></c:spPr></c:dPt>'))))
    expect(d.series[0].pointColors).toEqual([null, '00FF00'])
  })
  it.each([1, 2, 5, 40, 102, 0])('A15 style %s with theme', async (style) => {
    const pre = style === 102 ? `<mc:AlternateContent><mc:Choice Requires="c14" xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart"><c14:style val="102"/></mc:Choice><mc:Fallback><c:style val="1"/></mc:Fallback></mc:AlternateContent>` : style ? `<c:style val="${style}"/>` : ''
    const d = await display(chartXml(plot(), '', pre), true)
    expect(d.palette).toHaveLength(6)
    if (style === 1) expect(d.palette).toEqual(['595959', 'D9D9D9', 'A6A6A6', '404040', 'BFBFBF', '8C8C8C'])
    else if (style === 5 || style === 40) {
      expect(d.palette![0]).toBe(accents[style === 5 ? 2 : 5])
      expect(new Set(d.palette).size).toBe(6)
    } else expect(d.palette).toEqual(accents)
  })
  it('A15 no theme uses default Office palette', async () => {
    // parseDocx 补默认主题；直接调用 parseChartPartXml 才会得到 undefined。
    expect((await display(chartXml())).palette).toEqual(defaultAccents)
  })
  it.each(['b', 'l', 'r', 't', 'tr', 'empty', 'absent'])('A16 legend %s', async (pos) => {
    const legend = pos === 'absent' ? '' : pos === 'empty' ? '<c:legend/>' : `<c:legend><c:legendPos val="${pos}"/></c:legend>`
    expect((await display(chartXml(plot(), '', '', legend))).legendPos).toBe(pos === 'absent' ? undefined : pos === 'empty' ? 'r' : pos)
  })
  it('A17 no cached series stays Chart without display', async () => {
    const { parsed } = await chartSource(chartXml(plot('barChart', '', '<c:ser><c:idx val="0"/><c:val><c:numRef><c:f>S!$A$1</c:f></c:numRef></c:val></c:ser>')))
    expect(parsed.blocks[0]).toMatchObject({ type: 'passthrough', label: 'Chart' })
    expect(parsed.blocks[0].chartDisplay).toBeUndefined()
  })
  it.each(['dangling', 'external', 'missing part'])('A18 %s relationship', async (mode) => {
    const { parsed } = await source('m6-chart', { bodyXml: paragraph(chartRun()), extraRels: mode === 'dangling' ? '' : rel('rIdChart', `${R}/chart`, mode === 'external' ? 'https://example.invalid/chart.xml' : 'charts/missing.xml', mode === 'external' ? ' TargetMode="External"' : '') })
    expect(parsed.blocks[0]).toMatchObject({ type: 'passthrough', label: 'Chart' })
    expect(parsed.blocks[0].chartDisplay).toBeUndefined()
  })
  it.each(['anchor', 'two charts', 'with text', 'table'])('A19 placement %s', async (mode) => {
    const opts = chartOptions(chartXml(), { body: mode === 'table' ? table(paragraph(chartRun())) : paragraph(chartRun(mode === 'anchor' ? 'wrapSquare' : '') + (mode === 'with text' ? textRun('Body text') : mode === 'two charts' ? chartRun('', 'rIdSecond', 2) : '')) })
    if (mode === 'two charts') {
      opts.extraRels += rel('rIdSecond', `${R}/chart`, 'charts/chart2.xml')
      opts.extraParts!.push(part('word/charts/chart2.xml', chartXml(plot('lineChart')), `${CT}drawingml.chart+xml`))
    }
    const { parsed } = await source('m6-chart', opts)
    if (mode === 'table') {
      expect(parsed.blocks[0].type).toBe('table')
      expect(parsed.blocks[0].chartDisplay).toBeUndefined()
      expect(parsed.blocks[0].originalXml).toContain('rIdChart')
      // 疑似缺陷：单元格投影不保留图表，原 XML 仍在。
      expect(parsed.blocks[0].table?.rows[0][0].richParas).toEqual([{ runs: [] }])
    } else {
      expect(parsed.blocks.filter((b) => !b.hidden)).toHaveLength(1)
      expect(parsed.blocks[0].chartDisplay?.kind).toBe('bar')
      expect(parsed.blocks[0].previewText).toBe('')
      if (mode === 'two charts') expect(parsed.extras.chartParts['word/charts/chart2.xml']).toBeUndefined()
      if (mode === 'with text') {
        expect(parsed.blocks[0].originalXml).toContain('Body text')
        expect(parsed.blocks[0].runs).toBeUndefined()
      }
    }
  })
  it('A20 externalData retains raw chart part', async () => {
    const opts = chartOptions(chartXml(), { workbook: true })
    const { parsed } = await source('m6-chart', opts)
    expect(parsed.extras.chartParts[CHART_PATH]).toBe(opts.extraParts![0].xml)
    expect(parsed.extras.chartParts[CHART_PATH]).toContain('<c:externalData r:id="rIdBook">')
  })
})

function chartexXml(layout: string, dataTag = 'chartData') {
  return `<cx:chartSpace xmlns:cx="${CX}" xmlns:a="${A}"><cx:${dataTag}><cx:data id="0">` +
    '<cx:strDim type="cat"><cx:lvl ptCount="2"><cx:pt idx="0">A</cx:pt><cx:pt idx="1">B</cx:pt></cx:lvl></cx:strDim>' +
    '<cx:numDim type="val"><cx:lvl ptCount="2"><cx:pt idx="0">100</cx:pt><cx:pt idx="1">-40</cx:pt></cx:lvl></cx:numDim>' +
    `</cx:data></cx:${dataTag}><cx:chart><cx:plotArea><cx:plotAreaRegion><cx:series layoutId="${layout}"><cx:tx><cx:txData><cx:v>Extended</cx:v></cx:txData></cx:tx><cx:dataId val="0"/></cx:series></cx:plotAreaRegion></cx:plotArea></cx:chart></cx:chartSpace>`
}
async function chartexSource(layout: string, dataTag = 'chartData', alternate?: boolean) {
  const run = drawing(`<cx:chart xmlns:cx="${CX}" r:id="rIdCx"/>`, CX)
  const body = alternate === undefined ? paragraph(run) : paragraph(`<mc:AlternateContent xmlns:mc="${MC}" xmlns:cx="${CX}"><mc:Choice Requires="cx">${run}</mc:Choice>${alternate ? `<mc:Fallback>${real.IMAGE_PARAGRAPH_XML.slice(5, -6)}</mc:Fallback>` : ''}</mc:AlternateContent>`)
  return source('m6-chartex', { bodyXml: body, withImage: alternate === true, extraRels: rel('rIdCx', 'http://schemas.microsoft.com/office/2014/relationships/chartEx', 'charts/chartEx1.xml'), extraParts: [part('word/charts/chartEx1.xml', chartexXml(layout, dataTag), 'application/vnd.ms-office.chartex+xml')] })
}
describe('M6 chartex A21-A24', () => {
  it.each([['sunburst', 'pie'], ['treemap', 'pie'], ['waterfall', 'bar'], ['boxWhisker', 'bar'], ['funnel', 'bar'], ['paretoLine', 'line']])('A21 %s', async (layout, kind) => {
    const { parsed } = await chartexSource(layout)
    expect(parsed.blocks[0].chartDisplay).toMatchObject({ kind, categories: ['A', 'B'], series: [{ name: 'Extended', values: [100, -40] }] })
    expect(parsed.extras.chartParts['word/charts/chartEx1.xml']).toBeUndefined()
  })
  it('A22 renamed chartData still parses', async () => {
    const { parsed } = await chartexSource('waterfall', 'renamedData')
    expect(parsed.blocks[0].chartDisplay).toMatchObject({ kind: 'bar', categories: ['A', 'B'], series: [{ values: [100, -40] }] })
  })
  it('A23 Choice chartex prefers Fallback picture', async () => {
    const { parsed } = await chartexSource('sunburst', 'chartData', true)
    expect(parsed.blocks[0]).toMatchObject({ type: 'image', imageDataUrl: `data:image/png;base64,${real.TINY_PNG_BASE64}` })
    expect(parsed.blocks[0].chartDisplay).toBeUndefined()
  })
  it('A24 Choice without Fallback has Chart display', async () => {
    const { parsed } = await chartexSource('sunburst', 'chartData', false)
    expect(parsed.blocks[0]).toMatchObject({ type: 'passthrough', label: 'Chart', chartDisplay: { kind: 'pie' } })
  })
})

const point = (id: string, texts: string[], type = '') => `<dgm:pt modelId="${id}"${type ? ` type="${type}"` : ''}><dgm:t><a:bodyPr/><a:lstStyle/><a:p>${texts.map((t) => `<a:r><a:t>${esc(t)}</a:t></a:r>`).join('')}</a:p></dgm:t></dgm:pt>`
const edge = (id: number, src: string, dst: string, ord = 0) => `<dgm:cxn modelId="e${id}" type="parOf" srcId="${src}" destId="${dst}" srcOrd="${ord}" destOrd="0"/>`
function dataXml(cyclic = false) {
  return `<dgm:dataModel xmlns:dgm="${DGM}" xmlns:a="${A}" xmlns:r="${R}"><dgm:ptLst>` +
    point('root', ['Root & ', 'team']) + point('later', ['Later']) + point('first', ['First']) + point('leaf', ['Leaf']) + point('alone', ['Isolated']) +
    point('pres', ['IGNORED pres'], 'pres') + point('par', ['IGNORED par'], 'parTrans') + point('sib', ['IGNORED sib'], 'sibTrans') +
    '</dgm:ptLst><dgm:cxnLst>' + (cyclic ? edge(1, 'root', 'later') + edge(2, 'later', 'root') + edge(3, 'first', 'first') : edge(1, 'root', 'later', 9) + edge(2, 'root', 'first', 1) + edge(3, 'first', 'leaf')) +
    '</dgm:cxnLst></dgm:dataModel>'
}
const txBody = (prefix: string, texts: string[], size: number) => `<${prefix}:txBody><a:bodyPr/><a:lstStyle/>${texts.map((t) => `<a:p><a:r><a:rPr sz="${size}"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:rPr><a:t>${esc(t)}</a:t></a:r></a:p>`).join('')}</${prefix}:txBody>`
const xfrm = (x: number, y: number, w: number, h: number, rot = 0) => `<a:xfrm${rot ? ` rot="${rot * 60000}"` : ''}><a:off x="${x * 9525}" y="${y * 9525}"/><a:ext cx="${w * 9525}" cy="${h * 9525}"/></a:xfrm>`
function diagramXml() {
  const specs = [
    { prst: 'rect', fill: '<a:solidFill><a:srgbClr val="FF0000"/></a:solidFill>', line: '<a:ln w="19050"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln>', rot: 30 },
    { prst: 'roundRect', fill: '<a:solidFill><a:schemeClr val="accent2"/></a:solidFill>', line: '<a:ln><a:noFill/></a:ln>' },
    { prst: 'ellipse', fill: '<a:noFill/>', line: '' },
    { prst: 'line', fill: '<a:noFill/>', line: '<a:ln><a:solidFill><a:srgbClr val="445566"/></a:solidFill></a:ln>' },
    { prst: 'bentConnector3', fill: '<a:noFill/>', line: '<a:ln><a:solidFill><a:srgbClr val="445566"/></a:solidFill></a:ln>' },
    { prst: 'rect', fill: '<a:blipFill><a:blip r:embed="rIdLocalImage"/><a:stretch><a:fillRect l="-10000" t="-20000" r="0" b="0"/></a:stretch></a:blipFill>', line: '' },
  ]
  return `<dsp:drawing xmlns:dsp="${DSP}" xmlns:a="${A}" xmlns:r="${R}"><dsp:spTree>` + specs.map((s, i) =>
    `<dsp:sp><dsp:nvSpPr><dsp:cNvPr id="${i + 1}" name="Shape ${i + 1}"/><dsp:cNvSpPr/></dsp:nvSpPr><dsp:spPr>${xfrm(10 + i * 35, 20, 30, i === 3 ? 0 : 40, s.rot)}<a:prstGeom prst="${s.prst}"><a:avLst/></a:prstGeom>${s.fill}${s.line}</dsp:spPr>${i === 0 ? txBody('dsp', ['First paragraph', 'Second & third'], 1400) : ''}</dsp:sp>`).join('') + '</dsp:spTree></dsp:drawing>'
}
const diagramRun = (anchor = '') => drawing(`<dgm:relIds xmlns:dgm="${DGM}" r:dm="rIdDm" r:lo="rIdLo" r:qs="rIdQs" r:cs="rIdCs"/>`, DGM, anchor)
const photoRun = drawing('<pic:pic><pic:nvPicPr><pic:cNvPr id="20" name="Photo"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId10"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr>' + xfrm(0, 0, 30, 40) + '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>', 'http://schemas.openxmlformats.org/drawingml/2006/picture', 'wrapNone', true, 20)
async function diagramSource(opts: { suffix?: string; drawing?: boolean; body?: string; cyclic?: boolean; theme?: boolean } = {}) {
  const suffix = opts.suffix ?? '1'
  const hasDrawing = opts.drawing ?? true
  const theme = opts.theme ?? true
  const paths = [['rIdDm', 'diagramData', `data${suffix}`, 'dataModel'], ['rIdLo', 'diagramLayout', 'layout1', 'layoutDef'], ['rIdQs', 'diagramQuickStyle', 'quickStyle1', 'styleDef'], ['rIdCs', 'diagramColors', 'colors1', 'colorsDef']]
  return source('m6-smartart', {
    bodyXml: opts.body ?? paragraph(diagramRun()), withImage: hasDrawing,
    extraRels: paths.map(([id, type, name]) => rel(id, `${R}/${type}`, `diagrams/${name}.xml`)).join('') + (theme ? themeRel : ''),
    extraParts: [
      ...paths.map(([, type, name, root]) => part(`word/diagrams/${name}.xml`, type === 'diagramData' ? dataXml(opts.cyclic) : `<dgm:${root} xmlns:dgm="${DGM}"/>`, `${CT}drawingml.${type === 'diagramQuickStyle' ? 'diagramStyle' : type}+xml`)),
      ...(theme ? [themePart] : []),
      ...(hasDrawing ? [part(`word/diagrams/drawing${suffix}.xml`, diagramXml(), 'application/vnd.ms-office.drawingml.diagramDrawing+xml'), part(`word/diagrams/_rels/drawing${suffix}.xml.rels`, rels(rel('rIdLocalImage', `${R}/image`, '../media/image1.png')), 'application/vnd.openxmlformats-package.relationships+xml'),
        part(`word/diagrams/_rels/data${suffix}.xml.rels`, rels(rel('rIdDrawing', 'http://schemas.microsoft.com/office/2007/relationships/diagramDrawing', `drawing${suffix}.xml`)), 'application/vnd.openxmlformats-package.relationships+xml')] : []),
    ],
  })
}
const treeText = 'Root & team\nFirst\nLeaf\nLater\nIsolated'
describe('M6 SmartArt A25-A31', () => {
  it('A25 data tree order excludes presentation and transition points', async () => {
    const { parsed } = await diagramSource({ drawing: false })
    expect(parsed.blocks[0]).toMatchObject({ type: 'passthrough', label: 'SmartArt', previewText: treeText })
    expect(parsed.blocks[0].diagramDisplay).toBeUndefined()
  })
  it('A26 drawing geometry fills connectors local picture and text', async () => {
    const { parsed } = await diagramSource()
    const d = parsed.blocks[0].diagramDisplay!
    expect(d).toMatchObject({ widthPx: 300, heightPx: 200 })
    expect(d.shapes).toHaveLength(6)
    expect(d.shapes[0]).toEqual({ xPx: 10, yPx: 20, wPx: 30, hPx: 40, prst: 'rect', rotDeg: 30, fillHex: 'FF0000', lnHex: '123456', lnWPx: 2, texts: ['First paragraph', 'Second & third'], fontSizePt: 14, textColorHex: '112233' })
    expect(d.shapes[1]).toEqual({ xPx: 45, yPx: 20, wPx: 30, hPx: 40, prst: 'roundRect', fillHex: 'C0504D' })
    expect(d.shapes[2]).toEqual({ xPx: 80, yPx: 20, wPx: 30, hPx: 40, prst: 'ellipse' })
    expect(d.shapes[3]).toEqual({ xPx: 115, yPx: 20, wPx: 30, hPx: 0, prst: 'line', lnHex: '445566', lnWPx: 1 })
    expect(d.shapes[4]).toEqual({ xPx: 150, yPx: 20, wPx: 30, hPx: 40, prst: 'bentConnector3', lnHex: '445566', lnWPx: 1 })
    expect(d.shapes[5]).toEqual({ xPx: 185, yPx: 20, wPx: 30, hPx: 40, prst: 'rect', imageDataUrl: `data:image/png;base64,${real.TINY_PNG_BASE64}`, fillRect: { l: -0.1, t: -0.2, r: 0, b: 0 } })
  })
  it('A26 scheme without theme uses default Office accent', async () => {
    const { parsed } = await diagramSource({ theme: false })
    expect(parsed.blocks[0].diagramDisplay?.shapes[1].fillHex).toBe('ED7D31')
  })
  it.each(['', '3'])('A27 data%s.xml companion drawing lookup', async (suffix) => {
    const { parsed } = await diagramSource({ suffix })
    expect(parsed.blocks[0].diagramDisplay?.shapes).toHaveLength(6)
    expect(parsed.blocks[0].previewText).toBe(treeText)
  })
  it('A28 missing drawing preserves only data text', async () => {
    const { parsed } = await diagramSource({ suffix: '3', drawing: false })
    expect(parsed.blocks[0].previewText).toBe(treeText)
    expect(parsed.blocks[0].diagramDisplay).toBeUndefined()
  })
  it('A29 anchored diagram and sibling photo', async () => {
    const { parsed } = await diagramSource({ body: paragraph(diagramRun('wrapSquare') + photoRun) })
    expect(parsed.blocks[0].diagramDisplay).toMatchObject({ floating: true, offsetXEmu: 190500, offsetYEmu: 285750 })
    expect(parsed.blocks[0].textboxes?.some((b) => JSON.stringify(b).includes(real.TINY_PNG_BASE64))).toBe(true)
  })
  it.each(['table', 'with text'])('A30 diagram %s', async (mode) => {
    const { parsed } = await diagramSource({ body: mode === 'table' ? table(paragraph(diagramRun())) : paragraph(textRun('Body text') + diagramRun()) })
    expect(parsed.blocks[0].originalXml).toContain('rIdDm')
    if (mode === 'table') {
      expect(parsed.blocks[0].type).toBe('table')
      expect(parsed.blocks[0].diagramDisplay).toBeUndefined()
      expect(parsed.blocks[0].table?.rows[0][0].richParas).toEqual([{ runs: [] }])
    } else {
      expect(parsed.blocks[0].previewText).toBe(treeText)
      expect(parsed.blocks[0].diagramDisplay?.shapes).toHaveLength(6)
      expect(parsed.blocks[0].originalXml).toContain('Body text')
    }
  })
  it('A31 cyclic and self edges terminate and keep every text once', async () => {
    const { parsed } = await diagramSource({ drawing: false, cyclic: true })
    expect(parsed.blocks[0].previewText).toBe('Root & team\nLater\nFirst\nLeaf\nIsolated')
    expect(new Set(parsed.blocks[0].previewText!.split('\n')).size).toBe(5)
  })
})

function canvasShape(x: number, y: number, w: number, h: number, texts: string[] = [], prst = 'rect', extra = '', rot = 0) {
  return `<a:sp><a:nvSpPr><a:cNvPr id="${x + 1}" name="Canvas shape"/><a:cNvSpPr/></a:nvSpPr><a:spPr>${xfrm(x, y, w, h, rot)}<a:prstGeom prst="${prst}"><a:avLst/></a:prstGeom>${extra}</a:spPr>${texts.length ? `<a:txSp>${txBody('a', texts, 1800)}</a:txSp>` : ''}</a:sp>`
}
const canvasChildren = canvasShape(90, 120, 270, 180, ['Canvas'], 'rect', '<a:solidFill><a:srgbClr val="AABBCC"/></a:solidFill>', 45) +
  canvasShape(390, 120, 180, 180, [], 'ellipse', '<a:solidFill><a:schemeClr val="accent1"/></a:solidFill>') +
  '<a:pic><a:nvPicPr><a:cNvPr id="900" name="Canvas picture"/><a:cNvPicPr/></a:nvPicPr><a:blipFill><a:blip r:embed="rId10"/></a:blipFill><a:spPr>' + xfrm(600, 120, 180, 180) + '</a:spPr></a:pic>'
async function canvasSource(opts: { children?: string; anchor?: string; extent?: boolean; zero?: boolean; text?: boolean } = {}) {
  const canvas = `<lc:lockedCanvas xmlns:lc="${LC}" xmlns:a="${A}" xmlns:r="${R}"><a:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2857500" cy="1905000"/><a:chOff x="285750" y="571500"/><a:chExt cx="${opts.zero ? 0 : 8572500}" cy="${opts.zero ? 0 : 5715000}"/></a:xfrm></a:grpSpPr>${opts.children ?? canvasChildren}</lc:lockedCanvas>`
  return source('m6-canvas', { bodyXml: paragraph((opts.text ? textRun('Body text') : '') + drawing(canvas, LC, opts.anchor, opts.extent ?? true)), withImage: true, extraParts: [themePart], extraRels: themeRel })
}
describe('M6 locked canvas A32-A35', () => {
  it('A32 scaled geometry and raw font sizes', async () => {
    const { parsed } = await canvasSource()
    const d = parsed.blocks[0].diagramDisplay!
    expect(d).toMatchObject({ canvas: true, widthPx: 300, heightPx: 200 })
    // (90 - 30) / 3 = 20；字号保留 18pt，矩形不带 prst。
    expect(d.shapes).toEqual([
      { xPx: 20, yPx: 20, wPx: 90, hPx: 60, rotDeg: 45, fillHex: 'AABBCC', texts: ['Canvas'], fontSizePt: 18, textColorHex: '112233' },
      { xPx: 120, yPx: 20, wPx: 60, hPx: 60, prst: 'ellipse', fillHex: '4F81BD' },
      { xPx: 190, yPx: 20, wPx: 60, hPx: 60, imageDataUrl: `data:image/png;base64,${real.TINY_PNG_BASE64}` },
    ])
  })
  it('A33 overflowing columns are repositioned', async () => {
    const { parsed } = await canvasSource({ children: canvasShape(90, 120, 180, 15, ['ABCDEFGHIJKL']) + canvasShape(300, 120, 180, 15, ['MNOPQRSTUVWX']) })
    const shapes = parsed.blocks[0].diagramDisplay!.shapes
    expect(shapes).toHaveLength(2)
    expect(shapes.map((s) => s.yPx)).toEqual([0, 35])
    expect(shapes.map((s) => s.texts)).toEqual([['ABCDEFGHIJKL'], ['MNOPQRSTUVWX']])
  })
  it('A33 one letter per line splits into glyph shapes', async () => {
    const { parsed } = await canvasSource({ children: canvasShape(90, 120, 30, 15, ['ABCD']) })
    const shapes = parsed.blocks[0].diagramDisplay!.shapes
    expect(shapes.map((s) => s.texts)).toEqual([['A'], ['B'], ['C'], ['D']])
    expect(shapes.map((s) => s.yPx)).toEqual([0, 29, 58, 86])
    expect(shapes.every((s) => s.fontSizePt === 18 && s.hPx === 29)).toBe(true)
  })
  it.each(['wrapNone', 'wrapSquare'])('A34 anchored canvas %s drops vertical offset', async (anchor) => {
    const { parsed } = await canvasSource({ anchor })
    const d = parsed.blocks[0].diagramDisplay!
    expect(d.offsetXEmu).toBe(190500)
    expect(d.offsetYEmu).toBeUndefined()
    expect(d.floating).toBe(anchor === 'wrapNone' ? true : undefined)
  })
  it('A35 missing extent cannot build canvas display', async () => {
    const { parsed } = await canvasSource({ extent: false })
    expect(parsed.blocks[0].diagramDisplay).toBeUndefined()
    expect(parsed.blocks[0].type).toBe('image')
    expect(parsed.blocks[0].originalXml).toContain('<lc:lockedCanvas')
  })
  it('A35 zero chExt uses unit scale', async () => {
    const { parsed } = await canvasSource({ zero: true })
    expect(parsed.blocks[0].diagramDisplay?.shapes[0]).toMatchObject({ xPx: 60, yPx: 60, wPx: 270, hPx: 180, fontSizePt: 18 })
  })
  it('A35 canvas sharing paragraph keeps only shape preview', async () => {
    const { parsed } = await canvasSource({ text: true })
    expect(parsed.blocks[0]).toMatchObject({ label: 'Drawing object', previewText: 'Canvas', diagramDisplay: { canvas: true } })
    expect(parsed.blocks[0].originalXml).toContain('Body text')
  })
})

describe('M6 chart save A36-A40', () => {
  it.each(['bar', 'line', 'pie', 'extent', 'null', 'two', 'between'] as const)('A36 insert chart %s', async (mode) => {
    const { parsed } = await source('m6-chart', { bodyXml: paragraph(textRun('Before')) + paragraph(textRun('After')) })
    const kind = mode === 'line' || mode === 'pie' ? mode : 'bar'
    const chart: NewChart = { kind, title: `Inserted ${mode}`, categories: ['A', 'B'], series: [{ name: 'Inserted series', values: mode === 'null' ? [2, null] : [2, 4] }] }
    const insert: SaveBlock = { kind: 'chart', chart, ...(mode === 'extent' ? { extentPx: { w: 320, h: 180 } } : {}) }
    const original = originals(parsed)
    const blocks = mode === 'between' ? [original[0], insert, original[1]] : [...original, insert, ...(mode === 'two' ? [{ kind: 'chart' as const, chart: { ...chart, kind: 'pie' as const, title: 'Second chart' } }] : [])]
    const saved = await saveDocx(parsed, blocks)
    const reparsed = await output(saved)
    const charts = reparsed.blocks.filter((b) => b.chartDisplay)
    expect(charts).toHaveLength(mode === 'two' ? 2 : 1)
    expect(charts[0].chartDisplay).toMatchObject(chart)
    if (mode === 'two') expect(charts[1].chartDisplay).toMatchObject({ kind: 'pie', title: 'Second chart' })
    if (mode === 'extent') expect(charts[0].chartDisplay).toMatchObject({ widthPx: 320, heightPx: 180 })
    if (mode === 'between') expect(reparsed.blocks.filter((b) => !b.hidden).map((b) => b.type)).toEqual(['paragraph', 'passthrough', 'paragraph'])
    const zip = await JSZip.loadAsync(saved)
    expect(zip.file(CHART_PATH)).toBeTruthy()
    expect(await zip.file('word/_rels/document.xml.rels')!.async('string')).toContain('charts/chart1.xml')
  })
  const patches: { name: string; title: string; patch: ChartPatch; expected: object }[] = [
    { name: 'rich title', title: richTitle, patch: { title: 'Updated & title' }, expected: { title: 'Updated & title' } },
    { name: 'series name', title: richTitle, patch: { series: [{ name: 'Renamed' }] }, expected: { series: [{ name: 'Renamed', values: [2, 4] }] } },
    { name: 'values null preserves original', title: richTitle, patch: { series: [{ values: [null, 99] }] }, expected: { series: [{ values: [2, 99] }] } },
    { name: 'categories', title: refTitle, patch: { categories: [null, 'Changed'] }, expected: { categories: ['A', 'Changed'] } },
    { name: 'automatic title injection', title: autoTitle, patch: { title: 'Explicit title' }, expected: { title: 'Explicit title' } },
    { name: 'strRef title', title: refTitle, patch: { title: 'Updated cache' }, expected: { title: 'Updated cache' } },
  ]
  it.each(patches)('A37 partXml $name', async ({ title, patch, expected }) => {
    const { bytes, parsed } = await chartSource(chartXml(plot(), title))
    const patched = patchChartPartXml(parsed.extras.chartParts[CHART_PATH], patch)
    const saved = await saveDocx(parsed, originals(parsed), { partXml: { [CHART_PATH]: patched } })
    const reparsed = await output(saved)
    expect(reparsed.blocks[0].chartDisplay).toMatchObject(expected)
    const oldZip = await JSZip.loadAsync(bytes)
    const zip = await JSZip.loadAsync(saved)
    expect(await zip.file(CHART_PATH)!.async('string')).toBe(patched)
    expect(await zip.file('word/document.xml')!.async('string')).toBe(await oldZip.file('word/document.xml')!.async('string'))
  })
  it('A38 partBinary replaces embedded workbook bytes', async () => {
    const { parsed } = await chartSource(chartXml(), { workbook: true })
    const replacement = Buffer.from('M6 replaced workbook bytes').toString('base64')
    const saved = await saveDocx(parsed, originals(parsed), { partBinary: { [WORKBOOK_PATH]: replacement } })
    const zip = await JSZip.loadAsync(saved)
    expect(await zip.file(WORKBOOK_PATH)!.async('base64')).toBe(replacement)
    expect(await zip.file(CHART_PATH)!.async('string')).toBe(parsed.extras.chartParts[CHART_PATH])
  })
  it('A39 deleting chart removes chart part rels workbook and overrides', async () => {
    const { parsed } = await chartSource(chartXml(), { workbook: true, body: paragraph(textRun('Keep')) + paragraph(chartRun()) })
    const saved = await saveDocx(parsed, parsed.blocks.filter((b) => !b.hidden && b.label !== 'Chart').map((b) => ({ kind: 'original', docxIndex: b.docxIndex! })))
    const reparsed = await output(saved)
    expect(reparsed.blocks.filter((b) => !b.hidden)).toHaveLength(1)
    expect(reparsed.blocks[0].type).toBe('paragraph')
    expect(reparsed.extras.chartParts).toEqual({})
    const zip = await JSZip.loadAsync(saved)
    for (const path of [CHART_PATH, 'word/charts/_rels/chart1.xml.rels', WORKBOOK_PATH]) expect(zip.file(path)).toBeNull()
    expect(await zip.file('word/_rels/document.xml.rels')!.async('string')).not.toContain('rIdChart')
    expect(await zip.file('[Content_Types].xml')!.async('string')).not.toContain('/word/charts/chart1.xml')
  })
  it('A40 all-original save is byte-identical', async () => {
    const { bytes, parsed } = await chartSource(chartXml(plot(), richTitle), { workbook: true, theme: true })
    const saved = await saveDocx(parsed, originals(parsed))
    expect(saved).toEqual(bytes)
  })
})
