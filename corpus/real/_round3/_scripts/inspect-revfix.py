"""Read-only structural and PDF inspection of native Word Task A outputs."""

import argparse
import hashlib
import json
import posixpath
import zipfile
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

from lxml import etree


DELIVERY = Path('C:/word/real-word-round3-20260907')
TRACKED_CASES = ('run-edits', 'para-split-merge', 'table-and-move', 'tracked-two-authors')
PAIR_CASES = ('sect-insert', 'sect-delete', 'z-order', 'move-resize')
CASES = TRACKED_CASES + PAIR_CASES
NS = {
    'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main',
    'wp': 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
    'rel': 'http://schemas.openxmlformats.org/package/2006/relationships',
}
W = '{' + NS['w'] + '}'
REVISION_NAMES = (
    'ins', 'del', 'moveFrom', 'moveTo', 'moveFromRangeStart', 'moveFromRangeEnd',
    'moveToRangeStart', 'moveToRangeEnd', 'rPrChange', 'pPrChange', 'tblPrChange',
    'tblGridChange', 'trPrChange', 'tcPrChange', 'sectPrChange', 'numberingChange',
    'cellIns', 'cellDel', 'cellMerge', 'customXmlInsRangeStart', 'customXmlInsRangeEnd',
    'customXmlDelRangeStart', 'customXmlDelRangeEnd', 'customXmlMoveFromRangeStart',
    'customXmlMoveFromRangeEnd', 'customXmlMoveToRangeStart', 'customXmlMoveToRangeEnd',
)
AUTHOR_A = '\u4f5c\u8005\u7532'
AUTHOR_B = '\u4f5c\u8005\u4e59'
BEFORE = 'before \u524d\u6587'
AFTER = 'after \u540e\u6587'
RUN_ORIGINAL = '\u7b2c\u4e00\u53e5\u539f\u6587\u3002\u7b2c\u4e8c\u53e5\u539f\u6587\u3002\u7b2c\u4e09\u53e5\u539f\u6587\u3002'
RUN_ACCEPTED = '\u7b2c\u4e00\u53e5\u539f\u6587\u3002\u65b0\u589e\u7684\u4e00\u53e5\u3002\u7b2c\u4e09\u53e5\u539f\u6587\u3002'
PARA_A = '\u7532\u6bb5\u843d\u7684\u6587\u5b57\u3002'
PARA_B = '\u4e59\u6bb5\u843d\u7684\u6587\u5b57\u3002'
ORIGINAL_BODY = '\u539f\u59cb\u6b63\u6587\u3002'
MOVABLE = '\u53ef\u79fb\u52a8\u6bb5\u843d\u3002'


def xp(node, expression):
    return node.xpath(expression, namespaces=NS) if node is not None else []


def sha256(data):
    return hashlib.sha256(data).hexdigest().upper()


def on(node):
    return node is not None and node.get(W + 'val', 'true') not in ('0', 'false', 'off')


def first_attr(node, expression, default=None):
    values = xp(node, expression)
    return values[0] if values else default


def hidden(node, view='accepted'):
    excluded = ('del', 'moveFrom') if view == 'accepted' else ('ins', 'moveTo')
    return any(parent.tag in (W + name for name in excluded) for parent in node.iterancestors())


def node_text(node, view='accepted'):
    pieces = []
    for item in node.iter():
        if hidden(item, view):
            continue
        if item.tag in (W + 't', W + 'delText'):
            pieces.append(item.text or '')
        elif item.tag == W + 'tab':
            pieces.append('\t')
        elif item.tag in (W + 'br', W + 'cr'):
            pieces.append('\n')
    return ''.join(pieces)


def paragraph_texts(root, view='accepted'):
    """Project text wrappers and paragraph-mark changes; table cells stay separate."""
    result, pending, pending_parent = [], '', None
    for paragraph in xp(root, '/w:document/w:body/w:p'):
        text = node_text(paragraph, view)
        parent = paragraph.getparent()
        if pending and pending_parent is not parent:
            result.append(pending)
            pending = ''
        pending += text
        pending_parent = parent
        removed_mark = 'del' if view == 'accepted' else 'ins'
        if not xp(paragraph, 'w:pPr/w:rPr/w:' + removed_mark):
            result.append(pending)
            pending = ''
    if pending:
        result.append(pending)
    return result


