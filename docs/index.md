---
title: TS2, the Toy Storage Service
hide:
  - navigation
  - toc
---

# TS2, the Toy Storage Service

**Store a file under a name, get the same bytes back later.**

TS2 is an internal object store with a plain JSON REST API. You put files in named buckets and read them back by key. It is also a learning project, built one component per commit.

<div class="grid cards" markdown>

-   :material-account-arrow-right: **Use TS2**

    For teams who want to store files.

    [Customer guide](guide/overview.md) · [Getting started](guide/getting-started.md) · [Service levels](guide/service-levels.md) · [API reference](api/index.md) · [CLI reference](cli/index.md)

-   :material-cogs: **How TS2 works**

    For engineers, PMs and anyone learning how it is built.

    [Engineering](engineering/index.md) · [Components](engineering/components.md) · [Operations](engineering/operations.md) · [System overview](design/000-system-overview.md) · [Plan](plan.md)

</div>

## Try it in a minute

Start the service with `make run`, then in another terminal:

```sh
uv tool install --editable ./cli
ts2 mb demo
ts2 cp hello.txt ts2://demo/docs/hello.txt
ts2 ls demo
```

More in [getting started](guide/getting-started.md).
