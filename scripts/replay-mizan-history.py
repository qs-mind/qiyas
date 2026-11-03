#!/usr/bin/env python3
"""Read a saved conversation once and estimate context-maintenance costs offline.

No provider requests, artifact writes or private fixtures. The saved history is already
compacted and pictures are stripped: this is a retained-tail replay, not a reconstruction
of the original run. Full compaction below deliberately carries all existing text and
contract notes; its size is a conservative approximation, not the Rust implementation.
Usage: python3 scripts/replay-mizan-history.py /path/to/conversation.json [--output report.json]
"""
import argparse
import copy
import hashlib
import json
import statistics
import time
from collections import Counter
from pathlib import Path

MARK = '[tool result summary]'
REF_KEYS = set('handle handles added changed removed centrelines outlines hatches inserts command object driver id key code name object_type rule state pointer'.split())


def encode(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def size(value):
    return len(encode(value).encode())


def digest(value):
    return hashlib.sha256(encode(value).encode()).hexdigest()


def block(kind, value):
    return {'type': kind, 'value': value}


def cost(b):
    if b['type'] == 'image':
        return 1600
    if b['type'] == 'tool_result':
        return 8 + sum(1600 if p['type'] == 'png' else (len(p['value'].encode()) + 2) // 3 for p in b['value']['content'])
    return (size(b) + 2) // 3


def tokens(history):
    return sum(4 + sum(cost(b) for b in m['content']) for m in history)


def blocks(history, kind):
    return [b['value'] for m in history for b in m['content'] if b['type'] == kind]


def args(input_value):
    value = input_value.get('args', {}) if isinstance(input_value, dict) else {}
    if isinstance(value, str):
        try:
            return json.loads(value)
        except ValueError:
            return {}
    return value


def operation(call):
    value = call['input']
    op = call['name'] + '.' + value.get('op', 'picture') if isinstance(value, dict) else call['name'] + '.picture'
    command = args(value).get('command', {})
    if isinstance(command, str):
        try:
            command = json.loads(command)
        except ValueError:
            command = {}
    if isinstance(command, dict) and 'type' in command:
        op += '/' + command['type']
        if isinstance(command.get('kind'), str):
            op += '.' + command['kind']
    return op


def references(value, key='', found=None):
    found = {} if found is None else found
    if isinstance(value, dict):
        for k, v in value.items():
            if k not in ('schema', 'examples', 'example', 'defaults_used'):
                references(v, k, found)
    elif isinstance(value, list):
        for v in value:
            references(v, key, found)
    elif key in REF_KEYS and isinstance(value, (str, int, float)) and not isinstance(value, bool):
        found.setdefault(key, set()).add(encode(value))
    return found


def trim(history, modern, protected):
    high, low, minimum = (60000, 30000, 12000) if modern else (100000, 55000, 30000)
    before = tokens(history)
    if before < high:
        return 0
    calls = {c['id']: c for c in blocks(history, 'tool_use')}
    results = blocks(history, 'tool_result')
    recent = {r['tool_use_id'] for r in results[-4:]}
    complete = {r['tool_use_id'] for r in results}
    first_digest = next((c['id'] for c in calls.values() if c['name'] == 'qiyas_read' and c['input'].get('op') == 'digest'), None)
    after, changes = before, []
    for m in history:
        signed = any(b['type'] in ('thinking', 'raw') for b in m['content'])
        for i, b in enumerate(m['content']):
            value, replacement = b['value'], None
            if modern and b['type'] == 'tool_use':
                ident, input_value = value['id'], value['input']
                if not signed and ident not in protected | recent and ident in complete and isinstance(input_value, dict) and '_history_summary' not in input_value and size(input_value) >= 8192:
                    original = dict(call_id=ident, name=value['name'], input=input_value)
                    commands = args(input_value).get('commands')
                    summary = dict(operation=operation(value), artifact=digest(original), pointer='/input', command_count=len(commands) if isinstance(commands, list) else None, detail='Historical call already completed; original arguments are in the immutable artifact. This summary is not executable.')
                    replacement = block('tool_use', dict(value, input=dict(op=input_value.get('op'), _history_summary=summary)))
            elif b['type'] == 'tool_result':
                ident, content = value['tool_use_id'], value['content']
                if ident in recent or ident == first_digest or (content and content[0]['type'] == 'text' and content[0]['value'].startswith(MARK)):
                    continue
                refs, errors, artifact = {}, [], None
                for p in content:
                    if p['type'] != 'text':
                        continue
                    try:
                        parsed = json.loads(p['value'])
                        references(parsed, found=refs)
                        if isinstance(parsed, dict):
                            artifact = artifact or parsed.get('artifact')
                            errors.extend(parsed[k] for k in ('message', 'error', 'errors', 'code') if parsed.get(k) is not None)
                    except ValueError:
                        if value['is_error']:
                            errors.append(p['value'])
                evidence = [block('text', '[picture removed at trim; capture again if needed]') if p['type'] == 'png' else p for p in content]
                op = operation(calls[ident]) if ident in calls else None
                original = dict(call_id=ident, operation=op, content=evidence, is_error=value['is_error'])
                summary = dict(call_id=ident, operation=op, is_error=value['is_error'], references={k: sorted(v)[:5] for k, v in refs.items()}, errors=errors, artifact=artifact or digest(original))
                replacement = block('tool_result', dict(value, content=[block('text', MARK + ' ' + encode(summary))]))
            if replacement is not None and cost(replacement) < cost(b):
                after -= cost(b) - cost(replacement)
                changes.append((m, i, replacement))
            if after <= low:
                break
        if after <= low:
            break
    if before - after < minimum:
        return 0
    for m, i, replacement in changes:
        m['content'][i] = replacement
    return len(changes)


def compact(history, modern):
    if tokens(history) < (90000 if modern else 150000):
        return False
    low, keep, tail = (30000 if modern else 55000), 0, 0
    for m in reversed(history):
        if keep >= 8 and tail >= low:
            break
        keep += 1
        tail += tokens([m])
    cut = len(history) - keep
    while cut > 0 and (history[cut]['role'] != 'assistant' or history[cut - 1]['role'] != 'user'):
        cut -= 1
    if cut == 0:
        return False
    retired = history[:cut]
    # Keep all old note/user text verbatim. Production merges notes and contracts more tightly.
    note = [b for m in retired for b in m['content'] if b['type'] == 'text']
    results = {r['tool_use_id']: r for r in blocks(retired, 'tool_result')}
    index = []
    for c in blocks(retired, 'tool_use'):
        r = results.get(c['id'], {})
        item = dict(call_id=c['id'], operation=operation(c), is_error=r.get('is_error'), artifact=digest(r))
        if modern:
            item.update(input_artifact=digest(dict(call_id=c['id'], name=c['name'], input=c['input'])), input_pointer='/input')
        index.append(item)
    note.append(block('text', '[replay evidence index]\n' + encode(index)))
    history[:] = [dict(role='user', content=note)] + history[cut:]
    return True


def replay(source, modern, protected):
    history, measurements, trims, compactions = [], [], 0, 0
    for message in source:
        history.append(copy.deepcopy(message))
        # A provider request follows a completed user/result message, not a pending call.
        if message['role'] != 'user':
            continue
        if modern:
            trims += bool(trim(history, modern, protected))
            compactions += compact(history, modern)
        else:
            compactions += compact(history, modern)
            trims += bool(trim(history, modern, protected))
        measurements.append(tokens(history))
    return dict(request_points=len(measurements), mean=round(statistics.mean(measurements)), maximum=max(measurements), final=tokens(history), summed=sum(measurements), trims=trims, compactions=compactions)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('conversation', type=Path)
    parser.add_argument('--output', type=Path)
    options = parser.parse_args()
    start = time.perf_counter()
    raw = options.conversation.read_bytes()
    saved = json.loads(raw)
    events = [e['event'] for e in saved['transcript'] if e['type'] == 'event']
    usage = [e['usage']['input_tokens'] for e in events if e['type'] == 'cache_usage']
    type_cost = Counter()
    for m in saved['history']:
        for b in m['content']:
            type_cost[b['type']] += cost(b)
    protected = set((saved.get('provider_state') or {}).get('extras', {}))
    baseline = replay(saved['history'], False, protected)
    revised = replay(saved['history'], True, protected)
    all_calls = [e for e in events if e['type'] == 'tool_call']
    sizes = [size(c['input']) for c in all_calls]
    report = dict(source_sha256=hashlib.sha256(raw).hexdigest(), source_bytes=len(raw), source_updated=saved['updated'], history_messages=len(saved['history']), history_calls=len(blocks(saved['history'], 'tool_use')), transcript_calls=len(all_calls), transcript_failed_calls=sum(e['type'] == 'tool_result' and e['is_error'] for e in events), provider_usage_events=len(usage), observed_provider_mean=round(statistics.mean(usage)), observed_provider_max=max(usage), final_local_estimate=tokens(saved['history']), block_token_estimates=type_cost, argument_bytes=dict(total=sum(sizes), maximum=max(sizes), over_8192=sum(s >= 8192 for s in sizes)), baseline_tail_replay=baseline, revised_tail_replay=revised, estimated_tail_sum_reduction=round(1-revised['summed']/baseline['summed'],4), elapsed_seconds=round(time.perf_counter()-start,4), caveats=['Local UTF-8 bytes/3 estimator, not a provider tokenizer or bill.', 'Replay covers only the retained saved tail, already compacted; earlier full requests unavailable.', 'Saved images are removed; no image-token savings are counted.', 'Full compaction approximates the Rust notes with all old text retained and an evidence index; it does not execute Rust.', 'No tool/system schema overhead, provider reasoning accounting, cache residency, latency or live behavior is predicted.', 'Immutable artifacts are simulated by content hashes; no private file is changed or created.'])
    output = json.dumps(report, ensure_ascii=False, indent=2) + '\n'
    if options.output:
        options.output.write_text(output)
    else:
        print(output, end='')


if __name__ == '__main__':
    main()
