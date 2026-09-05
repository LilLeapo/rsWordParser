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

  it('writes hostile manifest', () => {
    writeFileSync(join(HOSTILE!, 'manifest.json'), JSON.stringify(manifest, null, 2))
    expect(manifest.length).toBeGreaterThanOrEqual(20)
  })
})