def property_dict(node):
    if node is None:
        return {}
    return {etree.QName(child).localname:
            {etree.QName(key).localname: value for key, value in child.attrib.items()}
            for child in node if not etree.QName(child).localname.endswith('Change')}


def run_format(run, paragraph, styles):
    current = {'bold': False, 'color': 'auto'}
    sources = []

    def apply(rpr, label, style=False):
        if rpr is None:
            return
        b = rpr.find(W + 'b')
        color = rpr.find(W + 'color')
        if b is not None:
            current['bold'] = (not current['bold']) if style and on(b) else (current['bold'] if style else on(b))
        if color is not None:
            current['color'] = color.get(W + 'val', 'auto')
        if b is not None or color is not None:
            sources.append({'source': label, 'properties': property_dict(rpr)})

    def apply_style(style_id, visited=None):
        visited = set() if visited is None else visited
        if not style_id or style_id in visited:
            return
        visited.add(style_id)
        matches = xp(styles, 'w:style[@w:styleId=' + json.dumps(style_id) + ']')
        if not matches:
            return
        style = matches[0]
        apply_style(first_attr(style, 'w:basedOn/@w:val'), visited)
        apply(style.find(W + 'rPr'), 'style:' + style_id, True)

    default_rpr = xp(styles, 'w:docDefaults/w:rPrDefault/w:rPr')
    apply(default_rpr[0] if default_rpr else None, 'documentDefaults')
    default_id = first_attr(styles, 'w:style[@w:type="paragraph" and @w:default="1"]/@w:styleId')
    apply_style(first_attr(paragraph, 'w:pPr/w:pStyle/@w:val', default_id))
    apply_style(first_attr(run, 'w:rPr/w:rStyle/@w:val'))
    apply(run.find(W + 'rPr'), 'direct')
    return dict(current, sources=sources)


