# Service levels for engineers

The internal view of the [customer service levels](../guide/service-levels.md). This page defines what each service level measures, what we run to, when we alert, and who looks at which number.

!!! warning "Illustrative numbers"
    Every number marked with a superscript footnote is made up for the demo.[^illus] The burn-rate multipliers come from the Google SRE workbook. The request metrics exist today. The disk, repair and scrub metrics do not. This page describes the intended way of running TS2. No alert rules, on-call rota or review meetings exist, and neither does BucketRegistryService, so the control plane team below is planned.

## Two levels of review

One review cannot walk every API's number, so there are two.

| Review | Who | Looks at | Cadence |
|---|---|---|---|
| **Org-wide ops review** | Service leads, wider org | The service-level set below, five numbers. Aggregated per region, with drill-down to cell, plus the top movers (the cells or operations whose number changed most). | Weekly |
| **Component team review** | Data plane team, control plane team | Every operation's availability and latency SLI, plus the SLOs of the dependencies the team calls. | Own meeting, weekly |

| Team | Owns | Operations |
|---|---|---|
| Data plane | FrontdoorService data path, ChunkStoreService, ObjectIndexService | PutObject, GetObject, HeadObject, DeleteObject, ListObjects |
| Control plane | Bucket management in FrontdoorService, BucketRegistryService | CreateBucket, DeleteBucket, GetBucket, ListBuckets |

ListObjects is read from ObjectIndexService, so the data plane team owns it, even though it is Tier 1.

## The service-level set

All availability SLIs read `ts2_http_requests_total`, which carries the labels `operation` and `status_class`. Latency reads `ts2_http_request_duration_seconds` with the same `operation` label.

### 1. Durability

Covered in [its own section](#durability-slis) below.

### 2. Data availability, Tier 0

| | |
|---|---|
| Operations | `operation` in `PutObject`, `GetObject`, `HeadObject`, `DeleteObject` |
| SLI | numerator: requests whose `status_class` is not `5xx`. Denominator: all requests for those operations. A 4xx is the caller's error, handled correctly, so it counts as good, which also keeps invalid requests out of the way. Computed per 5-minute window, averaged over the period. |
| Customer SLO | 99.99% per month[^t0] |
| Internal SLO | 99.995% per 30 days[^t0i] |
| Error budget (internal) | 0.005% of requests |

### 3. Management and listing availability, Tier 1

| | |
|---|---|
| Operations | `operation` in `CreateBucket`, `DeleteBucket`, `GetBucket`, `ListBuckets`, `ListObjects` |
| SLI | same shape as Tier 0, over these operations |
| Customer SLO | 99.9% per month[^t1] |
| Internal SLO | 99.95% per 30 days[^t1i] |
| Error budget (internal) | 0.05% of requests |

Why lower: rarer, retryable, off the data path.

A 4xx counts as good, a 5xx and a timeout count as bad. `HealthCheck` and `Unknown` are not in either tier.

### 4. Latency

| | SLI | Customer SLO | Internal SLO |
|---|---|---|---|
| GetObject | p99 time to first byte, objects up to 1 MiB | under 100 ms[^lat] | under 80 ms[^lat] |
| PutObject | p99 duration, objects up to 1 MiB | under 250 ms[^lat] | under 200 ms[^lat] |

`ts2_http_request_duration_seconds` is measured at frontdoor until the response is complete, so today it overstates GetObject time to first byte for large bodies. The 1 MiB limit keeps the gap small. A true first-byte histogram is on the [backlog](../backlog.md). There is no object-size label yet, so the size cut is made with the byte counters or a probe. That is a known gap.

### 5. Consistency

Strong read-after-write and list-after-write for all operations. It has no percentage and no error budget. A canary writes an object, reads it back and lists the bucket straight away, once a minute per cell, and any stale result is a bug filed as sev3. The canary is planned.[^cons]

## Alerts

Burn rate is how fast we use the error budget. 1x uses it all in 30 days. Each alert needs a long and a short window to agree, so a spike that has already ended does not page anyone. The error rate that trips an alert is the burn rate times the internal budget.

| Severity | Burn rate | Windows | Tier 0 trips at | Tier 1 trips at |
|---|---|---|---|---|
| Page (sev2) | 14.4x | 1 h and 5 min | above 0.072% | above 0.72% |
| Ticket (sev3) | 6x | 6 h and 30 min | above 0.03% | above 0.30% |
| Ticket (sev3) | 1x | 3 days and 6 h | above 0.005% | above 0.05% |
| Ticket (sev3) | n/a | 30 days | below 99.995% | below 99.95% |

Latency alerts use the internal SLO: ticket when p99 stays above it for 30 minutes, page when it is above the customer SLO for 10 minutes.

**Why the internal SLO is stricter.** If we only reacted at the customer number, the customer would already be hurting and the month would be spent. Running tighter leaves a buffer to see trouble, fix it and still meet the promise.

## Error budget

Tier 0 at the customer 99.99% allows about 4.3 minutes of full outage per 30-day month (43,200 minutes times 0.0001). At the internal 99.995% it is about 2.2 minutes.

Spending it means real requests failed. A bad deploy that errors for two minutes spends half of the customer budget. What we do with the budget:

- More than half left: ship as normal.
- Less than half left: the next week favours reliability work over features.
- Spent: freeze risky changes to the data path until the month rolls over, and write an incident review.

Tier 1 has ten times the budget (43.2 minutes), because the calls are less critical.

## Durability SLIs

11 nines cannot be measured. Losing one object in 10,000 years across 10 million objects never shows up in a month of data. So durability is **modelled**, and we watch **proxies**.

**The model.** Annual loss probability follows from the disk annual failure rate, the number of copies or the erasure code shape, and the time to repair a lost copy. Shorter repair time and more copies drive the number down. We recompute it when any input changes.

| Proxy | Target | Exists today |
|---|---|---|
| Objects lost (a read finds a manifest whose chunks are missing) | 0, always | Partly. A failed GetObject shows as a 5xx, but nothing counts it as a loss. |
| Scrub checksum failures (a background job re-reads chunks and checks them) | 0 unrepaired | Planned. No scrub job yet. |
| Under-replicated chunks | 0 for longer than the repair target | Planned. One copy only today. |
| Repair backlog (chunks waiting to be rebuilt) | draining | Planned. |
| Time to repair, p99 | within the modelled value | Planned. |

Objects lost is a page (sev2) at any non-zero value. The others become tickets and pages once the replication work lands, see the [plan](../plan.md).

[^illus]: Illustrative, made up for the demo.
[^t0]: Illustrative, made up for the demo.
[^t0i]: Illustrative, made up for the demo. Stricter than the customer number so we see trouble first.
[^t1]: Illustrative, made up for the demo.
[^t1i]: Illustrative, made up for the demo. Stricter than the customer number so we see trouble first.
[^lat]: Illustrative, made up for the demo.
[^cons]: Illustrative. The canary does not exist yet.
