#!/usr/bin/python3
"""Fake documented CLI. Never imports an SDK, discovers a login or calls a provider."""
import json
import os
import pathlib
import signal
import sys
import time
import urllib.request
import urllib.parse

root = pathlib.Path(__file__).parent
scenario = json.loads((root / "scenario.json").read_text())


def record(value):
    # Keep argv structure for safety assertions without recording the private MCP token.
    if "argv" in value and "--mcp-config" in value["argv"]:
        logged = list(value["argv"])
        index = logged.index("--mcp-config") + 1
        config = json.loads(logged[index])
        for server in config.get("mcpServers", {}).values():
            if "headers" in server:
                server["headers"] = {name: "[redacted]" for name in server["headers"]}
        logged[index] = json.dumps(config)
        value = dict(value, argv=logged)
    with (root / "calls.jsonl").open("a") as file:
        file.write(json.dumps(value) + "\n")


args = sys.argv[1:]
record({"argv": args, "environment": dict(os.environ), "cwd": os.getcwd()})
if args == ["--help"]:
    print(scenario["help"])
    if scenario.get("option_help"):
        print("  --effort <level> Effort level (low, medium, high, xhigh, max)")
        print("  --model <model> An alias for the latest model (e.g. 'fable', 'opus', or 'sonnet') or a model's full name")
elif args == ["--version"]:
    print("2.1.309 (Claude Code)")
elif args == ["auth", "--help"]:
    print("status  Show authentication status")
elif args == ["auth", "status"]:
    print(json.dumps({"loggedIn": True}))
elif "-p" in args:
    record({"input": json.loads(sys.stdin.readline())})
    for event in scenario.get("events_before_wait", []):
        print(json.dumps(event), flush=True)
    if scenario.get("mcp"):
        config = json.loads(args[args.index("--mcp-config") + 1])["mcpServers"]["qiyas"]
        assert urllib.parse.urlparse(config["url"]).hostname == "127.0.0.1"
        headers = dict(config["headers"], **{
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        })
        sequence = 0

        def rpc(method, params):
            global sequence
            sequence += 1
            request = {"jsonrpc": "2.0", "id": sequence, "method": method, "params": params}
            data = json.dumps(request).encode()
            with urllib.request.urlopen(urllib.request.Request(config["url"], data, headers), timeout=10) as response:
                session = response.headers.get("mcp-session-id")
                if session:
                    headers["mcp-session-id"] = session
                if "text/event-stream" in response.headers.get("Content-Type", ""):
                    while True:
                        line = response.readline()
                        assert line, "MCP response ended before the result"
                        # Servers may send an empty priming event before the result.
                        if line.startswith(b"data: ") and line[6:].strip():
                            result = json.loads(line[6:])
                            if result.get("id") == sequence:
                                break
                else:
                    result = json.load(response)
            record({"mcp_request": request, "mcp_response": result})
            assert "error" not in result, result
            return result["result"]

        rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                           "clientInfo": {"name": "fake-claude", "version": "1"}})
        initialized = {"jsonrpc": "2.0", "method": "notifications/initialized"}
        with urllib.request.urlopen(urllib.request.Request(
            config["url"], json.dumps(initialized).encode(), headers
        ), timeout=10) as response:
            response.read()
        record({"mcp_notification": initialized})
        listed = rpc("tools/list", {})
        assert {tool["name"] for tool in listed["tools"]} == {
            "qiyas_read", "qiyas_execute", "qiyas_capture"}

        def tool(name, op, arguments):
            value = {"op": op, "args": json.dumps(arguments)}
            identifier = "fake-tool-" + str(sequence)
            if scenario.get("stream_tools"):
                for event in [
                    {"type": "content_block_start", "index": 0, "content_block": {
                        "type": "tool_use", "id": identifier, "name": "mcp__qiyas__" + name, "input": {}}},
                    {"type": "content_block_delta", "index": 0, "delta": {
                        "type": "input_json_delta", "partial_json": json.dumps(value)}},
                    {"type": "content_block_stop", "index": 0},
                ]:
                    print(json.dumps({"type": "stream_event", "event": event}), flush=True)
            print(json.dumps({"type": "assistant", "message": {"content": [
                {"type": "tool_use", "id": identifier, "name": "mcp__qiyas__" + name, "input": value}
            ]}}), flush=True)
            result = rpc("tools/call", {"name": name, "arguments": value})
            assert not result.get("isError"), result
            print(json.dumps({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": identifier, "content": result["content"]}
            ]}}), flush=True)
            return json.loads(next(part["text"] for part in result["content"] if part["type"] == "text"))

        assert tool("qiyas_read", "records", {"collection": "entities"})["total"] == 0
        units_per_metre = scenario.get("units_per_metre", 1)
        tool("qiyas_execute", "run", {"layer": "Provider check", "command": {
            "type": "draw", "kind": "line", "a": [0, 0], "b": [3 * units_per_metre, 0]}})
        readback = tool("qiyas_read", "records", {"collection": "entities", "fields": ["length"]})
        assert readback["total"] == 1
        assert abs(readback["records"][0]["fields"]["length"] / units_per_metre - 3) < 1e-7
        tool("qiyas_execute", "undo_own", {})
        assert tool("qiyas_read", "records", {"collection": "entities"})["total"] == 0
    if scenario.get("wait"):
        def interrupted(number, frame):
            record({"signal": number})
            if number == signal.SIGTERM or not scenario.get("ignore_interrupt"):
                sys.exit(0)
        signal.signal(signal.SIGINT, interrupted)
        signal.signal(signal.SIGTERM, interrupted)
        record({"ready": True})
        while True:
            time.sleep(0.05)
    for event in scenario.get("events", []):
        print(json.dumps(event), flush=True)
else:
    sys.exit(2)
