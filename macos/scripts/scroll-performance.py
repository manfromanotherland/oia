#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Capture an isolated real-app scroll replay; never infer 120 FPS from event delivery."""

import argparse
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid


def wait_for(path, process, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        if process.poll() is not None:
            raise RuntimeError(f"App exited with status {process.returncode}; see app.log")
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {path.name}; see app.log")


def stop(process):
    if process is None or process.poll() is not None:
        return
    process.send_signal(signal.SIGINT)
    try:
        process.wait(timeout=45)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def main():
    script_dir = Path(__file__).resolve().parent
    repo = script_dir.parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--app", type=Path,
        default=repo / "macos/build/Build/Products/Release/Óia.app/Contents/MacOS/Oia",
        help="App executable to profile (defaults to the optimized Release build)",
    )
    parser.add_argument("--output", type=Path, required=True, help="New results directory; never overwritten")
    parser.add_argument("--fixture", type=Path, help="Reusable generated fixture directory")
    parser.add_argument("--count", type=int, default=10000)
    parser.add_argument("--template", default="SwiftUI")
    parser.add_argument("--no-trace", action="store_true", help="Verify fixture/replay only; leaves frame evidence unverified")
    parser.add_argument("--disable-materials", action="store_true", help="Diagnostic rendering-cost comparison only")
    parser.add_argument("--label", default="current")
    args = parser.parse_args()
    app = args.app.resolve()
    if not app.is_file() or not os.access(app, os.X_OK):
        parser.error(f"Build the app first, or set --app: {app}")
    output = args.output.resolve()
    if output.exists():
        parser.error("--output already exists; choose a new directory")
    output.mkdir(parents=True)
    fixture = args.fixture.resolve() if args.fixture else output / "fixture"
    suite = "is.edmundo.cuttings.performance." + uuid.uuid4().hex
    app_process = None
    recorder = None
    report = {
        "schema_version": 1, "label": args.label, "app": str(app), "fixture": str(fixture),
        "recording_scope": "app_process_only", "template": None if args.no_trace else args.template,
        "captured_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "frame_pacing_status": "unverified", "target_hz": 120,
        "frame_pacing_reason": "A successful replay or trace file does not prove presented frame deadlines.",
        "real_gesture_validation": "required", "status": "incomplete",
    }
    try:
        generated = subprocess.run([
            sys.executable, str(script_dir / "generate-performance-fixture.py"),
            str(fixture), "--count", str(args.count),
        ], check=True, capture_output=True, text=True)
        report["fixture_manifest"] = json.loads(generated.stdout)
        revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo,
                                  capture_output=True, text=True)
        report["revision"] = revision.stdout.strip() if revision.returncode == 0 else None
        worktree = subprocess.run(["git", "status", "--porcelain"], cwd=repo,
                                  capture_output=True, text=True)
        report["working_tree_dirty"] = bool(worktree.stdout.strip()) if worktree.returncode == 0 else None
        if not args.no_trace:
            templates = subprocess.run(["xcrun", "xctrace", "list", "templates"],
                                       check=True, capture_output=True, text=True)
            (output / "templates.txt").write_text(templates.stdout + templates.stderr)
            if args.template not in templates.stdout.splitlines():
                raise RuntimeError(f"Installed Instruments has no {args.template!r} template")

        environment = dict(os.environ)
        environment.update({
            "OIA_TEST_LIBRARY": str(fixture), "OIA_TEST_DB": str(output / "index.db"),
            "OIA_TEST_DEFAULTS": suite, "OIA_PERF_READY_PATH": str(output / "ready"),
            "OIA_PERF_TRIGGER_PATH": str(output / "start-replay"),
            "OIA_PERF_RESULT_PATH": str(output / "replay.json"),
            "OIA_PERF_DISABLE_MATERIALS": "1" if args.disable_materials else "0",
        })
        with (output / "app.log").open("w") as app_log, (output / "instruments.log").open("w") as trace_log:
            app_process = subprocess.Popen([str(app), "--performance-testing"], env=environment,
                                           stdout=app_log, stderr=subprocess.STDOUT)
            report["app_pid"] = app_process.pid
            wait_for(output / "ready", app_process, timeout=180)
            report["rss_kb_before_replay"] = int(subprocess.check_output(
                ["ps", "-o", "rss=", "-p", str(app_process.pid)], text=True).strip())
            if not args.no_trace:
                recorder = subprocess.Popen([
                    "xcrun", "xctrace", "record", "--template", args.template,
                    "--attach", str(app_process.pid), "--time-limit", "25s",
                    "--output", str(output / "scroll.trace"),
                ], stdout=trace_log, stderr=subprocess.STDOUT)
                time.sleep(3)
                if recorder.poll() is not None:
                    raise RuntimeError("Instruments exited before replay; see instruments.log")
            (output / "start-replay").touch()
            wait_for(output / "replay.json", app_process, timeout=60)
            report["replay"] = json.loads((output / "replay.json").read_text())
            report["rss_kb_after_replay"] = int(subprocess.check_output(
                ["ps", "-o", "rss=", "-p", str(app_process.pid)], text=True).strip())
            if recorder:
                stop(recorder)
                report["trace_exit_code"] = recorder.returncode
                if recorder.returncode != 0 or not (output / "scroll.trace").exists():
                    raise RuntimeError("Instruments did not finish a trace successfully; see instruments.log")
                exported = subprocess.run([
                    "xcrun", "xctrace", "export", "--input", str(output / "scroll.trace"), "--toc",
                    "--output", str(output / "trace-toc.xml"),
                ], capture_output=True, text=True)
                report["trace_toc_exported"] = exported.returncode == 0
            replay = report["replay"]
            if replay["status"] != "completed":
                raise RuntimeError(f"Replay did not scroll: {replay['status']}")
            starts = replay["counters"].get("scroll_starts", 0)
            report["swiftui_scroll_phase_observed"] = starts > 0
            # Missing phase notifications means this run did not reproduce the reported trigger.
            if starts == 0:
                raise RuntimeError("Scroll moved but SwiftUI reported no scroll starts; real gesture recording required")
            report["status"] = "replay_captured" if recorder else "replay_verified_without_trace"
    except (RuntimeError, subprocess.SubprocessError, OSError, ValueError) as error:
        report["error"] = str(error)
    finally:
        stop(recorder)
        stop(app_process)
        subprocess.run(["defaults", "delete", suite], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        (output / "report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"status": report["status"], "report": str(output / "report.json"),
                      "frame_pacing_status": "unverified", "error": report.get("error")}, indent=2))
    return 0 if report["status"] != "incomplete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
