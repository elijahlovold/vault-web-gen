---
type: writings
status: complete
created: 2026-09-02
tags:
 - tools
 - shared
 - printed
---

> Elijah Lovold, 2026-09-02

> [!info] Claim
> You should spend non-insignificant amount of time optimizing your workflow

# 1 - Time Invested

The first, most obvious, objection is that time invested > time saved for most cases. Granted; however, we're often not optimizing for total seconds saved, but rather total throughput of work done. "Work" being used here as a function of mental difficulty / complexity as well, not just bulk information processed / changed.

This becomes immediately relevant considering the time throughout a day / week is not equally [[Productivity|productive]]. For most mental tasks, it varies wildly throughout the day, with the exact peak time varying person-to-person. Further, I would argue, in the average case, the deviation from peak is rather large when sampled over the day. Thus, if our goal is optimizing the total amount of work per day, it quickly changes from an availability problem to a scheduling / execution problem. E.g., our 3 highest productivity hours might generate the same output as the rest of the day, in which case we can afford to spend more scheduling / optimization time in less productive periods. Since the amount of output is a product of productivity and time, workflow optimization should be evaluated as a function of the productivity during which the costs and benefits. E.g., we could justify 4 hours setting up + customizing nvim in order to save 2 hours of productive time, if the productivity ratio between the time periods invested is 2x or more.

# 2 - Friction and Flow

The more important factor beyond the previously mentioned arithmetic is reducing [[Mental-Friction]], since friction affects both whether you consider a task worthwhile and, if worthwhile, the amount of [[Mental-Energy]] it takes to execute it. Friction disrupts flow, and flow is most critical to maintain during productive hours. Also, I might choose not to do a task that has too much friction, degrading the efficiency of downstream tasks. E.g., I might not pull up a new window to reference the exact code if it takes > 15s, whereas I might if I have a set of keybindings to pull it up in 5s, which would help me execute my current task significantly faster.

# 3 - Fixed Investment and Nonlinear Compounding

Two last quick observations: workflow improvements often require fixed-time investment and yield nonlinear accumulation. Regarding the first point, any useful optimization will amortize its fixed cost if used long enough. An example regarding the latter: using nvim on Windows will help; but, using nvim + tmux + hyprland + quickshell + GNU core utils unlocks a ton of synergistic combinations. E.g., think of hotkeys, chords, and commands that start interfacing cross-tool or are only possible after knowing multiple tools.
