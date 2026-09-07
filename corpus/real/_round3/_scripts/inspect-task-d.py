"""Read-only package and optional Word-exported PDF inspection for Task D."""

import argparse
import hashlib
import importlib.util
import io
import json
import zipfile
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

from lxml import etree


ROOT = Path('C:/word/real-word-round3-20260907')
NS = {
    'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main',
    'w14': 'http://schemas.microsoft.com/office/word/2010/wordml',
    'w15': 'http://schemas.microsoft.com/office/word/2012/wordml',
    'wps': 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'inkml': 'http://www.w3.org/2003/InkML',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
}


def sha256(raw):
    return hashlib.sha256(raw).hexdigest().upper()


def xp(node, expression):
    return node.xpath(expression, namespaces=NS) if node is not None else []


def attr(node, prefix, name):
    return node.get('{' + NS[prefix] + '}' + name)


def package(path):
    raw = path.read_bytes()
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        names = archive.namelist()
        parts = {name: archive.read(name) for name in names if not name.endswith('/')}
        info = {
            'path': str(path), 'sha256': sha256(raw), 'bytes': len(raw),
            'crcError': archive.testzip(),
            'duplicateMembers': [name for name, count in Counter(names).items() if count > 1],
        }
    xml = {name: etree.fromstring(data) for name, data in parts.items() if name.endswith(('.xml', '.rels'))}
    info['partHashes'] = {name: sha256(data) for name, data in parts.items()}
    return info, parts, xml


def inspect_ink(path):
    info, parts, xml = package(path)
    document = xml['word/document.xml']
    info['countScope'] = 'All markup-compatibility branches in word/document.xml; counts do not imply all branches render simultaneously.'
    info['counts'] = {
        'w14:contentPart': len(xp(document, '//w14:contentPart')),
        'wps:wsp': len(xp(document, '//wps:wsp')),
        'a:prstGeom': len(xp(document, '//a:prstGeom')),
    }
    info['presetGeometries'] = [{'prst': node.get('prst'), 'path': document.getroottree().getpath(node)}
                                for node in xp(document, '//a:prstGeom')]
    info['contentParts'] = [{'relationshipId': attr(node, 'r', 'id'), 'attributes': dict(node.attrib),
                             'path': document.getroottree().getpath(node)}
                            for node in xp(document, '//w14:contentPart')]
    info['bodyText'] = '\n'.join(''.join(xp(paragraph, './/w:t/text()')) for paragraph in xp(document, '//w:body/w:p'))
    inks = []
    for name, raw in parts.items():
        if not name.startswith('word/ink/') or '/_rels/' in name:
            continue
        item = {'part': name, 'sha256': sha256(raw), 'bytes': len(raw)}
        try:
            root = etree.fromstring(raw)
            traces = xp(root, '//inkml:trace')
            item.update({'rootTag': root.tag, 'traceCount': len(traces),
                         'traces': [{'attributes': dict(node.attrib), 'dataCharacters': len(node.text or ''),
                                     'data': node.text or ''} for node in traces]})
        except etree.XMLSyntaxError as error:
            item['unreadableAsXml'] = str(error)
        inks.append(item)
    info['nativeInkParts'] = inks
    info['nativeInkPartCount'] = len(inks)
    info['readableInkTraceCount'] = sum(item.get('traceCount', 0) for item in inks)
    info['unreadableInkPartCount'] = sum('unreadableAsXml' in item for item in inks)
    info['requestedCircleStructureMet'] = (info['counts']['w14:contentPart'] == 0
                                           and info['counts']['wps:wsp'] > 0
                                           and any(item['prst'] in ('ellipse', 'flowChartConnector') for item in info['presetGeometries']))
    info['scopeLimitation'] = ('Operator reports exactly one straight drag, not a closed circle. '
                              'These structural observations cannot establish success or failure of single-stroke circle recognition. '
                              'The GUI chronology, drawing-tool state and actual drag are recorded separately by the operator.')
    return info


