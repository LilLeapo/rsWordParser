import argparse
import hashlib
import json
import re
import zipfile
from datetime import datetime, timezone
from pathlib import Path

import pdfplumber
import pypdfium2 as pdfium
from lxml import etree


DELIVERY = Path('C:/word/real-word-round2-20260907')
CASES = (
    'blank/blank-new', 'blank/blank-styles-used',
    'revisions2/rev-insert-delete', 'revisions2/rev-move', 'revisions2/rev-format',
    'revisions2/rev-table', 'revisions2/rev-section', 'revisions2/rev-accept-reject',
    'revisions2/rev-comment-threads', 'fields2/fields-seq-captions',
    'fields2/fields-index', 'fields2/fields-toc-stale', 'fields2/fields-citations',
    'fields2/fields-page-in-footer', 'sections2/sections-breaks-zoo',
    'image2/image-z-order', 'shapes2/textbox-linked',
)
NS = {
    'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main',
    'wp': 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing',
    'wps': 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape',
    'w15': 'http://schemas.microsoft.com/office/word/2012/wordml',
    'b': 'http://schemas.openxmlformats.org/officeDocument/2006/bibliography',
}


def xpath(node, expression):
    return node.xpath(expression, namespaces=NS)


def codes(root):
    results, stack = [], []
    for node in root.iter():
        local = etree.QName(node).localname
        if etree.QName(node).namespace != NS['w']:
            continue
        if local == 'fldSimple':
            results.append(node.get('{' + NS['w'] + '}instr', ''))
        elif local == 'fldChar':
            kind = node.get('{' + NS['w'] + '}fldCharType')
            if kind == 'begin':
                stack.append({'reading': True, 'code': ''})
            elif kind == 'separate' and stack:
                stack[-1]['reading'] = False
            elif kind == 'end' and stack:
                results.append(stack.pop()['code'])
        elif local == 'instrText' and stack and stack[-1]['reading']:
            stack[-1]['code'] += node.text or ''
    return results


