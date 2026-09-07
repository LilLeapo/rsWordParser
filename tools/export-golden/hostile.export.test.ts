/**
 * TEST-09 恶意与畸形输入清单 → corpus/hostile/；以及 TS 测试没有经 buildDocx 构造的补充语料
 *（Strict、Mixed）→ corpus/synthetic/extra__*.docx。作为 vitest 用例运行以复用 genoffice 的依赖。
 */
import { crc32 } from 'node:zlib'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import JSZip from 'jszip'
import { describe, expect, it } from 'vitest'
import * as real from '../tests/helpers/build-docx'
import { record } from './record'

const HOSTILE = process.env.EXPORT_GOLDEN_HOSTILE_OUT
if (!HOSTILE) throw new Error('EXPORT_GOLDEN_HOSTILE_OUT is not set')
mkdirSync(HOSTILE, { recursive: true })

const XML_DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n'
const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
const W_STRICT = 'http://purl.oclc.org/ooxml/wordprocessingml/main'
const R_STRICT = 'http://purl.oclc.org/ooxml/officeDocument/relationships'
const REL_T = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
const REL_S = 'http://purl.oclc.org/ooxml/officeDocument/relationships'
const CT_MAIN = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'
const CT_HDR = 'application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml'
const PLAIN = '<w:p><w:r><w:t>hello</w:t></w:r></w:p>'

const manifest: Array<Record<string, unknown>> = []
function emit(name: string, bytes: Uint8Array, expectation: string, ext = 'docx'): void {
  writeFileSync(join(HOSTILE!, `${name}.${ext}`), bytes)
  manifest.push({ name: `${name}.${ext}`, bytes: bytes.length, expectation })
}

/** 改写每条 central directory 记录声明的解压大小（hostile-input.test.ts 同款）。 */
function patchCentralSizes(bytes: Uint8Array, size: number): Uint8Array {
  const out = bytes.slice()
  const dv = new DataView(out.buffer, out.byteOffset, out.byteLength)
  for (let i = 0; i + 28 <= out.length; i++) {
    if (out[i] === 0x50 && out[i + 1] === 0x4b && out[i + 2] === 0x01 && out[i + 3] === 0x02) {
      dv.setUint32(i + 24, size, true)
    }
  }
  return out
}

function findEocd(bytes: Uint8Array): number {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  for (let i = bytes.length - 22; i >= 0; i--) if (view.getUint32(i, true) === 0x06054b50) return i
  throw new Error('no EOCD')
}

/** 给 entry 的 central directory 记录加 crc 合法的 0x7075 字段（zip-local-names.test.ts 同款）。 */
function injectUnicodePath(bytes: Uint8Array, entry: string, claimed: string): Uint8Array {
  const eocd = findEocd(bytes)
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const count = view.getUint16(eocd + 10, true)
  const dec = new TextDecoder()
  const enc = new TextEncoder()
  let p = view.getUint32(eocd + 16, true)
  for (let i = 0; i < count; i++) {
    expect(view.getUint32(p, true)).toBe(0x02014b50)
    const nameLen = view.getUint16(p + 28, true)
    const extraLen = view.getUint16(p + 30, true)
    const commentLen = view.getUint16(p + 32, true)
    const recordEnd = p + 46 + nameLen + extraLen + commentLen
    if (dec.decode(bytes.subarray(p + 46, p + 46 + nameLen)) === entry) {
      const claimedBytes = enc.encode(claimed)
      const field = new Uint8Array(4 + 5 + claimedBytes.length)
      const fv = new DataView(field.buffer)
      fv.setUint16(0, 0x7075, true)
      fv.setUint16(2, 5 + claimedBytes.length, true)
      field[4] = 1
      fv.setUint32(5, crc32(enc.encode(entry)), true)
      field.set(claimedBytes, 9)
      const out = new Uint8Array(bytes.length + field.length)
      out.set(bytes.subarray(0, recordEnd))
      out.set(field, recordEnd)
      out.set(bytes.subarray(recordEnd), recordEnd + field.length)
      const ov = new DataView(out.buffer)
      ov.setUint16(p + 8, view.getUint16(p + 8, true) & ~0x800, true)
      ov.setUint16(p + 30, extraLen + field.length, true)
      const newEocd = eocd + field.length
      ov.setUint32(newEocd + 12, view.getUint32(eocd + 12, true) + field.length, true)
      return out
    }
    p = recordEnd
  }
  throw new Error(`entry not found: ${entry}`)
}

