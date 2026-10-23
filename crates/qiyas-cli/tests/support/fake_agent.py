"""Offline protocol client: decisions use only the returned take-off kit."""
import json
from pathlib import Path
import subprocess
import sys

cli, drawing, workdir = sys.argv[1:4]
extra = sys.argv[4:]
method = None
if extra[:1] == ["--method"]:
    method = extra[1]
    extra = extra[2:]
# Before MCP starts, only the permitted case inputs and runner logs may be present.
assert {path.name for path in Path(workdir).iterdir()} <= {
    Path(drawing).name, "preamble.md", "agent-events.jsonl", "agent-stderr.log",
}
server = subprocess.Popen(
    [cli, "mcp", "--drawing", drawing, "--workdir", workdir] + (["--method", method] if method else []),
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
)
sequence = 0


def request(method, params):
    global sequence
    sequence += 1
    server.stdin.write(json.dumps({"jsonrpc": "2.0", "id": sequence,
                                  "method": method, "params": params}) + "\n")
    server.stdin.flush()
    while True:
        line = server.stdout.readline()
        assert line, "server closed without response"
        answer = json.loads(line)
        if answer.get("id") == sequence:
            assert "error" not in answer, answer
            return answer["result"]


def tool(name, op, args):
    print(json.dumps({"type": "mcp_tool_call", "tool": name,
                      "input": {"op": op, "args": args}}), flush=True)
    result = request("tools/call", {"name": name, "arguments": {"op": op, "args": args}})
    assert not result.get("isError"), result
    return json.loads(result["content"][0]["text"])


request("initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
                       "clientInfo": {"name": "offline-test", "version": "1"}})
server.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
server.stdin.flush()
tool("qiyas_read", "digest", {})
kit = tool("qiyas_read", "takeoff_kit", {})
if method:
    assert kit["measurement_method"]["name"] == method
    assert "Write rules from your knowledge" in kit["measurement_method"]["instruction"]
batch = kit["batch"]
for command in batch["commands"]:
    kind = command.get("kind")
    if command["type"] == "define_object_type":
        command["definition"]["geometry"] = "region"
        command["definition"]["quantities"] = {"surface": "geometry.area"}
    elif kind == "define":
        command["rule"]["match"] = {"entity_type": "LwPolyline", "closed_region": True}
    elif kind == "set_method":
        if method:
            # Synthetic transport fixture only; no rule of any standard is supplied.
            import copy
            rule = copy.deepcopy(kit["commands"]["measurement.set_method"]["example"]["method"]["rules"][0])
            cite = {"source": method, "clause": method + " clause TEST-ONLY (not a standard clause)"}
            rule["citation"] = cite
            rule["terms"][0]["citation"] = cite
            command["method"]["source_note"] = "Synthetic transport test only; not standard guidance"
            command["method"]["rules"] = [rule]
        rule = command["method"]["rules"][0]
        rule["unit"] = "m2"
        rule["kind"] = "area"
        rule["terms"][0]["factors"] = ["geometry.area"]
    elif kind == "set_mappings":
        command["mappings"][0]["unit"] = "m2"
        command["mappings"][0]["code"] = "surface-{id}"
        if method:
            command["mappings"][0]["citation"] = cite
tool("qiyas_execute", "batch", batch)
tool("qiyas_execute", "finish", {"answer": "Offline geometric BOQ; other scope unresolved."})
server.stdin.close()
assert server.wait(timeout=15) == 0
if extra:
    # Only this deliberate negative test reads a test truth file, after the protocol run.
    path = extra[0]
    print(json.dumps({"type": "tool_use", "name": "Read", "input": {"file_path": path}}))
    with open(path, encoding="utf-8") as truth_file:
        truth_file.read()
