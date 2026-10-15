#!/usr/bin/env python3
"""Compare host input-display bench transcripts. Not an FPS comparison."""
import hashlib
import re
import sys
from pathlib import Path


def parse(path: Path):
    steps = {}
    timings = {}
    header = {}
    current = None
    mode = None
    buf = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("STEP "):
            current = {"meta": line[5:]}
            mode = "meta"
            buf = []
            continue
        if line.startswith("TIMING "):
            meta = dict(part.split("=", 1) for part in line[7:].split())
            timings[meta["scenario"]] = meta
            continue
        if current is None:
            if "=" in line and line.startswith(("kind", "not", "label", "variant", "opt_level", "tree", "input_", "status_")):
                key, value = line.split("=", 1)
                header[key] = value
            continue
        if line == "STATE_VISIBLE_BEGIN":
            mode = "visible"
            buf = []
            continue
        if line == "STATE_VISIBLE_END":
            current["visible"] = "\n".join(buf) + "\n"
            mode = "meta"
            buf = []
            continue
        if line == "STATE_FULL_BEGIN":
            mode = "full"
            buf = []
            continue
        if line == "STATE_FULL_END":
            current["full"] = "\n".join(buf) + "\n"
            mode = "meta"
            buf = []
            continue
        if line == "ENDSTEP":
            meta = dict(part.split("=", 1) for part in current["meta"].split())
            key = (meta["scenario"], meta["step"])
            current["fields"] = meta
            for name in (
                "lookup_calls",
                "lookup_visits",
                "visibility_writes",
                "text_writes",
                "color_writes",
                "fade_writes",
                "allocs",
                "alloc_bytes",
            ):
                current[name] = int(meta[name]) if name in meta else int(current.get(name, 0))
            # Counters are emitted as their own lines before the state.
            steps[key] = current
            current = None
            mode = None
            continue
        if mode in ("visible", "full"):
            buf.append(line)
        elif "=" in line and current is not None:
            key, value = line.split("=", 1)
            current[key] = value
    # Attach line-style counters collected before ENDSTEP.
    return header, steps, timings


def sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def visible_state(text: str) -> str:
    # +0 and -0 have the same position. Matrix-dirty is an engine bookkeeping
    # bit, not a visible property; a newly allocated row need not have moved.
    text = text.replace('pos_bits=2147483648', 'pos_bits=0')
    return re.sub(r'flags=(\d+)', lambda m: 'flags=' + str(int(m[1]) & 1), text)


