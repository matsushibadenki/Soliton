"""Verify native text cancellation through an isolated Soliton desktop chat gateway."""
import argparse
from http.client import HTTPConnection
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
from urllib import request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("server", "state", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--ssd", action="store_true")
    args = parser.parse_args()
    if args.output.exists():
        raise FileExistsError(args.output)
    settings = json.loads(args.state.read_bytes())["settings"]
    assert settings["engine"] == "lfm3Omni"
    with tempfile.TemporaryDirectory() as tmp, socket.socket() as probe:
        root = Path(tmp)
        assets = root / "assets"
        assets.mkdir(mode=0o700)
        settings["omni"]["mediaRoot"] = str(assets)
        state = root / "studio.json"
        state.write_text(json.dumps({"settings": settings, "document": {"messages": []}}))
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
        probe.close()
        origin = f"http://127.0.0.1:{port}"
        headers = {"Origin": origin, "Content-Type": "application/json"}

        def api(path, data=None):
            body = None if data is None else json.dumps(data).encode()
            return request.urlopen(request.Request(origin + path, body, headers), timeout=60)

        def read_json(path, data=None):
            with api(path, data) as response:
                return json.load(response)

        def chat(prompt, limit):
            return api("/studio/chat", {"messages": [{"role": "user", "content": prompt}],
                                        "max_tokens": limit})

        def tokens(stream):
            for line in stream:
                if line.startswith(b"data:") and line[5:].strip() != b"[DONE]":
                    choices = json.loads(line[5:]).get("choices", [])
                    if choices and choices[0]["delta"].get("content"):
                        yield choices[0]["delta"]["content"]

        env = {**os.environ, "STUDIO_STATE_PATH": str(state), "STUDIO_PORT": str(port)}
        if args.ssd:
            cache = root / "cache"
            cache.mkdir(mode=0o700)
            env.update(SOLITON_OMNI_SSD_DIR=str(cache), SOLITON_OMNI_SSD_VERIFY="1")

        def read_state():
            return read_json("/studio/state")

        def wait_for(predicate):
            for _ in range(250):
                view = read_state()
                if predicate(view):
                    return view
                time.sleep(0.02)
            raise TimeoutError("Expected chat state was not observed")

        def count(view, event):
            return json.dumps(view["logs"]).count("[studio] Omni SSD " + event)

        def ssd_cases():
            def body(prompt):
                return json.dumps({"messages": [{"role": "user", "content": prompt}], "max_tokens": 16})

            cases = []
            for phase, code, other in [("saving", "42", "73"), ("restoring", "91", "64")]:
                read_json("/studio/action", {"action": "newConversation"})
                prompt = f"Remember the handoff code {code}. " + ("The inventory has already been checked. " * 120)
                prompt += " Reply with only the handoff code."
                if phase == "restoring":
                    with chat(prompt, 16) as stream:
                        assert "".join(tokens(stream)).strip() == code
                    prompt = "What handoff code did I give you? Reply with only the digits."
                before = count(read_state(), phase)
                connection = HTTPConnection("127.0.0.1", port, timeout=60)
                connection.request("POST", "/studio/chat", body(prompt), headers)
                if phase == "saving":
                    response = connection.getresponse()
                    assert response.status == 200
                    while b"[DONE]" not in response.readline():
                        assert not response.isclosed()
                wait_for(lambda view: view["document"]["generating"] and count(view, phase) > before)
                connection.sock.shutdown(socket.SHUT_RDWR)
                connection.close()
                wait_for(lambda view: not view["document"]["generating"])
                if phase == "restoring":
                    assert count(read_state(), "cache restored") > 0
                matching_history = read_state()["document"]["messages"][:2]
                read_json("/studio/action", {"action": "newConversation"})
                read_json("/studio/action", {"action": "messages", "messages": matching_history})
                with chat(f"The old handoff code is obsolete. The new code is {other}. Reply with the new code only.", 16) as stream:
                    assert stream.headers["x-soliton-omni-ssd"] == "miss"
                    answer = "".join(tokens(stream)).strip()
                assert answer == other, (phase, answer, other)
                cases.append({"disconnectedWhile": phase, "priorCode": code, "nextAnswer": answer,
                              "matchingTextPrefixInNextConversation": True, "nextConversationRestoredPriorSnapshot": False})
            return cases

        with (root / "host.log").open("wb") as log:
            host = subprocess.Popen([args.server.resolve()], stdout=log, stderr=log,
                                    env=env)
            try:
                for _ in range(100):
                    try:
                        headers["x-studio-token"] = read_json("/studio/session", {})["token"]
                        break
                    except OSError:
                        if host.poll() is not None:
                            raise RuntimeError("Studio exited during startup")
                        time.sleep(0.1)
                assert "x-studio-token" in headers
                read_json("/studio/action", {"action": "start"})
                for _ in range(120):
                    if read_json("/studio/state")["status"] == "ready":
                        break
                    time.sleep(0.25)
                else:
                    raise TimeoutError("Omni did not become ready")
                cases = []
                for chunks, code in ([] if args.ssd else [(1, "73"), (4, "42"), (8, "91")]):
                    with chat("Count from 1 to 1000, separating the numbers with commas.", 1024) as stream:
                        content = tokens(stream)
                        for _ in range(chunks):
                            assert next(content)
                    start = time.monotonic()
                    for _ in range(100):
                        snapshot = read_json("/studio/state")
                        if (not snapshot["document"]["generating"] and snapshot["requests"]
                                and snapshot["requests"][-1]["status"] == "cancelled"):
                            break
                        time.sleep(0.05)
                    else:
                        raise TimeoutError("Chat did not release after cancellation")
                    wait_ms = round((time.monotonic() - start) * 1000, 1)
                    partial = snapshot["document"]["messages"][-1]["content"]
                    prompt = f"The new handoff code is {code}. Reply only with the new code."
                    with chat(prompt, 16) as stream:
                        answer = "".join(tokens(stream)).strip()
                    assert answer == code, answer
                    history = read_json("/studio/state")["document"]["messages"]
                    assert history[-3]["content"] == partial and history[-1]["content"].strip() == code
                    cases.append({"cancelAfterContentChunks": chunks, "observedReleaseMs": wait_ms,
                                  "nextAnswer": answer, "partialHistoryPreserved": True})
                result = {"engine": "lfm3Omni", "route": "/studio/chat", "maxTokens": 1024,
                          "cases": cases}
                if args.ssd:
                    result["ssdCases"] = ssd_cases()
            finally:
                if host.poll() is None:
                    host.send_signal(signal.SIGINT)
                host.wait(timeout=30)
            assert host.returncode == 0
    with args.output.open("x") as out:
        json.dump(result, out, indent=2)
        out.write("\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
