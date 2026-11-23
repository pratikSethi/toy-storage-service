# Service levels

!!! warning "Illustrative numbers"
    TS2 is a learning project. Every number marked with a superscript footnote on this page is made up for the demo. They show what a real service level page contains. Nobody is paged at 3am for TS2, and nothing here is a commitment you can build a business on.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **p99**: the value that 99 of 100 requests beat. A p99 latency of 100 ms means 99 requests in 100 finish within 100 ms.

## Three terms

- **SLI** (service level indicator): a measurement. Example: good PutObject requests divided by valid PutObject requests, in each 5-minute window.
- **SLO** (service level objective): the target for that SLI over a period. Example: 99.99% over 30 days.
- **SLA** (service level agreement): the promise to you, with consequences if we miss it. It is set looser than the SLO, so we have room to react before we break the promise.

## What we publish

Five service levels cover the whole service. Operations differ in how much they matter, so availability comes in two tiers.

| Service level | Covers | Target |
|---|---|---|
| **Durability** | Every stored object | 99.999999999% (11 nines) per year[^dur] |
| **Data availability, Tier 0** | PutObject, GetObject, HeadObject, DeleteObject | 99.99% per month[^t0] |
| **Management and listing availability, Tier 1** | CreateBucket, DeleteBucket, GetBucket, ListBuckets, ListObjects | 99.9% per month[^t1] |
| **Latency** | GetObject time to first byte, and PutObject, on objects up to 1 MiB | p99 under 100 ms for GetObject and under 250 ms for PutObject[^lat] |
| **Consistency** | All operations | Strong read-after-write and list-after-write[^cons] |

### Durability

Durability is the chance an object you stored is still there, byte for byte, a year later. 11 nines means that if you store 10 million objects, you expect to lose one every 10,000 years. For an object store this is the most important number, because lost data cannot be given back with an apology or a credit.

This is a design target, not something we can measure directly. It is modelled from disk failure rates, how many copies exist and how fast we repair them.

### Availability

Availability is the share of valid requests we answer successfully. We count it in 5-minute windows and average over the month:

> availability = successful requests / valid requests

- A **5xx** response, or a request that times out, counts as a failure and counts against us.
- A **4xx** does not. A bad bucket name, a missing key or a 413 is the caller's request, handled correctly. Public cloud SLAs work the same way.
- Requests that are not valid (malformed, or unauthenticated once API keys exist) are not in the denominator.

**Tier 0 is the data path.** Putting, getting, checking and deleting objects is what the service is for, so it gets the highest bar.

**Tier 1 is lower on purpose.** These calls are rarer than reads and writes, and clients can retry them safely. They are also not on the data path: if listing is down, your applications can still read and write objects they already know by key.

What each tier allows:

| Period | Tier 0 (99.99%) | Tier 1 (99.9%) |
|---|---|---|
| Per month (30 days) | 4.3 minutes | 43.2 minutes |
| Per week | 1.0 minute | 10.1 minutes |
| Per day | 8.6 seconds | 1.4 minutes |

We aim higher internally so we can react before you notice, see [service levels for engineers](../engineering/service-levels.md).

### Latency

Targets are for objects up to 1 MiB, because large transfers depend on your network. For GetObject we measure the time to first byte: from when the endpoint receives the request to when it sends the first byte of the body. For PutObject we measure the time until the response is sent. Both are p99, meaning 99 requests in 100 are at least this fast.

### Consistency

Once a write succeeds, every later read sees it. Once a PutObject or DeleteObject succeeds, the next ListObjects shows the change. This is a property we test continuously with probe requests, not a percentage with a budget. A single stale read is a bug.

## If we miss it

No credits. If a month falls below target we run an incident review, send our customers a short report on what happened and what changes, and add follow-up work to the backlog.

## How companies publish this

Public clouds publish an SLA page with a formula and service credits, for example the [Amazon S3 SLA](https://aws.amazon.com/s3/sla/). They also run status and health dashboards for live state. Internal teams usually put the SLOs on a service catalog page like this one and keep the dashboards for engineers. The [SLO chapter of the Google SRE book](https://sre.google/sre-book/service-level-objectives/) explains how to pick SLIs and targets.

[^dur]: Illustrative, made up for the demo. Today TS2 keeps one copy of each object, so real durability is that of one disk. Replication and erasure coding are planned.
[^t0]: Illustrative, made up for the demo.
[^t1]: Illustrative, made up for the demo.
[^lat]: Illustrative, made up for the demo. The 100 ms and 250 ms figures are not measured on real traffic.
[^cons]: The property is real for the single-node design today, but the continuous probe that tests it is planned, so the claim is illustrative for now.
