#!/usr/bin/env python3
"""Stage the model owner's allowlist privately; explicitly promote reviewed clean files.

Run on the release source host with its existing Hugging Face credentials.
The source tree is read-only. Without --publish this only validates and plans.
--publish targets the private development branch. --promote additionally needs
--staging-revision and verifies the clean export matches that private snapshot.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
from urllib.parse import quote

REPOSITORY = "ethiopic/ethiopic-studio-models"
STAGING_REPOSITORY = "ethiopic/ethiopic-studio-models-private"
DOC_PATHS = {
    "README.md", "NOTICE", "LICENSES/Boson-Higgs-Audio-2.txt",
    "LICENSES/Meta-Llama-3.txt", "LICENSES/Pocket-TTS-terms.txt",
    "cards/asr.md", "cards/corrector.md", "cards/tts.md", "cards/codec.md",
    "cards/editing.md",
}

# A file allowlist alone does not make training-side documentation public-safe.
# Reject these references instead of silently editing the model owner's files.
PUBLIC_TEXT_RULES = {
    "private filesystem path": re.compile(
        r'(?i)(?:/(?:home|Users)/[^/\\\r\n"<>]+/|[A-Z]:[/\\]+Users[/\\]+[^/\\\r\n"<>]+[/\\])'),
    "private network address": re.compile(
        r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|"
        r"172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b"),
    "internal experiment reference": re.compile(
        r"(?i)(?:\b_work_[\w-]+|\b_validate_[\w-]+|\basr_finetune\b|"
        r"\bresearch/low_end_inference\b|\bOmniVoice/exp/|\bruns/(?:distill|finetune)_|"
        r"\bruns/20\d{2}-|\bTTS_small/scripts/|\bAMHARIC_FINETUNE\.md\b|"
        r"\beval_cer\d+\b|\bsamaudio_env\b|\bomnivoice_env\b)"),
    "unfinished publication note": re.compile(
        r"(?i)(?:\bTODO\b|\bFIXME\b|not verified offline|still to be confirmed)"),
    "credential marker": re.compile(
        r"\b(?:hf_[A-Za-z0-9]{25,}|ghp_[A-Za-z0-9]{25,}|github_pat_[A-Za-z0-9_]{25,})\b|"
        r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
}


def validate_public_text(text, context):
    for label, pattern in PUBLIC_TEXT_RULES.items():
        if pattern.search(text):
            # Do not echo a matched path, credential or host into CI logs.
            raise ValueError(f"Public release contains {label}: {context}")


def validate_public_release(manifest, files):
    """Inspect only the selected release metadata/text, never execute provider code."""
    validate_public_text(json.dumps(manifest, ensure_ascii=False), "manifest.json")
    for name, entry in files.items():
        if (Path(name).name not in {"LICENSE", "NOTICE"}
                and Path(name).suffix.lower() not in {".md", ".txt", ".py", ".json", ".yaml", ".yml", ".toml", ".cfg"}):
            continue
        # Large tokenizer vocabularies are model data, not publication documents.
        if entry["bytes"] > 4 * 1024 * 1024:
            if Path(name).name not in {"tokenizer.json", "vocab.json", "char_vocab.json"}:
                raise ValueError(f"Public release text needs separate inspection: {name}")
            continue
        validate_public_text(entry["source"].read_text(encoding="utf-8"), name)


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def safe_path(value):
    if (not isinstance(value, str) or not value or "\\" in value
            or any(ord(c) < 32 for c in value)
            or PurePosixPath(value).is_absolute()
            or any(p in ("", ".", "..") for p in value.split("/"))):
        raise ValueError(f"Unsafe release path: {value!r}")
    return value


def model_files(source, manifest):
    if manifest.get("manifest_version") not in (1, 2) or not manifest.get("variants"):
        raise ValueError("Expected a nonempty version-1 or version-2 Studio manifest")
    result, ids, folders = {}, set(), set()
    for variant in manifest["variants"]:
        identifier, folder = variant["id"], safe_path(variant["folder"])
        if not re.fullmatch(r"[a-z0-9][a-z0-9._-]*", identifier):
            raise ValueError("Invalid variant ID")
        if identifier in ids or folder in folders or "/" in folder or folder.startswith("_"):
            raise ValueError("Duplicate or invalid variant folder")
        ids.add(identifier)
        folders.add(folder)
        total = 0
        if not variant["files"]:
            raise ValueError("Empty variant")
        for entry in variant["files"]:
            relative = safe_path(entry["path"])
            target = f"{folder}/{relative}"
            if (target in result or "__pycache__" in relative.split("/")
                    or any(p.startswith(("_work_", "_validate_")) for p in relative.split("/"))
                    or relative == "build_manifest.py"):
                raise ValueError("Duplicate or disallowed release file")
            path = source / target
            size, sha = entry["bytes"], entry["sha256"]
            if (type(size) is not int or size < 0
                    or not re.fullmatch(r"[0-9a-f]{64}", sha)):
                raise ValueError("Invalid size or checksum")
            if not path.is_file() or path.stat().st_size != size or digest(path) != sha:
                raise ValueError(f"Release content does not match manifest: {target}")
            result[target] = {"source": path, "bytes": size, "sha256": sha}
            total += size
        if total != variant["total_bytes"]:
            raise ValueError(f"Incorrect total_bytes: {identifier}")
    for variant in manifest["variants"]:
        if any(dep not in ids for dep in variant.get("requires", [])):
            raise ValueError("Missing model dependency")
        alternatives=variant.get("requires_one_of",[])
        if (len(set(alternatives))!=len(alternatives)
                or any(dep not in ids or dep==variant['id'] for dep in alternatives)):
            raise ValueError("Missing or invalid alternative model dependency")
    for bundle in manifest["bundles"].values():
        for role in ("asr", "asr_live", "corrector", "tts", "tts_optin", "edit", "edit_optin"):
            if bundle.get(role) is not None and bundle[role] not in ids:
                raise ValueError("Bundle references an unpublished variant")
    return result


def hosted_manifest(source, revision, repository=REPOSITORY):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Pin the immutable model upload commit, never main")
    result = copy.deepcopy(source)
    if repository not in (REPOSITORY, STAGING_REPOSITORY):
        raise ValueError("Unexpected repository")
    result["hf_repo"] = repository
    result["hf_revision"] = revision
    return result


def file_url(manifest, folder, path):
    safe_path(folder)
    safe_path(path)
    if manifest["hf_repo"] not in (REPOSITORY, STAGING_REPOSITORY):
        raise ValueError("Unexpected repository")
    hosted_manifest({}, manifest["hf_revision"])
    return (f"https://huggingface.co/{manifest['hf_repo']}/resolve/{manifest['hf_revision']}/"
            f"{quote(folder, safe='/')}/{quote(path, safe='/')}")


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_bytes(json_bytes(value))
    temporary.replace(path)


def verify_tree(info, expected):
    files = {item.rfilename: item for item in info.siblings}
    for name, entry in expected.items():
        remote = files.get(name)
        if remote is None or remote.size != entry["bytes"]:
            raise ValueError(f"Missing or wrong-sized remote file: {name}")
        lfs = remote.lfs
        if lfs:
            sha = lfs.get("sha256") if isinstance(lfs, dict) else lfs.sha256
            if sha != entry["sha256"]:
                raise ValueError(f"Remote LFS checksum mismatch: {name}")
        else:
            data = entry["source"].read_bytes()
            blob = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
            if remote.blob_id != blob:
                raise ValueError(f"Remote Git content mismatch: {name}")


def publication_target(promote=False, staging_revision=None):
    if promote:
        if not isinstance(staging_revision, str) or not re.fullmatch('[0-9a-f]{40}', staging_revision):
            raise ValueError("Public promotion requires an exact reviewed private staging revision")
        return REPOSITORY, None, False
    if staging_revision is not None:
        raise ValueError("Use --staging-revision only with --promote")
    return STAGING_REPOSITORY, 'development', True


def publish(source, output, docs, promote=False, staging_revision=None):
    from huggingface_hub import CommitOperationAdd, HfApi

    subprocess.run([sys.executable, "build_manifest.py", "--check"], cwd=source, check=True)
    raw = (source / "manifest.json").read_bytes()
    manifest = json.loads(raw)
    expected = model_files(source, manifest)
    for name in sorted(DOC_PATHS):
        path = docs / name
        if not path.is_file():
            raise ValueError(f"Missing publication documentation: {name}")
        expected[name] = {"source": path, "bytes": path.stat().st_size, "sha256": digest(path)}

    repository, branch, private = publication_target(promote, staging_revision)
    if promote:
        validate_public_release(manifest, expected)

    output.mkdir(parents=True, exist_ok=True)
    (output / "manifest-source.json").write_bytes(raw)
    api = HfApi()
    api.create_repo(repository, repo_type="model", private=private, exist_ok=True)
    current = api.model_info(repository, files_metadata=True, timeout=30)
    if current.private != private or current.gated:
        raise ValueError("Repository visibility does not match the publication target")
    if promote:
        staged = api.model_info(STAGING_REPOSITORY, revision=staging_revision, files_metadata=True, timeout=30)
        if not staged.private:
            raise ValueError("Staging must be a separate private repository, not a public branch")
        verify_tree(staged, expected)
    else:
        api.create_branch(repository, branch=branch, revision=current.sha, repo_type='model', exist_ok=True)
        current = api.model_info(repository, revision=branch, files_metadata=True, timeout=30)
    try:
        verify_tree(current, expected)
        models_commit = current.sha
        print("All release files already match; reusing", models_commit, flush=True)
    except ValueError:
        operations = [CommitOperationAdd(path_in_repo=name, path_or_fileobj=entry["source"])
                      for name, entry in sorted(expected.items())]
        print(f"Uploading exact allowlist: {len(manifest['variants'])} variants, "
              f"{len(operations)} files including publication notices", flush=True)
        commit = api.create_commit(
            repo_id=repository, repo_type="model", revision=branch, operations=operations,
            parent_commit=current.sha, commit_message="Publish Studio ASR, corrector and TTS variants",
            num_threads=4,
        )
        models_commit = commit.oid
    write_json(output / "models-upload.json", {"repo": repository, "models_commit": models_commit})
    verify_tree(api.model_info(repository, revision=models_commit, files_metadata=True, timeout=30), expected)
    if (source / "manifest.json").read_bytes() != raw:
        raise ValueError("Owner changed the manifest during upload; re-run validation before publishing it")

    hosted = hosted_manifest(manifest, models_commit, repository)
    payload = json_bytes(hosted)
    (output / "manifest.json").write_bytes(payload)
    urls = {variant["id"]: [file_url(hosted, variant["folder"], file["path"])
                             for file in variant["files"]] for variant in hosted["variants"]}
    write_json(output / "download-urls.json", urls)
    latest = api.model_info(repository, revision=branch, timeout=30)
    if latest.sha != models_commit:
        raise ValueError("Repository changed during publication; inspect before updating the manifest")
    committed = api.create_commit(
        repo_id=repository, repo_type="model", revision=branch, parent_commit=models_commit,
        operations=[CommitOperationAdd(path_in_repo="manifest.json", path_or_fileobj=payload)],
        commit_message="Publish Studio manifest pinned to verified model upload",
    )
    receipt = {
        "repo": repository, "branch": branch or 'main', "private": private,
        "models_commit": models_commit, "manifest_commit": committed.oid,
        "source_manifest_sha256": hashlib.sha256(raw).hexdigest(),
        "hosted_manifest_sha256": hashlib.sha256(payload).hexdigest(),
        "variants": len(manifest["variants"]),
        "model_files": sum(len(v["files"]) for v in manifest["variants"]),
        "model_bytes": sum(v["total_bytes"] for v in manifest["variants"]),
        "manifest_url": f"https://huggingface.co/{repository}/resolve/{branch or 'main'}/manifest.json",
    }
    write_json(output / "publication-receipt.json", receipt)
    print(json.dumps(receipt), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--documentation", type=Path)
    parser.add_argument("--publish", action="store_true")
    parser.add_argument("--promote", action="store_true", help="Explicitly publish reviewed clean staged files to the public repo")
    parser.add_argument("--staging-revision", help="Exact private staging commit reviewed for public promotion")
    args = parser.parse_args()
    if args.promote and not args.publish:
        parser.error('--promote requires --publish')
    publication_target(args.promote, args.staging_revision)
    source, output = args.source.resolve(), args.output.resolve()
    if output == source or source in output.parents:
        raise ValueError("Keep publication output outside the model owner's source tree")
    if args.publish:
        if not args.documentation:
            parser.error("--publish requires --documentation")
        output.mkdir(parents=True, exist_ok=True)
        # The release source host is Linux; avoid concurrent commits from duplicate invocations.
        import fcntl
        with (output / ".publish.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            publish(source, output, args.documentation.resolve(), args.promote, args.staging_revision)
    else:
        manifest = json.loads((source / "manifest.json").read_text())
        files = model_files(source, manifest)
        print(json.dumps({"repo": STAGING_REPOSITORY, "private": True, "variants": len(manifest["variants"]),
                          "files": len(files), "bytes": sum(e["bytes"] for e in files.values())}))


if __name__ == "__main__":
    main()