def inspect_comments(path):
    info, _, xml = package(path)
    document = xml['word/document.xml']
    comments = []
    para_owners = {}
    for node in xp(xml.get('word/comments.xml'), '/w:comments/w:comment'):
        ids = xp(node, './/w:p/@w14:paraId')
        item = {'commentId': attr(node, 'w', 'id'), 'author': attr(node, 'w', 'author'),
                'date': attr(node, 'w', 'date'), 'initials': attr(node, 'w', 'initials'),
                'text': '\n'.join(''.join(xp(p, './/w:t/text()')) for p in xp(node, './/w:p')),
                'paraIds': ids, 'lastParaId': ids[-1] if ids else None,
                'commentsExtended': []}
        comments.append(item)
        for para_id in ids:
            para_owners.setdefault(para_id, []).append(item['commentId'])
    extension_rows = []
    orphan_extensions = []
    by_id = {item['commentId']: item for item in comments}
    for node in xp(xml.get('word/commentsExtended.xml'), '/w15:commentsEx/w15:commentEx'):
        para_id, parent_id = attr(node, 'w15', 'paraId'), attr(node, 'w15', 'paraIdParent')
        done = attr(node, 'w15', 'done')
        row = {'paraId': para_id, 'paraIdParent': parent_id, 'doneRaw': done,
               'done': done in ('1', 'true', 'on'),
               'commentIds': para_owners.get(para_id, []), 'parentCommentIds': para_owners.get(parent_id, [])}
        extension_rows.append(row)
        if len(row['commentIds']) != 1:
            orphan_extensions.append(row)
        else:
            by_id[row['commentIds'][0]]['commentsExtended'].append(row)
    graph_errors = []
    for item in comments:
        entries = item['commentsExtended']
        item['extensionPresent'] = len(entries) == 1
        item['parentCommentId'] = None
        item['parentParaId'] = None
        item['done'] = None
        if len(entries) != 1:
            graph_errors.append({'commentId': item['commentId'], 'error': 'Expected exactly one commentEx entry', 'count': len(entries)})
            continue
        entry = entries[0]
        item['parentParaId'], item['done'] = entry['paraIdParent'], entry['done']
        if entry['paraIdParent']:
            if len(entry['parentCommentIds']) == 1:
                item['parentCommentId'] = entry['parentCommentIds'][0]
            else:
                graph_errors.append({'commentId': item['commentId'], 'error': 'Unresolved or ambiguous parent paraId', 'paraIdParent': entry['paraIdParent']})
    for item in comments:
        chain, current, valid = [], item, True
        while current is not None:
            if current['commentId'] in chain:
                graph_errors.append({'commentId': item['commentId'], 'error': 'Parent cycle', 'chain': chain})
                valid = False
                break
            chain.append(current['commentId'])
            if not current['extensionPresent'] or (current['parentParaId'] and current['parentCommentId'] is None):
                valid = False
                break
            current = by_id.get(current['parentCommentId'])
        item['parentChainFromSelf'] = chain
        item['depth'] = len(chain) - 1 if valid else None
        item['rootCommentId'] = chain[-1] if valid else None
    roots = [item['commentId'] for item in comments if item['depth'] == 0]
    info.update({
        'commentsXmlPresent': 'word/comments.xml' in xml,
        'commentsExtendedXmlPresent': 'word/commentsExtended.xml' in xml,
        'comments': comments, 'commentCount': len(comments), 'extensionRows': extension_rows,
        'paraIdToCommentIds': para_owners,
        'duplicateCommentIds': [key for key, count in Counter(item['commentId'] for item in comments).items() if count > 1],
        'duplicateParaIds': {key: owners for key, owners in para_owners.items() if len(owners) != 1},
        'orphanExtensionRows': orphan_extensions, 'graphErrors': graph_errors,
        'rootCommentIds': roots, 'rootCount': len(roots),
        'maximumDepth': max((item['depth'] for item in comments if item['depth'] is not None), default=None),
        'depthConvention': 'Root depth 0; direct reply depth 1; reply to reply depth 2.',
        'threads': [{'rootCommentId': root, 'members': [{'commentId': item['commentId'], 'depth': item['depth'], 'done': item['done']}
                                                       for item in comments if item['rootCommentId'] == root]}
                    for root in roots],
        'anchors': {name: xp(document, '//w:' + name + '/@w:id')
                    for name in ('commentRangeStart', 'commentRangeEnd', 'commentReference')},
        'scopeLimitation': 'Saved parent/done attributes are structural evidence. They do not prove which reply or resolve UI button was used; consult operator GUI chronology.',
    })
    nested_resolved_threads = [thread['rootCommentId'] for thread in info['threads']
                              if any(member['depth'] >= 2 for member in thread['members'])
                              and all(member['done'] for member in thread['members'])]
    info['nestedFullyResolvedRootIds'] = nested_resolved_threads
    info['requestedThreadStructureMet'] = (info['rootCount'] == 3 and bool(nested_resolved_threads)
                                           and not graph_errors and not orphan_extensions
                                           and not info['duplicateCommentIds'] and not info['duplicateParaIds'])
    return info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--no-pdf', action='store_true')
    args = parser.parse_args()
    root = args.root
    report = {'generatedUtc': datetime.now(timezone.utc).isoformat(),
              'method': 'Read-only ZIP/lxml inspection with SHA-256 of final packages and their parts. Optional independent rendering of existing Word-exported PDFs. No COM, UI control or DOCX modification.',
              'ink': None, 'comments': None, 'pdfs': [], 'missing': []}
    for key, relative, inspector in [('ink', 'ink2/ink-to-shape-2.docx', inspect_ink),
                                      ('comments', 'comments2/comment-nesting.docx', inspect_comments)]:
        path = root / relative
        if path.exists():
            report[key] = inspector(path)
        else:
            report['missing'].append(str(path))
    if not args.no_pdf:
        spec = importlib.util.spec_from_file_location('revfix', root / '_scripts/inspect-revfix.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        annotation_path = root / '_scripts/task-d-pdf-annotations.json'
        annotations = json.loads(annotation_path.read_text(encoding='utf-8-sig')) if annotation_path.exists() else {}
        for name in ('d-ink.pdf', 'd-comments.pdf', 'd-comments-markup.pdf'):
            candidates = list((root / '_previews').rglob(name))
            if len(candidates) > 1:
                raise ValueError('Ambiguous PDF name: ' + name)
            if candidates:
                report['pdfs'].append(module.inspect_pdf(candidates[0], root / 'screenshots/task-d-pdf', Path(name).stem, annotations.get(name)))
    report['summary'] = {'packagesPresent': sum(report[key] is not None for key in ('ink', 'comments')),
                         'pdfCount': len(report['pdfs']), 'pdfPages': sum(item['pageCount'] for item in report['pdfs']),
                         'visuallyReviewedPdfs': sum(item['visualReviewed'] for item in report['pdfs'])}
    report['rows'] = [{'file': relative, 'sha256': report[key]['sha256'], 'structure': report[key],
                       'requiredStructureMet': report[key][condition],
                       'limitations': [report[key]['scopeLimitation']]}
                      for key, relative, condition in [('ink', 'ink2/ink-to-shape-2.docx', 'requestedCircleStructureMet'),
                                                        ('comments', 'comments2/comment-nesting.docx', 'requestedThreadStructureMet')]
                      if report[key] is not None]
    output = root / '_readouts/task-d-inspection.json'
    output.write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding='utf-8')
    print(json.dumps(report['summary'], indent=2))


if __name__ == '__main__':
    main()