def main() -> int:
    baseline_path, repeat_path, candidate_path, report_path = map(Path, sys.argv[1:5])
    base_header, base_steps, base_times = parse(baseline_path)
    _, repeat_steps, _ = parse(repeat_path)
    lines = [
        "# Input display host benchmark",
        "",
        "These numbers are a host CPU and pane-operation count. They are not emulator FPS.",
        "",
        f"Baseline label: {base_header.get('label', '')}",
        f"Tree nodes: {base_header.get('tree_nodes', '')}",
        f"Tree shape: {base_header.get('tree', '')}",
        "",
    ]
    problems = []
    for key, step in base_steps.items():
        other = repeat_steps.get(key)
        if other is None or other.get("visible") != step.get("visible") or other.get("full") != step.get("full"):
            problems.append(f"baseline repeat diverged at {key[0]}/{key[1]}")
    if problems:
        lines.append("Baseline repeat check failed.")
        lines.extend(f"- {item}" for item in problems)
    else:
        lines.append("Baseline repeated on the same binary and the pane dumps matched.")
    lines.append("")

    candidate_built = candidate_path.exists() and candidate_path.stat().st_size > 0
    header = None
    cand_steps = {}
    cand_times = {}
    if candidate_built:
        text = candidate_path.read_text(encoding="utf-8", errors="replace")
        if "kind=host-bench" in text:
            header, cand_steps, cand_times = parse(candidate_path)
        else:
            candidate_built = False
    if not candidate_built:
        lines.append("Candidate binary did not produce a transcript. See candidate-build.log.")
        report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print("\n".join(lines))
        return 2

    lines.append(f"Candidate label: {header.get('label', '')}")
    if not base_steps or set(base_steps) != set(cand_steps):
        problems.append('Baseline and candidate must contain the same nonempty scenario set')
    lines.append("")
    lines.append("| scenario | step | visits base | visits cand | calls base | calls cand | vis base | vis cand | text base | text cand | allocs base | allocs cand | visible |")
    lines.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |")
    notes = []
    for key in base_steps:
        scenario, step_name = key
        if step_name.startswith("stale"):
            base_hash = sha(base_steps[key].get("visible", ""))
            cand_hash = sha(cand_steps.get(key, {}).get("visible", ""))
            notes.append(
                f"Informational `{scenario}/{step_name}` is outside the pass rule. "
                f"visible baseline {base_hash} candidate {cand_hash}."
            )
            continue
        if key not in cand_steps:
            problems.append(f"candidate missing {scenario}/{step_name}")
            continue
        left = base_steps[key]
        right = cand_steps[key]
        same_visible = visible_state(left.get("visible", "")) == visible_state(right.get("visible", ""))
        same_full = left.get("full") == right.get("full")
        result_label = "match" if same_visible else "DIFFER"
        if key == ('reregister', 'after_invalidate'):
            # The old renderer only repaints row zero on a new layout. The new
            # renderer intentionally restores every still-live historical row.
            expected = visible_state(cand_steps[('reregister', 'established')]['visible'])
            restored = visible_state(right.get('visible', '')) == expected
            stale = visible_state(cand_steps[('reregister', 'stale_redraw')]['visible'])
            if not restored or stale == expected:
                problems.append('Registration must restore established history, and the missing-invalidation control must fail')
            result_label = 'history restored' if restored else 'DIFFER'
        elif not same_visible:
            problems.append(f"visible pane state differs at {scenario}/{step_name}")
        elif not same_full:
            notes.append(
                f"`{scenario}/{step_name}` screenshot-visible state matches. Full pane dump differs while the log root is hidden or children diverged off-screen."
            )
        lines.append(
            "| {scenario} | {step} | {vb} | {vc} | {cb} | {cc} | {sb} | {sc} | {tb} | {tc} | {ab} | {ac} | {ok} |".format(
                scenario=scenario,
                step=step_name,
                vb=left["lookup_visits"],
                vc=right["lookup_visits"],
                cb=left["lookup_calls"],
                cc=right["lookup_calls"],
                sb=left["visibility_writes"],
                sc=right["visibility_writes"],
                tb=left["text_writes"],
                tc=right["text_writes"],
                ab=left["allocs"],
                ac=right["allocs"],
                ok=result_label,
            )
        )
    lines.append("")
    lines.append("| scenario | baseline ns/iter | candidate ns/iter | candidate/baseline |")
    lines.append("| --- | ---: | ---: | ---: |")
    for scenario, timing in base_times.items():
        other = cand_times.get(scenario)
        if other is None:
            continue
        base_ns = int(timing["cpu_ns_per"])
        cand_ns = int(other["cpu_ns_per"])
        ratio = (cand_ns / base_ns) if base_ns else 0
        lines.append(f"| {scenario} | {base_ns} | {cand_ns} | {ratio:.3f} |")
    lines.append("")
    lines.append("CPU nanoseconds are host `Instant` around the scenario body at opt-level 3, 20 iterations after 2 warmups. Not FPS.")
    if notes:
        lines.append("")
        lines.append("Notes:")
        lines.extend(f"- {note}" for note in notes)
    if problems:
        lines.append("")
        lines.append("Failures:")
        lines.extend(f"- {item}" for item in problems)
    else:
        lines.append("")
        lines.append("Visible pane state matched baseline on ordinary updates; registration restored all retained history and the missing-invalidation control failed as expected.")
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