def inspect_file(path):
    raw = path.read_bytes()
    with zipfile.ZipFile(path) as archive:
        crc = archive.testzip()
        names = archive.namelist()
        parts = {name: etree.fromstring(archive.read(name)) for name in names if name.endswith(('.xml', '.rels'))}
        media = {name: sha256(archive.read(name)) for name in names if name.startswith('word/media/')}
    root = parts['word/document.xml']
    counts, locations, authors = Counter(), [], set()
    for part_name, part in parts.items():
        if not part_name.startswith('word/'):
            continue
        for node in part.iter():
            name = etree.QName(node).localname
            if etree.QName(node).namespace == NS['w'] and name in REVISION_NAMES:
                counts[name] += 1
                author = node.get(W + 'author')
                if author:
                    authors.add(author)
                locations.append({'part': part_name, 'element': name, 'id': node.get(W + 'id'),
                                  'author': author, 'text': node_text(node, 'rejected' if name in ('del', 'moveFrom') else 'accepted')})
    revision_counts = {name: counts[name] for name in REVISION_NAMES}
    runs = []
    paragraphs = []
    for paragraph in xp(root, '/w:document/w:body/w:p'):
        paragraphs.append({'acceptedText': node_text(paragraph), 'rejectedText': node_text(paragraph, 'rejected'),
                           'properties': property_dict(paragraph.find(W + 'pPr')),
                           'markInsertions': len(xp(paragraph, 'w:pPr/w:rPr/w:ins')),
                           'markDeletions': len(xp(paragraph, 'w:pPr/w:rPr/w:del')),
                           'previousProperties': [property_dict(item) for item in xp(paragraph, 'w:pPr/w:pPrChange/w:pPr')]})
        for run in xp(paragraph, './/w:r'):
            text = node_text(run)
            if text and not hidden(run):
                runs.append(dict(text=text, **run_format(run, paragraph, parts.get('word/styles.xml'))))
    tables = []
    for table in xp(root, '/w:document/w:body/w:tbl'):
        rows = []
        for row in xp(table, 'w:tr'):
            rows.append({'inserted': bool(xp(row, 'w:trPr/w:ins')), 'deleted': bool(xp(row, 'w:trPr/w:del')),
                         'cells': [{'text': node_text(cell), 'originalText': node_text(cell, 'rejected'),
                                    'gridSpan': int(first_attr(cell, 'w:tcPr/w:gridSpan/@w:val', '1')),
                                    'properties': property_dict(cell.find(W + 'tcPr')),
                                    'previousProperties': [property_dict(item) for item in xp(cell, 'w:tcPr/w:tcPrChange/w:tcPr')]}
                                   for cell in xp(row, 'w:tc')]})
        tables.append({'gridColumns': len(xp(table, 'w:tblGrid/w:gridCol')), 'rows': rows})
    sections = []
    for section in xp(root, '//w:sectPr[not(ancestor::w:sectPrChange)]'):
        sections.append({'type': first_attr(section, 'w:type/@w:val', 'nextPage'),
                         'orientation': first_attr(section, 'w:pgSz/@w:orient', 'portrait'),
                         'widthTwips': int(first_attr(section, 'w:pgSz/@w:w', '0')),
                         'heightTwips': int(first_attr(section, 'w:pgSz/@w:h', '0')),
                         'margins': property_dict(section).get('pgMar', {})})
    rels = {node.get('Id'): node.get('Target') for node in parts.get('word/_rels/document.xml.rels', [])}
    drawings = []
    for anchor in xp(root, '//wp:anchor'):
        rid = first_attr(anchor, './/a:blip/@r:embed')
        target = rels.get(rid, '')
        media_path = posixpath.normpath(posixpath.join('word', target)) if target else None
        drawings.append({'id': first_attr(anchor, 'wp:docPr/@id'), 'name': first_attr(anchor, 'wp:docPr/@name'),
                         'relativeHeight': int(anchor.get('relativeHeight', '0')),
                         'horizontalBase': first_attr(anchor, 'wp:positionH/@relativeFrom'),
                         'verticalBase': first_attr(anchor, 'wp:positionV/@relativeFrom'),
                         'xEmu': int(first_attr(anchor, 'wp:positionH/wp:posOffset/text()', '0')),
                         'yEmu': int(first_attr(anchor, 'wp:positionV/wp:posOffset/text()', '0')),
                         'widthEmu': int(first_attr(anchor, 'wp:extent/@cx', '0')),
                         'heightEmu': int(first_attr(anchor, 'wp:extent/@cy', '0')),
                         'wrapSquare': bool(xp(anchor, 'wp:wrapSquare')), 'media': media_path,
                         'mediaSha256': media.get(media_path)})
    pairs = {}
    for kind in ('moveFrom', 'moveTo'):
        starts = xp(root, '//w:' + kind + 'RangeStart')
        pairs[kind] = {'startIds': [item.get(W + 'id') for item in starts],
                       'endIds': xp(root, '//w:' + kind + 'RangeEnd/@w:id'),
                       'startNames': [item.get(W + 'name') for item in starts]}
    return {'sha256': sha256(raw), 'bytes': len(raw), 'crcError': crc,
            'duplicateMembers': len(names) - len(set(names)), 'xmlPartsParsed': len(parts),
            'compatibilityModes': xp(parts.get('word/settings.xml'), '//w:compatSetting[@w:name="compatibilityMode"]/@w:val'),
            'trackRevisionsSetting': bool(xp(parts.get('word/settings.xml'), '//w:trackRevisions[not(@w:val="0" or @w:val="false")]')),
            'doNotTrackMovesSetting': any(on(item) for item in xp(parts.get('word/settings.xml'), '//w:doNotTrackMoves')),
            'revisionCounts': revision_counts, 'revisionTotal': sum(counts.values()), 'revisionLocations': locations,
            'authors': sorted(authors), 'delTextCount': len(xp(root, '//w:del/w:r/w:delText | //w:del//w:delText')),
            'rowInsertions': len(xp(root, '//w:trPr/w:ins')), 'rowDeletions': len(xp(root, '//w:trPr/w:del')),
            'paragraphMarkInsertions': len(xp(root, '//w:pPr/w:rPr/w:ins')),
            'paragraphMarkDeletions': len(xp(root, '//w:pPr/w:rPr/w:del')),
            'moveRanges': pairs, 'paragraphsAcceptedProjection': paragraph_texts(root),
            'paragraphsRejectedProjection': paragraph_texts(root, 'rejected'),
            'paragraphs': paragraphs, 'runs': runs, 'tables': tables, 'sections': sections, 'drawings': drawings,
            'allDocPrIds': xp(root, '//wp:docPr/@id'),
            'stylesUsed': sorted(set(xp(root, '//w:pStyle/@w:val | //w:rStyle/@w:val | //w:tblStyle/@w:val'))),
            'mediaHashes': media}