function contentTypes(overrides: Array<[string, string]>): string {
  return (
    `${XML_DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
    '<Default Extension="xml" ContentType="application/xml"/>' +
    overrides.map(([p, ct]) => `<Override PartName="${p}" ContentType="${ct}"/>`).join('') +
    '</Types>'
  )
}

function rels(entries: Array<[string, string, string]>): string {
  return (
    `${XML_DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
    entries.map(([id, type, target]) => `<Relationship Id="${id}" Type="${type}" Target="${target}"/>`).join('') +
    '</Relationships>'
  )
}

async function zipOf(files: Record<string, string | Uint8Array>): Promise<Uint8Array> {
  const zip = new JSZip()
  for (const [path, content] of Object.entries(files)) zip.file(path, content)
  return zip.generateAsync({ type: 'uint8array' })
}

async function mutate(bytes: Uint8Array, f: (zip: JSZip) => Promise<void> | void): Promise<Uint8Array> {
  const zip = await JSZip.loadAsync(bytes)
  await f(zip)
  return zip.generateAsync({ type: 'uint8array' })
}

/** 3000 层 `w:txbxContent` 套娃（页眉里的深树，MOD_TOO_DEEP）。 */
function nestedTxbx(depth: number): string {
  const open =
    '<w:txbxContent><w:p><w:r><w:pict>' +
    `<v:shape xmlns:v="${V}" id="n" type="#_x0000_t202" style="width:10pt;height:10pt"><v:textbox>`
  const close = '</v:textbox></v:shape></w:pict></w:r></w:p></w:txbxContent>'
  return (
    open.repeat(depth) +
    '<w:txbxContent><w:p><w:r><w:t>bottom</w:t></w:r></w:p></w:txbxContent>' +
    close.repeat(depth)
  )
}

function nestedTables(depth: number): string {
  return (
    '<w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc>'.repeat(depth) +
    '<w:p><w:r><w:t>deep</w:t></w:r></w:p>' +
    '</w:tc></w:tr></w:tbl>'.repeat(depth)
  )
}


const WPS = 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape'
const WPG = 'http://schemas.microsoft.com/office/word/2010/wordprocessingGroup'
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
const WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'
const V = 'urn:schemas-microsoft-com:vml'

/** 深度 depth 的 wpg 组套娃，最里面放一个带字的形状。 */
function nestedGroups(depth: number): string {
  const open =
    '<wpg:grpSp><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/>' +
    '<a:chOff x="0" y="0"/><a:chExt cx="914400" cy="914400"/></a:xfrm></wpg:grpSpPr>'
  const shape =
    '<wps:wsp><wps:cNvPr id="9" name="deep"/><wps:spPr><a:xfrm><a:off x="0" y="0"/>' +
    '<a:ext cx="100000" cy="100000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>' +
    '<a:solidFill><a:srgbClr val="4472C4"/></a:solidFill></wps:spPr>' +
    '<wps:txbx><w:txbxContent><w:p><w:r><w:t>deep</w:t></w:r></w:p></w:txbxContent></wps:txbx>' +
    '</wps:wsp>'
  return (
    `<w:p><w:r><w:drawing><wp:inline xmlns:wp="${WP}"><wp:extent cx="914400" cy="914400"/>` +
    `<a:graphic xmlns:a="${A}"><a:graphicData uri="${WPG}">` +
    `<wpg:wgp xmlns:wpg="${WPG}" xmlns:wps="${WPS}">` +
    open.repeat(depth) +
    shape +
    '</wpg:grpSp>'.repeat(depth) +
    '</wpg:wgp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'
  )
}

describe('TEST-09 hostile corpus', () => {
  it('zip limits (PKG-02)', async () => {
    const base = await real.buildDocx({ bodyXml: PLAIN })
    emit('zip-part-too-large', patchCentralSizes(base, 600 * 1024 * 1024), 'PKG_PART_TOO_LARGE')
    emit('zip-total-too-large', patchCentralSizes(base, 450 * 1024 * 1024), 'PKG_TOTAL_TOO_LARGE')
    const many = await mutate(base, (zip) => {
      for (let i = 0; i < 10010; i++) zip.file(`junk/${i}.bin`, 'x')
    })
    emit('zip-too-many-parts', many, 'PKG_TOO_MANY_PARTS')
  })

  it('zip unicode-path shadow (PKG-01)', async () => {
    const bytes = await real.buildDocx({
      bodyXml: '<w:p><w:r><w:t>RIGHT</w:t></w:r></w:p>',
      extraParts: [
        {
          path: 'decoy-content.xml',
          xml:
            XML_DECL +
            `<w:document xmlns:w="${W}"><w:body><w:p><w:r><w:t>WRONG</w:t></w:r></w:p></w:body></w:document>`,
          contentType: 'application/xml',
        },
      ],
    })
    const hostile = injectUnicodePath(
      injectUnicodePath(bytes, 'decoy-content.xml', 'word/document.xml'),
      'word/document.xml',
      'decoy-content.xml',
    )
    emit('zip-unicode-path-shadow', hostile, 'body text is RIGHT; unedited save is byte-identical')
  })

  it('deep XML (XML-08)', async () => {
    const smart =
      '<w:p>' + '<w:smartTag>'.repeat(3000) + '<w:r><w:t>deep</w:t></w:r>' + '</w:smartTag>'.repeat(3000) + '</w:p>'
    emit('xml-deep-smarttag', await real.buildDocx({ bodyXml: smart + PLAIN }), 'parses; paragraph editable')
    emit('xml-deep-table', await real.buildDocx({ bodyXml: nestedTables(5000) + PLAIN }), 'parses; depth>64 is TooDeep')
  })

  it('unbalanced XML (XML-08, PKG-11)', async () => {
    const base = await real.buildDocx({ bodyXml: PLAIN })
    const main = await mutate(base, async (zip) => {
      const xml = await zip.file('word/document.xml')!.async('string')
      zip.file('word/document.xml', xml.replace('</w:r></w:p>', '</w:p>'))
    })
    emit('xml-unbalanced-main', main, 'Err(XML_MALFORMED)')

    const withHeader = await real.buildDocx({
      bodyXml: PLAIN,
      sectPrExtra: '<w:headerReference w:type="default" r:id="rIdHdr"/>',
      extraRels: `<Relationship Id="rIdHdr" Type="${REL_T}/header" Target="header1.xml"/>`,
      extraParts: [
        {
          path: 'word/header1.xml',
          xml: `${XML_DECL}<w:hdr xmlns:w="${W}"><w:p><w:r><w:t>hdr</w:t></w:p></w:hdr>`,
          contentType: CT_HDR,
        },
      ],
    })
    emit('xml-unbalanced-header', withHeader, 'parses; header part Opaque + diagnostic')
  })

  it('field / span / rels / ids (FLD-02, SPAN-04, PKG-05/06, SAVE-02)', async () => {
    emit(
      'field-unclosed',
      await real.buildDocx({
        bodyXml:
          '<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>' +
          '<w:r><w:t>tail</w:t></w:r></w:p>' +
          PLAIN,
      }),
      'FLD_UNCLOSED diagnostic; paragraph editable; unedited save byte-identical',
    )
    emit(
      'span-orphan-end',
      await real.buildDocx({ bodyXml: '<w:p><w:r><w:t>a</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p>' + PLAIN }),
      'SPAN_ORPHAN_END PreExisting; save succeeds',
    )
    emit(
      'rels-missing-target',
      await real.buildDocx({ bodyXml: real.IMAGE_PARAGRAPH_XML.replace(/r:embed="[^"]*"/, 'r:embed="rId404"') + PLAIN }),
      'image broken; unedited save byte-identical',
    )
    emit(
      'rels-escape-root',
      await real.buildDocx({
        bodyXml: real.IMAGE_PARAGRAPH_XML.replace(/r:embed="[^"]*"/, 'r:embed="rIdEsc"') + PLAIN,
        extraRels: `<Relationship Id="rIdEsc" Type="${REL_T}/image" Target="../../x.png"/>`,
      }),
      'PKG_PATH_ESCAPES_ROOT diagnostic; treated as missing',
    )
    emit(
      'dup-ids',
      await real.buildDocx({
        bodyXml:
          '<w:p><w:ins w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z"><w:r><w:t>x</w:t></w:r></w:ins>' +
          '<w:ins w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z"><w:r><w:t>y</w:t></w:r></w:ins></w:p>',
      }),
      'diagnostic; save succeeds',
    )
  })

  it('package-level damage (PKG-04, XML-01)', async () => {
    const base = await real.buildDocx({ bodyXml: PLAIN })
    emit('content-types-missing', await mutate(base, (zip) => zip.remove('[Content_Types].xml')), 'PKG_NO_CONTENT_TYPES; parse continues')
    const utf16 = await mutate(base, async (zip) => {
      const styles = await zip.file('word/styles.xml')!.async('string')
      const decl = styles.replace(/encoding="UTF-8"/, 'encoding="UTF-16"')
      const body = Buffer.from(decl, 'utf16le')
      zip.file('word/styles.xml', Buffer.concat([Buffer.from([0xff, 0xfe]), body]))
    })
    emit('encoding-utf16-part', utf16, 'XML_TRANSCODED diagnostic; styles parsed')
  })

  it('flavors (PKG-08): strict-minimal, mixed-flavor', async () => {
    const strictDoc =
      XML_DECL +
      `<w:document xmlns:w="${W_STRICT}" xmlns:r="${R_STRICT}"><w:body>` +
      '<w:p><w:pPr><w:keepNext w:val="true"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>strict</w:t></w:r></w:p>' +
      '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:body></w:document>'
    const strict = await zipOf({
      '[Content_Types].xml': contentTypes([['/word/document.xml', CT_MAIN]]),
      '_rels/.rels': rels([['rId1', `${REL_S}/officeDocument`, 'word/document.xml']]),
      'word/document.xml': strictDoc,
    })
    await record(strict, 'hostile.export', 'extra__strict-minimal')

    const strictWithHeader =
      XML_DECL +
      `<w:document xmlns:w="${W_STRICT}" xmlns:r="${R_STRICT}"><w:body>` +
      PLAIN +
      '<w:sectPr><w:headerReference w:type="default" r:id="rId2"/><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>' +
      '</w:body></w:document>'
    const mixed = await zipOf({
      '[Content_Types].xml': contentTypes([
        ['/word/document.xml', CT_MAIN],
        ['/word/header1.xml', CT_HDR],
      ]),
      '_rels/.rels': rels([['rId1', `${REL_S}/officeDocument`, 'word/document.xml']]),
      'word/document.xml': strictWithHeader,
      'word/_rels/document.xml.rels': rels([['rId2', `${REL_S}/header`, 'header1.xml']]),
      'word/header1.xml': `${XML_DECL}<w:hdr xmlns:w="${W}" xmlns:r="${R}"><w:p><w:r><w:t>transitional header</w:t></w:r></w:p></w:hdr>`,
    })
    emit('mixed-flavor', mixed, 'PackageFlavor::Mixed; each part keeps its own flavor')
    await record(mixed, 'hostile.export', 'extra__mixed-flavor')
  })

  it('drawing trees (TEST-09, MOD-11)', async () => {
    // 组套娃 3000 层：遍历必须是迭代的，深度上限之外降级而不是爆栈
    emit(
      'drawing-deep-groups',
      await real.buildDocx({ bodyXml: nestedGroups(3000) + PLAIN }),
      'parses; group depth > 64 degrades, no stack overflow',
    )
    // VML 画布退化：coordsize 为 0 / 非数字，孩子坐标离谱，组还引用自己的 shapetype
    const canvas =
      `<w:p><w:r><w:pict><v:group xmlns:v="${V}" xmlns:o="urn:schemas-microsoft-com:office:office"` +
      ' id="g1" editas="canvas" style="width:150pt;height:75pt" coordsize="0,0" coordorigin="-2147483648,-2147483648">' +
      '<v:shapetype id="g1" o:spt="75" coordsize="21600,21600"/>' +
      '<v:shape id="c1" type="#g1" style="position:absolute;left:99999999999;top:-99999999999;width:1e400;height:0">' +
      '<v:textbox><w:txbxContent><w:p><w:r><w:t>canvas child</w:t></w:r></w:p></w:txbxContent></v:textbox>' +
      '</v:shape>' +
      `<v:group id="g1" style="width:0;height:0" coordsize="1,1"><v:shape id="c2" type="#g1" style="width:10;height:10"/></v:group>` +
      '</v:group></w:pict></w:r></w:p>'
    emit(
      'drawing-cyclic-group',
      await real.buildDocx({ bodyXml: canvas + PLAIN }),
      'parses; degenerate group scale and self-referencing ids do not loop',
    )
    // 绘图里的关系全是悬空的：图、VML 预览图、外部文本框 part
    const dangling =
      '<w:p><w:r><w:drawing><wp:inline xmlns:wp="' + WP + '"><wp:extent cx="914400" cy="914400"/>' +
      `<a:graphic xmlns:a="${A}"><a:graphicData uri="${WPS}">` +
      `<wps:wsp xmlns:wps="${WPS}"><wps:spPr><a:blipFill><a:blip r:embed="rIdGone1"/></a:blipFill></wps:spPr>` +
      '<wps:txbx r:txbx="rIdGone2"/></wps:wsp>' +
      '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>' +
      `<w:r><w:pict><v:shape xmlns:v="${V}" id="s1" type="#_x0000_t75" style="width:50pt;height:50pt">` +
      '<v:imagedata r:id="rIdGone3"/></v:shape></w:pict></w:r></w:p>'
    emit(
      'drawing-missing-rels',
      await real.buildDocx({ bodyXml: dangling + PLAIN }),
      'parses; every dangling r:id degrades to no media, bytes untouched',
    )
    // 畸形 style / coordsize / path / 颜色：一个都不能让投影 panic
    const junk =
      `<w:p><w:r><w:pict><v:group xmlns:v="${V}" style="width:abc;height:;left:1e999" coordsize="not,numbers">` +
      '<v:shape id="j1" style=";;;width:--3pt;height:0pt;margin-left:NaNpt;position:ABSOLUTE"' +
      ' fillcolor="#zzzzzz" strokecolor="" strokeweight="-1pt" path="m0,0c1">' +
      '<v:textbox><w:txbxContent><w:p><w:r><w:t>junk</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape>' +
      `<v:rect id="j2" style="width:10;height:10" path="m,l21600,,21600,21600,,21600nfxe" coordsize="-1,-1"/>` +
      '</v:group></w:pict></w:r></w:p>'
    emit(
      'drawing-bad-style',
      await real.buildDocx({ bodyXml: junk + PLAIN }),
      'parses; unparsable style/coordsize/path values are dropped, not guessed',
    )
  })

  it('header/footer and section damage (TEST-09, MOD-10, PROP-09)', async () => {
    // 引用了 rels 里根本不存在的 rId：节的槽必须当"没声明"，hfParts 不能出现悬空条目
    emit(
      'hf-dangling-reference',
      await real.buildDocx({
        bodyXml: PLAIN,
        sectPrExtra:
          '<w:headerReference w:type="default" r:id="rIdGoneHdr"/>' +
          '<w:footerReference w:type="first" r:id="rIdGoneFtr"/>',
      }),
      'PKG_REL_MISSING; the slot reads as undeclared and hfParts has no dangling entry',
    )

    // header part 是二进制垃圾：整 part 降级为 Opaque，正文照旧可编辑
    const binaryHeader = await real.buildDocx({
      bodyXml: PLAIN,
      sectPrExtra: '<w:headerReference w:type="default" r:id="rIdHdrBin"/>',
      extraRels: `<Relationship Id="rIdHdrBin" Type="${REL_T}/header" Target="header1.xml"/>`,
      extraParts: [
        { path: 'word/header1.xml', xml: '\u0000\u0001\u0002 not xml at all \u00ff', contentType: CT_HDR },
      ],
    })
    emit(
      'hf-part-binary',
      binaryHeader,
      'PKG_OPAQUE_PART; the header part is Opaque, SetHeaderFooter on it is Err, body still editable',
    )

    // sectPr 的每个值都不合法：几何回退缺省，每处记一条 PROP_BAD_VALUE。
    // 这一份手搭 zip——`buildDocx` 会在 `sectPrExtra` 之后补一个合法的 `w:pgSz`，
    // 那就测不到"尺寸不可解析时怎么办"了
    const badSect =
      XML_DECL +
      `<w:document xmlns:w="${W}" xmlns:r="${R}"><w:body>` +
      PLAIN +
      '<w:sectPr><w:type w:val="weird"/><w:pgSz w:w="abc" w:h="-1"/>' +
      '<w:pgNumType w:start="x"/><w:cols w:num="0"/><w:titlePg w:val="maybe"/>' +
      '</w:sectPr></w:body></w:document>'
    emit(
      'sectpr-bad-values',
      await zipOf({
        '[Content_Types].xml': contentTypes([['/word/document.xml', CT_MAIN]]),
        '_rels/.rels': rels([['rId1', `${REL_T}/officeDocument`, 'word/document.xml']]),
        'word/document.xml': badSect,
        'word/_rels/document.xml.rels': rels([]),
      }),
      'PROP_BAD_VALUE per bad value; page geometry falls back to defaults; unedited save byte-identical',
    )

    // 页眉里 3000 层文本框套娃：投影必须是迭代的
    const deepTxbx =
      '<w:p><w:r><w:pict>' +
      `<v:shape xmlns:v="${V}" id="deep" type="#_x0000_t202" style="width:100pt;height:100pt">` +
      `<v:textbox>${nestedTxbx(3000)}</v:textbox>` +
      '</v:shape></w:pict></w:r></w:p>'
    const deepHeader = await real.buildDocx({
      bodyXml: PLAIN,
      sectPrExtra: '<w:headerReference w:type="default" r:id="rIdHdrDeep"/>',
      extraRels: `<Relationship Id="rIdHdrDeep" Type="${REL_T}/header" Target="header1.xml"/>`,
      extraParts: [
        {
          path: 'word/header1.xml',
          xml: `${XML_DECL}<w:hdr xmlns:w="${W}" xmlns:r="${R}">${deepTxbx}</w:hdr>`,
          contentType: CT_HDR,
        },
      ],
    })
    emit(
      'hf-deep-txbx',
      deepHeader,
      'MOD_TOO_DEEP; the header projection stays iterative and does not overflow the stack',
    )
  })

  it('writes hostile manifest', () => {
    writeFileSync(join(HOSTILE!, 'manifest.json'), JSON.stringify(manifest, null, 2))
    expect(manifest.length).toBeGreaterThanOrEqual(24)
  })
})

describe('M6 嵌入对象病态输入（任务书 §7）', () => {
  const CHART_CT = 'application/vnd.openxmlformats-officedocument.drawingml.chart+xml'
  const DGM = 'http://schemas.openxmlformats.org/drawingml/2006/diagram'

  it('chart part 标签不闭合', async () => {
    // chart part 里 <c:chart> 不闭合：文档本身完好，图表 part 应整 part 降级
    const malformed = real.CHART_PART_XML.replace('</c:chart></c:chartSpace>', '</c:chartSpace>')
    emit(
      'chart-part-malformed',
      await real.buildDocx({
        bodyXml: real.CHART_PARAGRAPH_XML + PLAIN,
        extraRels: real.CHART_RELS,
        extraParts: [{ path: 'word/charts/chart1.xml', xml: malformed, contentType: CHART_CT }],
      }),
      'parses; chart block without chartDisplay; PKG_OPAQUE_PART; unedited save byte-identical',
    )
  })

  it('c:chart r:id 悬空；另一段 cx:chart 无 Fallback', async () => {
    // chartex（cx:chart）直接裸露在 graphicData 里，没有 mc:AlternateContent 的 Fallback 图片
    const chartexP =
      '<w:p><w:r><w:drawing><wp:inline><wp:extent cx="5486400" cy="3200400"/>' +
      `<a:graphic xmlns:a="${A}"><a:graphicData uri="http://schemas.microsoft.com/office/drawing/2014/chartex">` +
      `<cx:chart xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex" xmlns:r="${R}" r:id="rId31"/>` +
      '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'
    emit(
      'chart-missing-rel',
      await real.buildDocx({ bodyXml: real.CHART_PARAGRAPH_XML + chartexP + PLAIN }),
      'parses; PKG_REL_MISSING; no chartDisplay',
    )
  })

  it('SmartArt cxn 成环 + 自指 + srcOrd 缺失 + 5000 个 dgm:pt', async () => {
    const pts: string[] = []
    for (let i = 0; i < 5000; i++) {
      pts.push(
        `<dgm:pt modelId="{${i}}"><dgm:t><a:bodyPr/><a:p><a:r><a:t>节点${i}</a:t></a:r></a:p></dgm:t></dgm:pt>`,
      )
    }
    const cxns =
      // A→B→A 成环
      `<dgm:cxn modelId="{c1}" dgm:type="parOf" dgm:srcId="{0}" dgm:destId="{1}" dgm:srcOrd="0" dgm:dstOrd="0"/>` +
      `<dgm:cxn modelId="{c2}" dgm:type="parOf" dgm:srcId="{1}" dgm:destId="{0}" dgm:srcOrd="0" dgm:dstOrd="0"/>` +
      // 自指
      `<dgm:cxn modelId="{c3}" dgm:type="parOf" dgm:srcId="{2}" dgm:destId="{2}" dgm:srcOrd="0" dgm:dstOrd="0"/>` +
      // srcOrd 缺失
      `<dgm:cxn modelId="{c4}" dgm:type="parOf" dgm:srcId="{3}" dgm:destId="{4}" dgm:dstOrd="0"/>`
    const dataXml =
      XML_DECL +
      `<dgm:dataModel xmlns:dgm="${DGM}" xmlns:a="${A}">` +
      `<dgm:ptLst>${pts.join('')}</dgm:ptLst><dgm:cxnLst>${cxns}</dgm:cxnLst></dgm:dataModel>`
    const smartartP =
      '<w:p><w:r><w:drawing><wp:inline><wp:extent cx="5486400" cy="419100"/>' +
      `<a:graphic xmlns:a="${A}"><a:graphicData uri="${DGM}">` +
      `<dgm:relIds xmlns:dgm="${DGM}" xmlns:r="${R}" r:dm="rId40" r:lo="rId41" r:qs="rId42" r:cs="rId43"/>` +
      '</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'
    emit(
      'diagram-cyclic-cxn',
      await real.buildDocx({
        bodyXml: smartartP + PLAIN,
        extraRels: `<Relationship Id="rId40" Type="${REL_T}/diagramData" Target="diagrams/data1.xml"/>`,
        extraParts: [
          {
            path: 'word/diagrams/data1.xml',
            xml: dataXml,
            contentType: 'application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml',
          },
        ],
      }),
      'parses without hanging; all texts kept once',
    )
  })

  it('lockedCanvas 退化几何', async () => {
    // chExt 为 0 与负数（缩放除零/负缩放）、坐标 1e30、字号 sz="-5"、a:pic 无 a:blip
    const canvas =
      '<w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/>' +
      `<a:graphic xmlns:a="${A}"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas">` +
      '<lc:lockedCanvas xmlns:lc="http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas">' +
      '<a:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/>' +
      '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="-914400"/></a:xfrm></a:grpSpPr>' +
      '<a:sp><a:nvSpPr><a:cNvPr id="2" name="s2"/><a:cNvSpPr/><a:nvPr/></a:nvSpPr>' +
      '<a:spPr><a:xfrm><a:off x="1e30" y="-1e30"/><a:ext cx="-500" cy="0"/></a:xfrm>' +
      '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></a:spPr>' +
      '<a:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="zh-CN" sz="-5"/><a:t>退化</a:t></a:r></a:p></a:txBody></a:sp>' +
      '<a:pic><a:nvPicPr><a:cNvPr id="3" name="p3"/><a:cNvPicPr/><a:nvPr/></a:nvPicPr>' +
      '<a:blipFill><a:stretch><a:fillRect/></a:stretch></a:blipFill>' +
      '<a:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></a:xfrm>' +
      '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></a:spPr></a:pic>' +
      '</lc:lockedCanvas></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>'
    emit(
      'canvas-degenerate',
      await real.buildDocx({ bodyXml: canvas + PLAIN }),
      'parses; MOD_BAD_GEOMETRY; no non-finite numbers in output',
    )
  })

  it('OMML 3000 层分数套娃', async () => {
    const deep =
      '<m:f><m:num>'.repeat(3000) +
      '<m:r><m:t>x</m:t></m:r>' +
      '</m:num></m:f>'.repeat(3000)
    emit(
      'omml-deep',
      await real.buildDocx({ bodyXml: `<w:p><m:oMath>${deep}</m:oMath></w:p>` + PLAIN }),
      'parses; MOD_TOO_DEEP; no stack overflow',
    )
  })

  it('aidocs-ink run：悬空 r:embed、descr 实体、posOffset 非数字', async () => {
    const inkRun =
      '<w:r><w:drawing>' +
      `<wp:anchor xmlns:wp="${WP}" distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="251658247" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">` +
      '<wp:simplePos x="0" y="0"/>' +
      '<wp:positionH relativeFrom="column"><wp:posOffset>abc</wp:posOffset></wp:positionH>' +
      '<wp:positionV relativeFrom="paragraph"><wp:posOffset>-abc</wp:posOffset></wp:positionV>' +
      '<wp:extent cx="1000" cy="800"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/>' +
      '<wp:docPr id="7" name="aidocs-ink 7" descr="{&quot;strokes&quot;:[{&quot;tool&quot;:&quot;pen&amp;ink&quot;}]}"/>' +
      '<wp:cNvGraphicFramePr/>' +
      `<a:graphic xmlns:a="${A}"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">` +
      '<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">' +
      '<pic:nvPicPr><pic:cNvPr id="7" name="aidocs-ink 7"/><pic:cNvPicPr/></pic:nvPicPr>' +
      '<pic:blipFill><a:blip r:embed="rIdGone"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>' +
      '<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1000" cy="800"/></a:xfrm>' +
      '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>' +
      '</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>'
    emit(
      'ink-garbage',
      await real.buildDocx({
        bodyXml: `<w:p>${inkRun}<w:r><w:t>批注段</w:t></w:r></w:p>` + PLAIN,
      }),
      'parses; inks[0].dataUrl null; payload decoded; offsets 0',
    )
  })

  // ---- M7（`spec/18` 7.0⑤）：修订 / 分节 / 绘图的病态输入 ----

  it('修订包裹 500 层套娃（w:ins / w:del 交替）', async () => {
    const opens: string[] = []
    const closes: string[] = []
    for (let i = 0; i < 500; i++) {
      const tag = i % 2 === 0 ? 'w:ins' : 'w:del'
      opens.push(`<${tag} w:id="${2000 + i}" w:author="A${i % 3}" w:date="2024-01-01T00:00:0${i % 10}Z">`)
      closes.unshift(`</${tag}>`)
    }
    // 最内层在 w:del 里（i = 499 是奇数），所以文本节点用 w:delText
    const inner = '<w:r><w:delText>deep</w:delText></w:r>'
    emit(
      'rev-nested-wrappers',
      await real.buildDocx({ bodyXml: `<w:p>${opens.join('')}${inner}${closes.join('')}</w:p>` + PLAIN }),
      'parses; 500 nesting levels get the right depth; iterative walk, no stack overflow; unedited save byte-identical',
    )
  })

  it('moveFrom 无孪生 / w:name 对不上 / range 跨段', async () => {
    const D = 'w:author="搬运工" w:date="2024-02-02T02:02:02Z"'
    // ① 只有 moveFrom 一半，全文没有任何 moveTo
    const lonely =
      `<w:p><w:moveFromRangeStart w:id="1" w:name="move-a" ${D}/>` +
      `<w:moveFrom w:id="2" ${D}><w:r><w:delText>被搬走但没有落点</w:delText></w:r></w:moveFrom>` +
      '<w:moveFromRangeEnd w:id="1"/></w:p>'
    // ② moveTo 的 w:name 与任何 moveFrom 都不同
    const mismatched =
      `<w:p><w:moveToRangeStart w:id="3" w:name="move-b" ${D}/>` +
      `<w:moveTo w:id="4" ${D}><w:r><w:t>落点，但名字对不上</w:t></w:r></w:moveTo>` +
      '<w:moveToRangeEnd w:id="3"/></w:p>'
    // ③ 范围标记跨三段：start 在第一段、end 在第三段
    const crossStart =
      `<w:p><w:moveFromRangeStart w:id="5" w:name="cross" ${D}/>` +
      `<w:moveFrom w:id="6" ${D}><w:r><w:delText>跨段一</w:delText></w:r></w:moveFrom></w:p>`
    const crossMid = `<w:p><w:moveFrom w:id="7" ${D}><w:r><w:delText>跨段二</w:delText></w:r></w:moveFrom></w:p>`
    const crossEnd =
      `<w:p><w:moveFrom w:id="8" ${D}><w:r><w:delText>跨段三</w:delText></w:r></w:moveFrom>` +
      '<w:moveFromRangeEnd w:id="5"/></w:p>'
    emit(
      'rev-move-unpaired',
      await real.buildDocx({ bodyXml: lonely + mismatched + crossStart + crossMid + crossEnd + PLAIN }),
      'parses; REV_UNPAIRED_MOVE for the three unpaired halves; pair = None; AcceptAll / RejectAll succeed',
    )
  })

  it('*PrChange 没有内层容器 / 有多个内层容器', async () => {
    const D = 'w:author="改格式的" w:date="2024-03-03T03:03:03Z"'
    // rPrChange：一个空的、一个两层 w:rPr
    const runs =
      `<w:p><w:r><w:rPr><w:b/><w:rPrChange w:id="10" ${D}/></w:rPr><w:t>空 rPrChange</w:t></w:r>` +
      `<w:r><w:rPr><w:i/><w:rPrChange w:id="11" ${D}><w:rPr><w:u w:val="single"/></w:rPr>` +
      '<w:rPr><w:strike/></w:rPr></w:rPrChange></w:rPr><w:t>两个 rPr</w:t></w:r></w:p>'
    // pPrChange：一个空的、一个两层 w:pPr
    const paras =
      `<w:p><w:pPr><w:jc w:val="center"/><w:pPrChange w:id="12" ${D}/></w:pPr>` +
      '<w:r><w:t>空 pPrChange</w:t></w:r></w:p>' +
      `<w:p><w:pPr><w:pPrChange w:id="13" ${D}><w:pPr><w:jc w:val="left"/></w:pPr>` +
      '<w:pPr><w:jc w:val="right"/></w:pPr></w:pPrChange></w:pPr><w:r><w:t>两个 pPr</w:t></w:r></w:p>'
    // tblPrChange：空的；tblGridChange：没有 w:tblGrid
    const table =
      `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblPrChange w:id="14" ${D}/></w:tblPr>` +
      `<w:tblGrid><w:gridCol w:w="4000"/><w:tblGridChange w:id="15"/></w:tblGrid>` +
      '<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/>' +
      `<w:tcPrChange w:id="16" ${D}><w:tcPr/><w:tcPr/></w:tcPrChange></w:tcPr>` +
      '<w:p><w:r><w:t>格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>'
    emit(
      'rev-change-empty',
      await real.buildDocx({ bodyXml: runs + paras + table + PLAIN }),
      'parses; empty *PrChange gives an all-default snapshot, multiple inner containers take the first; no EngineInvariantViolation',
    )
  })

  it('w:del 里是 w:t、w:ins 里有 w:delText', async () => {
    const D = 'w:author="错配的" w:date="2024-04-04T04:04:04Z"'
    const body =
      `<w:p><w:del w:id="20" ${D}><w:r><w:t>删除包裹里却是 w:t</w:t></w:r></w:del>` +
      `<w:ins w:id="21" ${D}><w:r><w:delText>插入包裹里却是 w:delText</w:delText></w:r></w:ins></w:p>` +
      // 字段指令的镜像错配：w:del 里是 instrText、w:ins 里是 delInstrText
      `<w:p><w:del w:id="22" ${D}><w:r><w:fldChar w:fldCharType="begin"/></w:r>` +
      '<w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r></w:del>' +
      `<w:ins w:id="23" ${D}><w:r><w:delInstrText xml:space="preserve"> TIME </w:delInstrText></w:r>` +
      '<w:r><w:fldChar w:fldCharType="end"/></w:r></w:ins></w:p>'
    emit(
      'rev-del-with-t',
      await real.buildDocx({ bodyXml: body + PLAIN }),
      'parses; the mismatched text nodes keep their own kind; unedited save byte-identical',
    )
  })

  it('单元格段落带 sectPr', async () => {
    const cellSect =
      '<w:pPr><w:sectPr w:rsidR="00000000"><w:pgSz w:w="11906" w:h="16838"/>' +
      '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="851" w:footer="992" w:gutter="0"/>' +
      '<w:cols w:space="425"/></w:sectPr></w:pPr>'
    const table =
      '<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>' +
      '<w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>' +
      `<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr><w:p>${cellSect}` +
      '<w:r><w:t>格里有分节符</w:t></w:r></w:p></w:tc>' +
      '<w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>普通格</w:t></w:r></w:p></w:tc>' +
      '</w:tr></w:tbl>'
    emit(
      'sectpr-in-cell',
      await real.buildDocx({ bodyXml: table + PLAIN }),
      'parses; the cell sectPr does not create a Document section; InsertSectionBreak there is EDIT_BAD_POSITION; unedited save byte-identical',
    )
  })

  it('wp:anchor 缺 extent、relativeHeight 溢出、positionH 无子元素', async () => {
    const anchor =
      '<w:p><w:r><w:drawing>' +
      `<wp:anchor xmlns:wp="${WP}" distT="0" distB="0" distL="114300" distR="114300" simplePos="0" ` +
      'relativeHeight="99999999999" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">' +
      '<wp:simplePos x="0" y="0"/>' +
      '<wp:positionH relativeFrom="column"/>' +
      '<wp:positionV relativeFrom="paragraph"><wp:posOffset>635000</wp:posOffset></wp:positionV>' +
      '<wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/>' +
      '<wp:docPr id="31" name="无 extent 的锚"/><wp:cNvGraphicFramePr/>' +
      `<a:graphic xmlns:a="${A}"><a:graphicData uri="${WPS}">` +
      `<wps:wsp xmlns:wps="${WPS}"><wps:cNvPr id="32" name="s32"/><wps:cNvSpPr/>` +
      '<wps:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></wps:spPr>' +
      '<wps:txbx><w:txbxContent><w:p><w:r><w:t>没有尺寸</w:t></w:r></w:p></w:txbxContent></wps:txbx>' +
      '<wps:bodyPr/></wps:wsp>' +
      '</a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>'
    emit(
      'drawing-anchor-no-extent',
      await real.buildDocx({ bodyXml: anchor + PLAIN }),
      'parses; missing extent and empty positionH degrade to 0; relativeHeight beyond u32 does not overflow; unedited save byte-identical',
    )
  })

  it('重写 hostile manifest（含 M6 / M7 各 6 份）', () => {
    // 已有 describe 的 manifest 落盘 it 先于本节执行，这里用完整数组重写一遍
    writeFileSync(join(HOSTILE!, 'manifest.json'), JSON.stringify(manifest, null, 2))
    expect(manifest.length).toBeGreaterThanOrEqual(36)
  })
})
