# Dogfood Postmortem

## Result
Helpful.

## What the packet got right
- It exposed current dirty workspace state before more edits.
- It included relevant release-readiness code/doc targets.
- It included validation guidance for Rust release gates.
- It made generated/local artifact risk visible before commit.

## What the packet got wrong / needs product work
- It is still larger than ideal for a handoff artifact.
- Strict validation correctly fails warning-bearing packets, so dogfood should capture both strict and non-strict validation outputs.
- The product should keep treating benchmark token overhead as claim-blocking until measured benefit improves.

## Product fixes implied
- Keep compact injection as the default when warnings or large code excerpts are present.
- Keep average Layers overhead tokens as a claim gate.
- Preserve strict confidence/injection-policy consistency.

## Follow-up tasks
- Add a fresh-clone stable-core proof.
- Continue reducing targeted-preflight token overhead.
