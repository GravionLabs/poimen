# Directory fixtures

What the [`jsonl-file` binding](../../bindings/jsonl-file.md) says about a directory of run files: which runs a reader lists, and which files a writer may delete. A reader or writer in any language checks itself against these cases; `test/directory.ts` does it for this repository.

## Layout

```text
fixtures/directory/<case>/
  shared/<run>.jsonl   the shared directory (per user, or AGENT_EVENTS_DIR)
  legacy/<run>.jsonl   the legacy directory <git-common-dir>/.agents/events/, when the case has one;
                       read with or without AGENT_EVENTS_DIR
  expected.json
```

## expected.json

| Field | Meaning |
| :-- | :-- |
| `now` | the clock of the reader or writer |
| `files` | every run file of the case (path relative to the case) with its `mtime`. A checkout does not keep modification times, so set or assume these. |
| `runs` | reader cases: the runs listed, each once, in order of the run id: `run`, `repo` (`origin.repo`, absent without one), `status`, `seqs` (the events after deduplication by `(run, seq)`) |
| `writer` | retention cases: the writer that prunes, with its `origin.repo` |
| `mayDelete` | retention cases: exactly the files this writer MAY delete, sorted. Every other file stays. |

No run records a `jsonl-file.pid`, so a run without `run.finished` is judged by the file age alone.

## Cases

| Case | Shows |
| :-- | :-- |
| `two-repositories` | runs of two repositories in one directory, both listed |
| `shared-and-legacy` | a run in the shared and the legacy directory is listed once; a run only in the legacy directory (no `origin`) is listed too |
| `retention-per-repository` | a writer of `example/a` may delete its old finished and abandoned runs and old runs without `origin`; never its running run, its recent runs, the runs of `example/b` or those without a repository |

The run ids of the retention case are readable names so that `expected.json` explains itself; a writer uses globally unique ids.
