#!/usr/bin/env python3
"""CubeSandbox reproduction driver.

The automl-server reproduction worker invokes this script when
REPRODUCE_BACKEND=cubesandbox. It receives a capsule workspace directory as
argv[1]; that directory contains a ``spec.json`` describing what to run:

    {
      "paper_id": "...",
      "runtime": "python3",
      "command": "python3 main.py",
      "timeout_secs": 900,
      "allow_network": false,
      "cube_sandbox_url": "http://127.0.0.1:PORT",
      "cube_sandbox_api_key": "..."
    }

It runs the command inside a Tencent CubeSandbox (KVM-isolated, E2B-compatible)
and prints a single JSON outcome object on the last line of stdout:

    {"status": "passed|failed|error", "exit_code": 0, "stdout_tail": "...",
     "metrics": {...}, "output_hashes": {"metrics.json": "<sha256>"},
     "sandbox_id": "cube_..."}

Keeping the vendor-specific bits here (rather than in the Rust server) means the
CubeSandbox integration can be finished/tuned on the configured sandbox host without
rebuilding the server. Until the `cubesandbox` SDK is installed and a sandbox
service is reachable, this driver transparently falls back to a local
subprocess so the pipeline is runnable end to end.
"""

import hashlib
import json
import os
import subprocess
import sys


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def collect_evidence(workspace: str):
    """Read metrics.json and hash conventional output artifacts."""
    metrics = {}
    output_hashes = {}

    metrics_path = os.path.join(workspace, "metrics.json")
    if os.path.isfile(metrics_path):
        raw = open(metrics_path, "rb").read()
        output_hashes["metrics.json"] = sha256_hex(raw)
        try:
            metrics = json.loads(raw.decode("utf-8"))
        except Exception:
            metrics = {}

    outputs_dir = os.path.join(workspace, "outputs")
    if os.path.isdir(outputs_dir):
        for name in sorted(os.listdir(outputs_dir)):
            path = os.path.join(outputs_dir, name)
            if os.path.isfile(path):
                output_hashes[f"outputs/{name}"] = sha256_hex(open(path, "rb").read())

    return metrics, output_hashes


def status_for(exit_code):
    if exit_code is None:
        return "error"
    return "passed" if exit_code == 0 else "failed"


def run_in_cubesandbox(spec, workspace):
    """Run the capsule inside CubeSandbox via its E2B-compatible SDK.

    Returns the outcome dict, or raises to trigger the local fallback.
    """
    from cubesandbox import Sandbox  # type: ignore

    url = spec.get("cube_sandbox_url") or None
    api_key = spec.get("cube_sandbox_api_key") or None
    timeout = int(spec.get("timeout_secs", 900))

    # E2B-compatible constructor. Adjust kwargs to your CubeSandbox build.
    sandbox = Sandbox(domain=url, api_key=api_key, timeout=timeout)
    try:
        # Upload capsule files into the guest working directory.
        for root, _dirs, files in os.walk(workspace):
            for fname in files:
                if fname == "spec.json":
                    continue
                local = os.path.join(root, fname)
                rel = os.path.relpath(local, workspace)
                with open(local, "rb") as fh:
                    sandbox.files.write(rel, fh.read())

        proc = sandbox.commands.run(spec["command"], timeout=timeout)
        exit_code = getattr(proc, "exit_code", None)
        stdout = getattr(proc, "stdout", "") or ""
        stderr = getattr(proc, "stderr", "") or ""

        # Pull evidence back out of the guest, if present.
        metrics, output_hashes = {}, {}
        try:
            raw = sandbox.files.read("metrics.json")
            data = raw if isinstance(raw, bytes) else str(raw).encode("utf-8")
            output_hashes["metrics.json"] = sha256_hex(data)
            metrics = json.loads(data.decode("utf-8"))
        except Exception:
            pass

        log = stdout + ("\n--- stderr ---\n" + stderr if stderr else "")
        return {
            "status": status_for(exit_code),
            "exit_code": exit_code,
            "stdout_tail": log[-4000:],
            "metrics": metrics,
            "output_hashes": output_hashes,
            "sandbox_id": getattr(sandbox, "sandbox_id", None),
        }
    finally:
        try:
            sandbox.kill()
        except Exception:
            pass


def run_local_fallback(spec, workspace):
    """Plain subprocess fallback so the driver is runnable without CubeSandbox."""
    timeout = int(spec.get("timeout_secs", 900))
    try:
        proc = subprocess.run(
            ["sh", "-c", spec["command"]],
            cwd=workspace,
            capture_output=True,
            timeout=timeout,
        )
        exit_code = proc.returncode
        log = proc.stdout.decode("utf-8", "replace")
        err = proc.stderr.decode("utf-8", "replace")
        if err:
            log += "\n--- stderr ---\n" + err
    except subprocess.TimeoutExpired:
        return {
            "status": "error",
            "exit_code": None,
            "stdout_tail": f"timed out after {timeout}s",
            "metrics": {},
            "output_hashes": {},
            "sandbox_id": None,
        }

    metrics, output_hashes = collect_evidence(workspace)
    return {
        "status": status_for(exit_code),
        "exit_code": exit_code,
        "stdout_tail": log[-4000:],
        "metrics": metrics,
        "output_hashes": output_hashes,
        "sandbox_id": "local-fallback",
    }


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"status": "error", "stdout_tail": "missing workspace arg"}))
        return
    workspace = sys.argv[1]

    try:
        spec = json.load(open(os.path.join(workspace, "spec.json")))
    except Exception as exc:
        print(json.dumps({"status": "error", "stdout_tail": f"bad spec.json: {exc}"}))
        return

    try:
        outcome = run_in_cubesandbox(spec, workspace)
    except Exception as exc:
        # SDK missing or sandbox unreachable. Only fall back to host-local
        # execution when explicitly allowed (dev) — never in production, where
        # running the capsule on the host would defeat isolation.
        if spec.get("allow_local_fallback"):
            outcome = run_local_fallback(spec, workspace)
            note = f"[cubesandbox unavailable, local fallback: {exc}] "
            outcome["stdout_tail"] = note + outcome.get("stdout_tail", "")
        else:
            outcome = {
                "status": "error",
                "exit_code": None,
                "stdout_tail": f"cubesandbox unavailable and local fallback disabled: {exc}",
                "metrics": {},
                "output_hashes": {},
                "sandbox_id": None,
            }

    print(json.dumps(outcome))


if __name__ == "__main__":
    main()