def inspect_structure(path, case):
    with zipfile.ZipFile(path) as archive:
        bad_crc = archive.testzip()
        names = archive.namelist()
        parts = {name: etree.fromstring(archive.read(name)) for name in names if name.endswith(('.xml', '.rels'))}
    root = parts['word/document.xml']
    text = ''.join(xpath(root, '//w:t/text()'))
    field_codes = codes(root)
    counts = {name: len(xpath(root, '//w:' + name)) for name in (
        'ins', 'del', 'delText', 'moveFrom', 'moveTo', 'moveFromRangeStart', 'moveFromRangeEnd',
        'moveToRangeStart', 'moveToRangeEnd', 'rPrChange', 'pPrChange', 'numPr', 'tcPrChange',
        'tblPrChange', 'sectPrChange', 'sectPr', 'cellMerge', 'commentRangeStart', 'commentRangeEnd',
    )}
    counts.update(rowIns=len(xpath(root, '//w:trPr/w:ins')),
                  rowDel=len(xpath(root, '//w:trPr/w:del')),
                  numberingRevision=len(xpath(root, '//w:pPr[w:numPr and w:pPrChange] | //w:numPr/w:numberingChange | //w:numPr/w:ins')))
    checks = []

    def check(name, passed, actual):
        checks.append({'requirement': name, 'passed': bool(passed), 'actual': actual})

    def count_field(name):
        return sum(bool(re.match(r'^\s*' + name + r'(?:\s|$)', code)) for code in field_codes)

    check('ZIP CRC and unique member names', bad_crc is None and len(names) == len(set(names)),
          {'crcError': bad_crc, 'entries': len(names), 'uniqueEntries': len(set(names))})
    if not case.startswith('blank/'):
        check('before/after markers', 'before \u524d\u6587' in text and 'after \u540e\u6587' in text, text)
    stem = case.split('/')[-1]
    if case.startswith('blank/'):
        body = xpath(root, '/w:document/w:body')[0]
        children = [etree.QName(child).localname for child in body]
        empty = not xpath(body, './/w:t | .//w:delText')
        check('one empty paragraph and final sectPr only', children == ['p', 'sectPr'] and empty, children)
        required = ['word/styles.xml', 'word/settings.xml', 'word/theme/theme1.xml',
                    'word/fontTable.xml', 'word/webSettings.xml']
        check('required standard parts', all(name in names for name in required), {name: name in names for name in required})
        if stem == 'blank-styles-used':
            style_names = xpath(parts['word/styles.xml'], '//w:style/w:name/@w:val')
            for name in ('heading 1', 'heading 2', 'List Paragraph'):
                check('retained used style ' + name, name.lower() in [value.lower() for value in style_names], style_names)
    elif stem == 'rev-insert-delete':
        authors = sorted(set(xpath(root, '//w:ins/@w:author | //w:del/@w:author')))
        check('two insertions, deletion with text, two authors', counts['ins'] == 2 and counts['del'] >= 1
              and counts['delText'] >= 1 and len(authors) == 2, {'counts': counts, 'authors': authors})
    elif stem == 'rev-move':
        for name in ('moveFrom', 'moveTo', 'moveFromRangeStart', 'moveFromRangeEnd', 'moveToRangeStart', 'moveToRangeEnd'):
            check('native ' + name, counts[name] >= 1, counts[name])
        for prefix in ('moveFrom', 'moveTo'):
            starts = sorted(xpath(root, '//w:' + prefix + 'RangeStart/@w:id'))
            ends = sorted(xpath(root, '//w:' + prefix + 'RangeEnd/@w:id'))
            check(prefix + ' range IDs paired', bool(starts) and starts == ends, {'start': starts, 'end': ends})
    elif stem == 'rev-format':
        for name in ('rPrChange', 'pPrChange', 'numberingRevision'):
            check(name, counts[name] >= 1, counts[name])
    elif stem == 'rev-table':
        for name in ('rowIns', 'rowDel', 'tcPrChange', 'tblPrChange'):
            check(name, counts[name] >= 1, counts[name])
        spans = xpath(root, '//w:gridSpan/@w:val')
        check('merged cell spans two columns', '2' in spans, spans)
    elif stem == 'rev-section':
        check('section property change', counts['sectPrChange'] >= 1, counts['sectPrChange'])
    elif stem == 'rev-accept-reject':
        check('only pending insertion remains', counts['ins'] == 1, counts['ins'])
        check('accepted kept, rejected absent, pending kept', 'ACCEPTED-INSERT' in text
              and 'REJECTED-INSERT' not in text and 'PENDING-INSERT' in text, text)
    elif stem == 'rev-comment-threads':
        comments = parts.get('word/comments.xml')
        extended = parts.get('word/commentsExtended.xml')
        comment_count = len(xpath(comments, '//w:comment')) if comments is not None else 0
        parents = xpath(extended, '//@w15:paraIdParent') if extended is not None else []
        resolved = xpath(extended, '//*[@w15:done="1" or @w15:done="true"]') if extended is not None else []
        parent_map = {item.get('{' + NS['w15'] + '}paraId'): item.get('{' + NS['w15'] + '}paraIdParent')
                      for item in extended} if extended is not None else {}
        depths = []
        for paragraph in parent_map:
            depth, current, visited = 0, paragraph, set()
            while parent_map.get(current) and current not in visited:
                visited.add(current)
                current = parent_map[current]
                depth += 1
            depths.append(depth)
        check('five comments', comment_count == 5, comment_count)
        check('two reply parent links', len(parents) >= 2, parents)
        check('resolved thread', bool(resolved), len(resolved))
        check('three root comment threads', sum(value is None for value in parent_map.values()) == 3, parent_map)
        check('two nested reply levels', max(depths, default=0) >= 2,
              {'parentMap': parent_map, 'maximumReplyDepth': max(depths, default=0),
               'documentAnchorCount': counts['commentRangeStart']})
    elif stem == 'fields-seq-captions':
        figures = [code for code in field_codes if re.match(r'^\s*SEQ\s+\u56fe(?:\s|$)', code)]
        check('three SEQ figure captions', len(figures) == 3, figures)
        check('cross-reference field', count_field('REF') >= 1, field_codes)
    elif stem == 'fields-index':
        check('three XE and one INDEX', count_field('XE') == 3 and count_field('INDEX') == 1, field_codes)
    elif stem == 'fields-toc-stale':
        check('TOC present', count_field('TOC') == 1, field_codes)
        check('two old cached titles and two changed headings', all(value in text for value in (
            'Original heading one', 'Original heading two', 'Changed heading one', 'Changed heading two')), text)
        references = []
        for code in field_codes:
            match = re.match(r'^\s*PAGEREF\s+(?:"([^"]+)"|(\S+))', code, re.IGNORECASE)
            if match:
                references.append(match.group(1) or match.group(2))
        bookmarks = {node.get('{' + NS['w'] + '}name'): node.get('{' + NS['w'] + '}id')
                     for node in xpath(root, '//w:bookmarkStart')}
        bookmark_ends = set(xpath(root, '//w:bookmarkEnd/@w:id'))
        missing = sorted({name for name in references
                          if name not in bookmarks or bookmarks[name] not in bookmark_ends})
        check('three TOC page references have paired bookmark targets', len(references) == 3 and not missing,
              {'references': references, 'bookmarks': bookmarks, 'missingOrUnpairedTargets': missing})
    elif stem == 'fields-citations':
        sources = [(name, item) for name, item in parts.items() if name.startswith('customXml/')
                   and etree.QName(item).localname == 'Sources' and etree.QName(item).namespace == NS['b']]
        types = [value for _, item in sources for value in xpath(item, '//b:Source/b:SourceType/text()')]
        check('two citations and bibliography', count_field('CITATION') == 2 and count_field('BIBLIOGRAPHY') == 1, field_codes)
        check('embedded book and journal sources', sorted(types) == ['Book', 'JournalArticle'],
              {'parts': [name for name, _ in sources], 'types': types})
    elif stem == 'fields-page-in-footer':
        footer_codes = [code for name, item in parts.items() if re.match(r'word/footer\d+\.xml$', name) for code in codes(item)]
        check('PAGE and NUMPAGES in footer', all(any(re.match(r'^\s*' + keyword + r'(?:\s|$)', code)
              for code in footer_codes) for keyword in ('PAGE', 'NUMPAGES')), footer_codes)
    elif stem == 'sections-breaks-zoo':
        sections = xpath(root, '//w:sectPr[not(ancestor::w:sectPrChange)]')
        types = [xpath(section, 'w:type/@w:val')[0] if xpath(section, 'w:type/@w:val') else 'nextPage' for section in sections]
        check('five sections', len(sections) == 5, len(sections))
        check('next, continuous, even, odd breaks', all(value in types for value in ('nextPage', 'continuous', 'evenPage', 'oddPage')), types)
        third = sections[2] if len(sections) >= 3 else None
        check('third section different first page and first header', third is not None
              and bool(xpath(third, 'w:titlePg')) and bool(xpath(third, 'w:headerReference[@w:type="first"]')),
              etree.tostring(third, encoding='unicode') if third is not None else None)
    elif stem == 'image-z-order':
        anchors = [{'name': xpath(anchor, 'wp:docPr/@name')[0], 'relativeHeight': int(anchor.get('relativeHeight'))}
                   for anchor in xpath(root, '//wp:anchor')]
        heights = {item['name']: item['relativeHeight'] for item in anchors}
        check('three anchors', len(anchors) == 3, anchors)
        check('picture 2 behind 1 behind 3', all(name in heights for name in ('Round2 Picture 1', 'Round2 Picture 2', 'Round2 Picture 3'))
              and heights.get('Round2 Picture 2', 0) < heights.get('Round2 Picture 1', 0) < heights.get('Round2 Picture 3', 0), anchors)
    elif stem == 'textbox-linked':
        start = xpath(root, '//wps:txbx[@id]')
        follow = xpath(root, '//wps:linkedTxbx')
        check('native first and linked textboxes', len(start) == 1 and len(follow) == 1,
              [etree.tostring(item, encoding='unicode') for item in start + follow])
        check('link ID matches and continuation sequence is 1', bool(start) and bool(follow)
              and start[0].get('id') == follow[0].get('id') and follow[0].get('seq') == '1',
              [dict(item.attrib) for item in start + follow])
    return {'passed': all(item['passed'] for item in checks), 'checks': checks, 'counts': counts,
            'fieldCodes': field_codes, 'fullText': text, 'xmlPartsParsed': len(parts)}


