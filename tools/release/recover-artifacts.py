"""Recover original publish bytes from this repository's release workflow only."""
import json
import os
import subprocess
from pathlib import Path
from artifact_contract import validate_artifacts


def validate_run(run, repository, run_id):
    if (str(run.get("id")) != run_id or run.get("repository", {}).get("full_name") != repository
            or run.get("path") != ".github/workflows/release.yml"
            or run.get("event") != "workflow_dispatch" or run.get("status") != "completed"):
        raise ValueError("Recovery source must be a completed release workflow in this repository")


def main():
    run_id = os.environ["RECOVERY_RUN_ID"]
    if not run_id.isdigit() or int(run_id) <= 0:
        raise ValueError("Invalid recovery run ID")
    repository = os.environ["GITHUB_REPOSITORY"]
    run = json.loads(subprocess.check_output(["gh", "api", f"repos/{repository}/actions/runs/{run_id}"]))
    validate_run(run, repository, run_id)
    subprocess.run(["gh", "run", "download", run_id, "--repo", repository,
                    "--pattern", "employee-*", "--dir", "recovery-download"], check=True)
    out = Path("release-output")
    out.mkdir(exist_ok=False)
    for directory in Path("recovery-download").iterdir():
        if not directory.is_dir():
            raise ValueError("Unexpected recovery artifact layout")
        for source in directory.iterdir():
            target = out / source.name
            if not source.is_file() or target.exists():
                raise ValueError("Unexpected or duplicate recovered artifact")
            target.write_bytes(source.read_bytes())
    manifest = json.loads(Path("connector.json").read_text(encoding="utf-8"))
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    validate_artifacts(out, manifest, commit, run_id)
    print("Recovered original publish artifacts from run " + run_id)


if __name__ == "__main__":
    main()
