---
name: get-next-matching-issue
description: "Asks Claude to work on the next issue matching a list of labels."
argument-hint: "[comma-separated label-list like: label,label,...]"
disable-model-invocation: true
user-invocable: true
---

The user is requesting you work on the next open issue labeled-with `$ARGUMENTS`. 
We will run a command that will search for relevant issues, determine their ready-for-work-ness, and then inform you vis-a-vis which of the following three situations is applicable:

1. there are no open issues labeled-with `$ARGUMENTS`:
    - tell user there are no open issues to work on
2. there are open issues labeled-with `$ARGUMENTS`, but they're all (transitively) blocked by issues outside that set:
    - you'll be given a table of matching issues and another table of their blockers
    - you should analyze the issues and their blockers
    - you should make a report to the user
    - you *may* include suggestions for what to do, but only that (do not just start working on an issue)
3. there are open, unblocked issues labeled-with `$ARGUMENTS`:
    - you'll be directly given the content for a ticket
    - you should tell the user what you're working on, then get to work

Without further ado, here is the output from that command:

!`just find-next-matching-issue $ARGUMENTS`