def check(target, requirement, passed, actual, expected=None):
    item = {'requirement': requirement, 'passed': bool(passed), 'actual': actual}
    if expected is not None:
        item['expected'] = expected
    target.append(item)


def normalized_paragraphs(row):
    return [text for text in row['paragraphsAcceptedProjection'] if text]


def check_case(case, files):
    checks = []
    expected_names = ('base', 'tracked', 'accepted', 'rejected') if case in TRACKED_CASES else ('before', 'after')
    check(checks, 'Required native Word files present and structurally readable', all(name in files for name in expected_names), sorted(files), list(expected_names))
    if not all(name in files for name in expected_names):
        return checks
    for name, row in files.items():
        check(checks, name + ': valid ZIP CRC and unique members', row['crcError'] is None and row['duplicateMembers'] == 0,
              {'crcError': row['crcError'], 'duplicateMembers': row['duplicateMembers']})
        if name != 'tracked':
            check(checks, name + ': zero revision markers in all Word XML parts', row['revisionTotal'] == 0, row['revisionCounts'])
    if case in TRACKED_CASES:
        base, tracked, accepted, rejected = (files[name] for name in expected_names)
        expected_authors = [AUTHOR_A, AUTHOR_B] if case == 'tracked-two-authors' else [AUTHOR_A]
        check(checks, 'Tracked revision authors', sorted(tracked['authors']) == sorted(expected_authors), tracked['authors'], expected_authors)
        check(checks, 'Rejected body paragraphs equal base', normalized_paragraphs(rejected) == normalized_paragraphs(base), normalized_paragraphs(rejected), normalized_paragraphs(base))
        check(checks, 'Rejected body character bold/color equal base', character_signature(rejected) == character_signature(base),
              character_signature(rejected), character_signature(base))
        check(checks, 'Accepted body text equals tracked final-view text', normalized_paragraphs(accepted) == [t for t in tracked['paragraphsAcceptedProjection'] if t],
              normalized_paragraphs(accepted), tracked['paragraphsAcceptedProjection'])
        if case in ('run-edits', 'tracked-two-authors'):
            check(checks, 'Tracked text insertion', tracked['revisionCounts']['ins'] > 0, tracked['revisionCounts']['ins'])
            check(checks, 'Tracked deletion contains delText', tracked['revisionCounts']['del'] > 0 and tracked['delTextCount'] > 0,
                  {'del': tracked['revisionCounts']['del'], 'delText': tracked['delTextCount']})
    if case == 'run-edits':
        check(checks, 'Base exact three-paragraph text', normalized_paragraphs(base) == [BEFORE, RUN_ORIGINAL, AFTER], normalized_paragraphs(base))
        check(checks, 'Accepted exact inserted/deleted text', normalized_paragraphs(accepted) == [BEFORE, RUN_ACCEPTED, AFTER], normalized_paragraphs(accepted))
        check(checks, 'Tracked run property change exists', tracked['revisionCounts']['rPrChange'] > 0, tracked['revisionCounts']['rPrChange'])
        keyword = '\u7b2c\u4e09\u53e5'
        for label, row in (('tracked', tracked), ('accepted', accepted)):
            all_text = ''.join(item['text'] for item in row['runs'])
            offset = all_text.find(keyword)
            relevant, current = [], 0
            for run in row['runs']:
                stop = current + len(run['text'])
                if offset >= 0 and current < offset + len(keyword) and stop > offset:
                    relevant.append(run)
                current = stop
            check(checks, label + ': third-sentence keyword bold and red', bool(relevant) and all(run['bold'] and run['color'].upper() == 'FF0000' for run in relevant), relevant)
    elif case == 'para-split-merge':
        check(checks, 'Base exact four-paragraph text', normalized_paragraphs(base) == [BEFORE, PARA_A, PARA_B, AFTER], normalized_paragraphs(base))
        for kind, key in (('insertion', 'paragraphMarkInsertions'), ('deletion', 'paragraphMarkDeletions')):
            check(checks, 'Tracked paragraph-mark ' + kind, tracked[key] > 0, tracked[key])
        check(checks, 'Tracked paragraph property change', tracked['revisionCounts']['pPrChange'] > 0, tracked['revisionCounts']['pPrChange'])
        text = normalized_paragraphs(accepted)
        check(checks, 'Accepted split first paragraph and merged second/after paragraph', len(text) == 4 and text[0] == BEFORE and text[1] + text[2] == PARA_A and text[3] == PARA_B + AFTER, text)
        if len(text) >= 3:
            formatted = [p for p in accepted['paragraphs'] if p['acceptedText'] == text[2]]
            props = formatted[0]['properties'] if formatted else {}
            check(checks, 'Split second paragraph centered with two-character first-line indent', props.get('jc', {}).get('val') == 'center' and props.get('ind', {}).get('firstLineChars') == '200', props)
        base_properties = [p['properties'] for p in base['paragraphs'] if p['acceptedText']]
        rejected_properties = [p['properties'] for p in rejected['paragraphs'] if p['acceptedText']]
        relevant_properties = lambda values: [{key: p.get(key) for key in ('jc', 'ind')} for p in values]
        check(checks, 'Rejected alignment and indentation return to base', relevant_properties(rejected_properties) == relevant_properties(base_properties),
              relevant_properties(rejected_properties), relevant_properties(base_properties))
    elif case == 'table-and-move':
        base_tables = base['tables']
        check(checks, 'Base 2-column 3-row A1/B1 through A3/B3 table', len(base_tables) == 1 and base_tables[0]['gridColumns'] == 2 and
              [[c['text'] for c in r['cells']] for r in base_tables[0]['rows']] == [['A1', 'B1'], ['A2', 'B2'], ['A3', 'B3']], base_tables)
        for label, key in (('row insertion', 'rowInsertions'), ('row deletion', 'rowDeletions')):
            check(checks, 'Tracked ' + label, tracked[key] > 0, tracked[key])
        check(checks, 'Tracked cell property change', tracked['revisionCounts']['tcPrChange'] > 0, tracked['revisionCounts']['tcPrChange'])
        check(checks, 'Move tracking is not disabled in settings', not tracked['doNotTrackMovesSetting'], tracked['doNotTrackMovesSetting'])
        for element in ('moveFrom', 'moveTo', 'moveFromRangeStart', 'moveFromRangeEnd', 'moveToRangeStart', 'moveToRangeEnd'):
            check(checks, 'Tracked native ' + element, tracked['revisionCounts'][element] > 0, tracked['revisionCounts'][element])
        for kind, data in tracked['moveRanges'].items():
            check(checks, kind + ': paired range IDs', bool(data['startIds']) and sorted(data['startIds']) == sorted(data['endIds']), data)
        ranges = tracked['moveRanges']
        check(checks, 'Move source/destination range names paired', bool(ranges['moveFrom']['startNames']) and sorted(ranges['moveFrom']['startNames']) == sorted(ranges['moveTo']['startNames']), ranges)
        accepted_tables = accepted['tables']
        check(checks, 'Accepted three rows; first row spans both columns', len(accepted_tables) == 1 and len(accepted_tables[0]['rows']) == 3 and
              len(accepted_tables[0]['rows'][0]['cells']) == 1 and accepted_tables[0]['rows'][0]['cells'][0]['gridSpan'] == 2, accepted_tables)
        check(checks, 'Accepted last original row removed', not any(value in json.dumps(accepted_tables) for value in ('A3', 'B3')), accepted_tables)
        inserted_rows = [r for table in tracked['tables'] for r in table['rows'] if r['inserted']]
        inserted_text = [cell['text'] for cell in inserted_rows[0]['cells']] if len(inserted_rows) == 1 else None
        check(checks, 'Accepted inserted second row retained and original row two retained third', len(accepted_tables) == 1 and len(accepted_tables[0]['rows']) == 3 and
              [cell['text'] for cell in accepted_tables[0]['rows'][1]['cells']] == inserted_text and
              [cell['text'] for cell in accepted_tables[0]['rows'][2]['cells']] == ['A2', 'B2'], accepted_tables)
        check(checks, 'Rejected table restores original row count and original row two/three', len(rejected['tables']) == 1 and len(rejected['tables'][0]['rows']) == 3 and
              [[cell['text'] for cell in row['cells']] for row in rejected['tables'][0]['rows'][1:]] == [['A2', 'B2'], ['A3', 'B3']], rejected['tables'])
        text = normalized_paragraphs(accepted)
        check(checks, 'Accepted movable paragraph immediately precedes after', MOVABLE in text and text.index(MOVABLE) + 1 < len(text) and text[text.index(MOVABLE) + 1] == AFTER, text)
    elif case == 'tracked-two-authors':
        check(checks, 'Base exact text', normalized_paragraphs(base) == [BEFORE, ORIGINAL_BODY, AFTER], normalized_paragraphs(base))
        inserts = [r for r in tracked['revisionLocations'] if r['element'] == 'ins' and r['text']]
        check(checks, 'Both authors have text insertions', all(any(r['author'] == a for r in inserts) for a in (AUTHOR_A, AUTHOR_B)), inserts)
        deletes = [r for r in tracked['revisionLocations'] if r['element'] == 'del' and r['text']]
        check(checks, 'Author B deletes suffix of original body', any(r['author'] == AUTHOR_B and ORIGINAL_BODY.endswith(r['text']) and len(r['text']) < len(ORIGINAL_BODY) for r in deletes), deletes)
    elif case in ('sect-insert', 'sect-delete'):
        before, after = files['before'], files['after']
        check(checks, 'Section operation preserves concatenated paragraph text', ''.join(normalized_paragraphs(before)) == ''.join(normalized_paragraphs(after)),
              {'before': normalized_paragraphs(before), 'after': normalized_paragraphs(after)})
        if case == 'sect-insert':
            check(checks, 'Before has one section; after has two', len(before['sections']) == 1 and len(after['sections']) == 2, {'before': before['sections'], 'after': after['sections']})
            check(checks, 'New second section landscape and wider than tall', len(after['sections']) == 2 and after['sections'][1]['orientation'] == 'landscape' and
                  after['sections'][1]['widthTwips'] > after['sections'][1]['heightTwips'], after['sections'])
            check(checks, 'New second section begins next page', len(after['sections']) == 2 and after['sections'][1]['type'] == 'nextPage', after['sections'])
        else:
            check(checks, 'Before two sections portrait then landscape with next-page break', len(before['sections']) == 2 and before['sections'][0]['orientation'] == 'portrait' and
                  before['sections'][1]['orientation'] == 'landscape' and before['sections'][1]['type'] == 'nextPage', before['sections'])
            check(checks, 'After contains one section', len(after['sections']) == 1, after['sections'])
    elif case == 'z-order':
        before, after = files['before'], files['after']
        for label, row in (('before', before), ('after', after)):
            check(checks, label + ': three floating pictures with unique docPr IDs', len(row['drawings']) == 3 and len(row['allDocPrIds']) == 3 and len(set(row['allDocPrIds'])) == 3, row['drawings'])
            hashes = [item['mediaSha256'] for item in row['drawings']]
            check(checks, label + ': same bitmap for all three pictures', len(hashes) == 3 and len(set(hashes)) == 1 and hashes[0] is not None, hashes)
        original, changed = drawing_map(before), drawing_map(after)
        check(checks, 'Picture identities preserved', set(original) == set(changed) and len(original) == 3, {'before': list(original), 'after': list(changed)})
        if set(original) == set(changed) and len(original) == 3:
            old_order = sorted(original, key=lambda key: original[key]['relativeHeight'])
            new_order = sorted(changed, key=lambda key: changed[key]['relativeHeight'])
            check(checks, 'Original lowest picture becomes highest; others keep order', new_order == old_order[1:] + old_order[:1], {'beforeBackToFront': old_order, 'afterBackToFront': new_order})
            geometry = ('xEmu', 'yEmu', 'widthEmu', 'heightEmu', 'horizontalBase', 'verticalBase', 'mediaSha256')
            check(checks, 'Z-order operation preserves image content and geometry', all(all(original[key][prop] == changed[key][prop] for prop in geometry) for key in original),
                  {key: {'before': original[key], 'after': changed[key]} for key in original})
    elif case == 'move-resize':
        before, after = files['before'], files['after']
        check(checks, 'One floating square-wrapped picture in both', len(before['drawings']) == len(after['drawings']) == 1 and
              before['drawings'][0]['wrapSquare'] and after['drawings'][0]['wrapSquare'], {'before': before['drawings'], 'after': after['drawings']})
        if len(before['drawings']) == len(after['drawings']) == 1:
            b, a = before['drawings'][0], after['drawings'][0]
            delta = {'dxEmu': a['xEmu'] - b['xEmu'], 'dyEmu': a['yEmu'] - b['yEmu']}
            check(checks, 'Position bases preserved', a['horizontalBase'] == b['horizontalBase'] and a['verticalBase'] == b['verticalBase'], {'before': b, 'after': a})
            check(checks, 'Moved right/down approximately two centimeters', all(abs(v - 720000) <= 108000 for v in delta.values()), delta, {'eachOffsetEmu': 720000, 'toleranceEmu': 108000})
            ratios = {key: a[key] / b[key] if b[key] else None for key in ('widthEmu', 'heightEmu')}
            check(checks, 'Width and height halved preserving ratio', all(value is not None and abs(value - 0.5) <= 0.005 for value in ratios.values()), ratios)
            check(checks, 'Bitmap content preserved', a['mediaSha256'] is not None and a['mediaSha256'] == b['mediaSha256'], {'before': b['mediaSha256'], 'after': a['mediaSha256']})
    return checks


