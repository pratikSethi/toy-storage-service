# Service levels

!!! warning "Illustrative numbers"
    TS2 is a learning project. The numbers on this page show what a real service level page contains. Nobody is paged at 3am for TS2, and nothing here is a commitment you can build a business on.

## Three terms

- **SLI** (service level indicator): a measurement, such as the share of requests that succeed.
- **SLO** (service level objective): the target we set for an SLI, such as 99.9% over a month.
- **SLA** (service level agreement): an SLO with consequences if we miss it. Public clouds pay credits. Internally it is usually a review.

## What we publish

| | Target |
|---|---|
| **Availability** | 99.9% of valid requests succeed, per calendar month |
| **Latency** | p99 under 100 ms for GET and HEAD, and under 250 ms for PUT, on objects up to 1 MiB, measured at the endpoint |
| **Durability** | Design goal only, see below |

### Availability

Availability is the share of valid requests we answer successfully. We count it in 5-minute windows and average over the month:

> availability = successful requests / valid requests

- A **5xx** response, or a request that times out, counts as a failure and counts against us.
- A **4xx** does not. A bad bucket name, a missing key or a 413 is the caller's request, handled correctly. Public cloud SLAs work the same way.
- Requests that are not valid (malformed, or unauthenticated once API keys exist) are not in the denominator.

What 99.9% allows:

| Period | Allowed downtime at 99.9% |
|---|---|
| Per month (30 days) | 43.2 minutes |
| Per week | 10.1 minutes |
| Per day | 1.4 minutes |

We aim higher internally so we can react before you notice, see [operations](../engineering/operations.md).

### Latency

Targets are for small objects, because large uploads depend on your network. Time is measured from when the endpoint receives the request to when it sends the response headers.

### Durability

Today every object is stored once. If the disk holding it fails, the object is lost. Replication across machines is planned, and the design goal is that an object, once acknowledged, is not lost to a single machine failure. This is a goal, not a measured promise. Keep the original if you cannot afford to lose it.

## If we miss it

No credits. If a month falls below target we run an incident review, send our customers a short report on what happened and what changes, and add follow-up work to the backlog.

## How companies publish this

Public clouds publish an SLA page with a formula and service credits, for example the [Amazon S3 SLA](https://aws.amazon.com/s3/sla/). They also run status and health dashboards for live state. Internal teams usually put the SLO on a service catalog page like this one and keep the dashboards for engineers. The [SLO chapter of the Google SRE book](https://sre.google/sre-book/service-level-objectives/) explains how to pick SLIs and targets.