parser = argparse.ArgumentParser()
parser.add_argument('--source-root', type=Path, default=DELIVERY)
parser.add_argument('--output', type=Path, default=DELIVERY / '_readouts/task-d-pdf-review.json')
parser.add_argument('--case', action='append', choices=CASES, help='Inspect only this case; repeat for multiple cases.')
parser.add_argument('--png-root', type=Path, default=DELIVERY / 'screenshots/task-d-pdf')
parser.add_argument('--annotations', type=Path, default=DELIVERY / '_scripts/task-d-pdf-visual.json')
parser.add_argument('--no-pdf', action='store_true')
args = parser.parse_args()
selected_cases = args.case or CASES
previous = json.loads(args.output.read_text(encoding='utf-8-sig')) if args.output.exists() else {}
previous_rows = {row['case']: row for row in previous.get('rows', [])}
annotation_path = args.annotations
annotations = json.loads(annotation_path.read_text(encoding='utf-8-sig')) if annotation_path.exists() else {}
rows = []
for case in selected_cases:
    path = args.source_root / (case + '.docx')
    row = {'case': case, 'source': str(path), 'exists': path.exists(), 'visualReviewed': False}
    if path.exists():
        try:
            row['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest().upper()
            row['structure'] = inspect_structure(path, case)
        except Exception as error:
            row['error'] = str(error)
    pdf_path = args.source_root / '_previews' / (case + '.pdf')
    if not args.no_pdf and pdf_path.exists():
        row['pdf'] = str(pdf_path)
        row['pdfSha256'] = hashlib.sha256(pdf_path.read_bytes()).hexdigest().upper()
        old = previous_rows.get(case, {})
        if old.get('pdfSha256') == row['pdfSha256'] and all((DELIVERY / page['png']).exists() for page in old.get('pages', [])):
            row['pageCount'] = old['pageCount']
            row['pages'] = old['pages']
            annotation = annotations.get(case)
            if annotation and annotation.get('pdfSha256') == row['pdfSha256']:
                row['visualReview'] = annotation
                row['visualReviewed'] = sorted(annotation.get('pageNumbers', [])) == list(range(1, row['pageCount'] + 1))
            rows.append(row)
            continue
        document = pdfium.PdfDocument(pdf_path)
        text_document = pdfplumber.open(pdf_path)
        row['pageCount'] = len(document)
        row['pages'] = []
        png_root = args.png_root
        png_root.mkdir(parents=True, exist_ok=True)
        for number, page in enumerate(document, 1):
            png = png_root / (case.replace('/', '--') + '-page-' + str(number) + '.png')
            bitmap = page.render(scale=1.25)
            image = bitmap.to_pil().convert('RGB')
            image.save(png)
            pixels = image.get_flattened_data() if hasattr(image, 'get_flattened_data') else image.getdata()
            nonwhite = sum(min(pixel) < 245 for pixel in pixels)
            text_page = text_document.pages[number - 1]
            row['pages'].append({'page': number, 'png': str(png.relative_to(DELIVERY)).replace('\\', '/'),
                                 'sizePt': [text_page.width, text_page.height], 'text': text_page.extract_text(),
                                 'nonwhitePixelCount': nonwhite, 'totalPixels': image.width * image.height})
            bitmap.close()
            page.close()
        document.close()
        text_document.close()
        annotation = annotations.get(case)
        if annotation and annotation.get('pdfSha256') == row['pdfSha256']:
            row['visualReview'] = annotation
            row['visualReviewed'] = sorted(annotation.get('pageNumbers', [])) == list(range(1, row['pageCount'] + 1))
    rows.append(row)
report = {'reviewedAt': datetime.now(timezone.utc).isoformat(), 'sourceRoot': str(args.source_root),
          'method': 'Read-only ZIP CRC, lxml namespace-aware structural inspection and optional Word-exported PDF rendering/text extraction. No Word calls or DOCX changes. visualReviewed remains false until rendered pages are independently viewed.',
          'requiredCases': len(selected_cases), 'existingFiles': sum(row['exists'] for row in rows),
          'structurePassed': sum(row.get('structure', {}).get('passed') is True for row in rows),
          'pdfsAvailable': sum('pdf' in row for row in rows), 'rows': rows}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding='utf-8')
print(json.dumps({'report': str(args.output), 'existingFiles': report['existingFiles'],
                  'structurePassed': report['structurePassed'], 'pdfsAvailable': report['pdfsAvailable'],
                  'failures': [{'case': row['case'], 'error': row.get('error'),
                                'checks': [item for item in row.get('structure', {}).get('checks', []) if not item['passed']]}
                               for row in rows if row.get('error') or (row.get('structure') and not row['structure']['passed'])]},
                 ensure_ascii=True, indent=2))
