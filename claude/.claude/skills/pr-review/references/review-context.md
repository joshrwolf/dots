# Review context and local conversations

Use `scripts/review-context` relative to this skill's installed directory. It
resolves the shared Herdr-review binary from the physical skill source and
passes the current working directory explicitly. Run it from the agent's
checkout, not from the skill directory. No Herdr focus changes are required.
The executable accepts one JSON request on stdin and prints one JSON response.
Use a JSON encoder for bodies containing quotes or newlines, not shell string
interpolation. The helper performs no GitHub queries or database logic.

## Load

Send:

```json
{"id":1,"method":"context.load","params":{}}
```

The envelope contains either `result` or `error`. An error also has a nonzero
process exit status; do not interpret it as an empty review. A null result means
there is no bound review in this checkout.

A successful context result contains `checkout_root`, `common_git_dir`,
`head_oid`, `working_tree_dirty`, and `state`:

- `state.context`: identity, title, and external `references` (`kind`, `locator`).
- `state.observation`: identity and captured `comparison.base.oid` and
  `comparison.target.oid`; each revision also records its source.
- `state.binding`: identity, owning workspace, and checkout root.
- `state.threads`: saved local threads and messages from both reviewer and agent.
- `state.agent_requests`: agent requests with stable request and reserved-message
  IDs, and their dispatch attempts. Reservation prevents duplicate sends; it
  does not mean delivery succeeded or an answer was saved.

State is paged. When `state.next_cursor` is non-null, load the next page by
passing that exact object as `params.cursor` to `context.load`. Continue until
`next_cursor` is null before claiming to have inspected all saved findings or
answers. Pages can repeat thread or request IDs: merge their messages or dispatch
attempts by ID, including repeated fragments within the same page. Do not mistake
a partial thread for its complete history. The cursor carries a context revision
and observation identity; if the tool returns `stale_cursor`, discard the partial
result and restart with empty params. Never construct or edit cursor fields.

`state.revision` is a change token, not a review phase or schema version. Request
`recovery` is null when no recovery is available, `retry` when it is safe to
retry, or `inspect_before_retry` when an uncertain earlier delivery needs human
inspection. Saved answers remain independent of the dispatch outcome.

Read the reference with `kind: "github.pull_request"`; its `locator` is the
explicit PR URL. Do not derive it from
the branch. If there is no single PR reference, stop and ask which review is
intended. Compare `head_oid` with the captured target and examine
`working_tree_dirty` before relying on live files. When dirty, inspect local
changes with Git directly; the tool deliberately does not embed an unbounded
status listing in its response. Use the captured commit
objects for the comparison even if local edits exist.

## Save or revise a finding

Use binding and observation IDs from the loaded state. `finding.save` accepts
the following shape (example IDs must be replaced with returned IDs):

```json
{
  "id":2,
  "method":"finding.save",
  "params":{
    "binding_id":"<binding-id>",
    "observation_id":"<observation-id>",
    "finding":{
      "finding":{
        "key":"release-success-before-cleanup",
        "kind":"finding",
        "severity":"non_blocking",
        "title":"Cleanup failure can repeat a successful release",
        "evidence":"Success is recorded after fallible cleanup; the caller retries the whole release.",
        "related_locations":[
          {"path":"internal/worker.go","side":"target","start_line":88,"end_line":90}
        ]
      },
      "location":{"path":"internal/release.go","side":"target","start_line":142,"end_line":142},
      "body":"Persist release success independently of retryable issue cleanup.",
      "author":"codex"
    }
  }
}
```

- Set `author` to the actual agent identity, not the human reviewer.
- `kind` is `finding`, `question`, or `design`. `severity` is null, `blocking`,
  `non_blocking`, or `nit`; do not manufacture defect severity for a preference.
- `location` is the primary comment placement; set it to null for a general
  review-level finding. Related locations provide evidence or fix sites, not
  additional copies of the finding.
- Paths are repository-relative. Lines are one-based, inclusive, and refer to
  the named side of the captured comparison. The backend captures the anchor.
- Keep `key` stable for the same concern across invocations. Reuse an existing
  finding's key when revising it; do not include a run timestamp or current line
  number in the key. Check existing threads before introducing a new key.

The result is the updated binding state. Retain returned thread identities for
continuation. If a binding or observation changed, reload context and reassess
the comparison before retrying; never blindly write against a different PR.
Saving findings does not dispatch or post anything to GitHub.

## Reply to an agent request

Use the binding, request, dispatch attempt, and thread IDs in the delivery
prompt, not a guessed current request. Send through the same helper:

```json
{"id":3,"method":"thread.reply","params":{"binding_id":"<binding-id>","request_id":"<request-id>","attempt_id":"<dispatch-attempt-id>","thread_id":"<thread-id>","message":{"author":"codex","body":"Your answer to the new reviewer messages."}}}
```

Save one complete answer per thread per request. Identical retries return the
existing message; different content under the same request/thread is rejected.
Retries keep the request and its new-message IDs but use a new dispatch attempt.
Read answers already saved for the request and answer only missing threads;
do not repeat edits or other actions described in earlier answers. If the
attempt was superseded, do not guess a replacement ID or write against the new
assignment. Report that the attempted save is stale.
Messages have explicit `origin` (`reviewer` or `agent`) and `reply_to_request`.
Do not infer authorship from a display name. Agent replies are context for later
exchanges, never new messages to send back to the agent. The editor refreshes
its binding independently of dispatch activity; no Neovim control or tab
switching is needed.

If the save fails, include the answer in chat and disclose the failure. A turn
finishing is not proof that answers were saved. Do not resolve threads merely
because you answered them; resolution is optional reviewer housekeeping.
