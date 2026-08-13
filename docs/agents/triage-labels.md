# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Edit the right-hand column to match whatever vocabulary you actually use.

## Creating the labels

These labels do not exist in a fresh GitHub repo (except `wontfix`, which GitHub seeds
by default). Create the missing four once, against the tracker repo named in
[`issue-tracker.md`](./issue-tracker.md):

```sh
gh label create needs-triage    --repo johnnylibretexts/libretexts-reader --color FBCA04 --description "Maintainer needs to evaluate this issue"
gh label create needs-info      --repo johnnylibretexts/libretexts-reader --color D4C5F9 --description "Waiting on reporter for more information"
gh label create ready-for-agent --repo johnnylibretexts/libretexts-reader --color 0E8A16 --description "Fully specified, ready for an AFK agent"
gh label create ready-for-human --repo johnnylibretexts/libretexts-reader --color 1D76DB --description "Requires human implementation"
```