def table_signature(row):
    return [[[(cell['text'], cell['gridSpan']) for cell in item['cells']] for item in table['rows']] for table in row['tables']]


def character_signature(row):
    return [(character, run['bold'], run['color']) for run in row['runs'] for character in run['text']]


def drawing_map(row):
    return {item['name'] or item['id']: item for item in row['drawings']}


def inspect_pdf(path, png_root, key, annotation):
    import pdfplumber
    import pypdfium2 as pdfium
    digest = sha256(path.read_bytes())
    row = {'path': str(path), 'sha256': digest, 'pages': [], 'visualReviewed': False}
    png_root.mkdir(parents=True, exist_ok=True)
    document = pdfium.PdfDocument(path)
    with pdfplumber.open(path) as text_document:
        for number, page in enumerate(document, 1):
            png = png_root / (key.replace('/', '--') + '-page-' + str(number) + '.png')
            bitmap = page.render(scale=1.25)
            raster = bitmap.to_pil().convert('RGB')
            raster.save(png)
            pixels = raster.get_flattened_data() if hasattr(raster, 'get_flattened_data') else raster.getdata()
            text_page = text_document.pages[number - 1]
            row['pages'].append({'page': number, 'png': str(png), 'pngSha256': sha256(png.read_bytes()),
                                 'widthPt': text_page.width, 'heightPt': text_page.height,
                                 'text': text_page.extract_text(), 'nonwhitePixels': sum(min(pixel) < 245 for pixel in pixels)})
            bitmap.close()
            page.close()
    document.close()
    row['pageCount'] = len(row['pages'])
    if annotation and annotation.get('pdfSha256') == digest:
        row['visualReview'] = annotation
        row['visualReviewed'] = sorted(annotation.get('pageNumbers', [])) == list(range(1, row['pageCount'] + 1))
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, default=DELIVERY)
    parser.add_argument('--output', type=Path, default=DELIVERY / '_readouts/revfix-inspection.json')
    parser.add_argument('--case', action='append', choices=CASES)
    parser.add_argument('--no-pdf', action='store_true')
    parser.add_argument('--png-root', type=Path, default=DELIVERY / 'screenshots/revfix-pdf')
    parser.add_argument('--annotations', type=Path, default=DELIVERY / '_scripts/revfix-pdf-visual.json')
    args = parser.parse_args()
    annotations = json.loads(args.annotations.read_text(encoding='utf-8-sig')) if args.annotations.exists() else {}
    report = {'generatedUtc': datetime.now(timezone.utc).isoformat(), 'sourceRoot': str(args.source_root),
              'method': 'Read-only ZIP/lxml structural inspection and optional rendering of Word-exported PDF. No Word calls and no DOCX modifications. Projection resolves inline insert/delete/move wrappers and paragraph-mark changes; native accepted/rejected files are the reference for table-merge semantics. PDF visual review is false until an independent hash-bound annotation covers every page.',
              'limitations': ['Same-source branching and native Word authoring require the separate authoring operation log; package XML alone cannot prove them.',
                             'The table-and-move source paragraph already immediately precedes after. Literal cut/paste to that same place may produce no move markup; missing markup remains an explicit unmet requirement.',
                             'PDF final-view content does not establish revision-markup or dialog observations.'],
              'rows': [], 'cases': []}
    for case in args.case or CASES:
        files = {}
        names = ('base', 'tracked', 'accepted', 'rejected') if case in TRACKED_CASES else ('before', 'after')
        for name in names:
            key = case + '/' + name
            path = args.source_root / 'revfix' / (key + '.docx')
            row = {'case': case, 'file': name + '.docx', 'key': key, 'path': str(path), 'exists': path.exists()}
            if path.exists():
                try:
                    row['structure'] = inspect_file(path)
                    files[name] = row['structure']
                except Exception as error:
                    row['error'] = str(error)
            pdf_path = args.source_root / '_previews/revfix' / (key + '.pdf')
            if not args.no_pdf and pdf_path.exists():
                try:
                    row['pdf'] = inspect_pdf(pdf_path, args.png_root, key, annotations.get(key))
                except Exception as error:
                    row['pdfError'] = str(error)
            report['rows'].append(row)
        checks = check_case(case, files)
        case_record = {'case': case, 'checks': checks, 'requirementsPassed': sum(item['passed'] for item in checks),
                       'requirementsTotal': len(checks), 'allRequirementsPassed': all(item['passed'] for item in checks)}
        if case == 'table-and-move' and 'base' in files and 'rejected' in files:
            case_record['nativeOutcomeComparisons'] = {
                'rejectedTableEqualsBase': table_signature(files['rejected']) == table_signature(files['base']),
                'baseTableTextAndSpans': table_signature(files['base']),
                'rejectedTableTextAndSpans': table_signature(files['rejected']),
                'interpretation': 'This comparison documents native Word rejection behavior. Table merge reversibility is not imposed as an additional specification requirement.'}
        report['cases'].append(case_record)
    report['summary'] = {'expectedFiles': len(report['rows']), 'existingFiles': sum(row['exists'] for row in report['rows']),
                         'readableFiles': sum('structure' in row for row in report['rows']),
                         'pdfsAvailable': sum('pdf' in row for row in report['rows']),
                         'pdfsVisuallyReviewed': sum(row.get('pdf', {}).get('visualReviewed', False) for row in report['rows']),
                         'requirementsPassed': sum(case['requirementsPassed'] for case in report['cases']),
                         'requirementsTotal': sum(case['requirementsTotal'] for case in report['cases'])}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding='utf-8')
    print(json.dumps({'output': str(args.output), 'summary': report['summary'],
                      'unmetRequirements': {case['case']: [check for check in case['checks'] if not check['passed']]
                                            for case in report['cases'] if not case['allRequirementsPassed']}}, ensure_ascii=True, indent=2))


if __name__ == '__main__':
    main()
