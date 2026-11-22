# Operations

The view from the people who run TS2. The customer promise is on the [service levels](../guide/service-levels.md) page. This page covers how we stay ahead of it.

!!! warning "Illustrative"
    The thresholds are the standard ones from the Google SRE workbook, applied to a made-up service. The heat map data is invented. No real traffic produced it.

## Region, availability zone, cell

- A **region** is a geographic location, such as us-west.
- An **availability zone** (AZ) is an isolated site inside a region.
- A **cell** is a self-contained copy of the data plane (one ObjectIndexService plus its ChunkStoreService nodes) that serves a fixed set of buckets. Cells never talk to each other. A cell is the unit of **blast radius**: a bad deploy or a corrupted index hurts one cell only. See [cells in the system overview](../design/000-system-overview.md).

Today there is one location and one cell. Letting customers pick a region, and later an AZ, per bucket is planned (see the [backlog](../backlog.md)). Operators will watch availability per region and per cell.

## SLO ladder

Four numbers, from loosest to tightest:

| Level | Availability | What it is |
|---|---|---|
| Customer SLA/SLO | 99.9% per month (43.2 min of failure) | What we publish. |
| Internal SLO | 99.95% per 30 days (21.6 min) | What we run to. |
| Ticket (sev3) | 30-day availability below 99.95%, or a slow budget burn | Someone looks within a working day. |
| Page (sev2) | Fast budget burn | Someone looks now. |

**Why the internal target is stricter.** If we only reacted at 99.9% the customer would already be hurting and the month would be nearly spent. Running to 99.95% means we have the other half of the budget as a buffer: we see trouble, fix it and still meet the promise.

### Burn-rate alerts

The error budget is the allowed failure: 0.05% of requests against the 99.95% internal SLO. Burn rate is how fast we are using it. A burn rate of 1 uses the whole budget in exactly 30 days. A burn rate of 14.4 uses 2% of the budget in one hour. Each alert needs a long window and a short window to both agree, so a spike that has already ended does not page anyone.

| Severity | Burn rate | Windows | Error rate that trips it | Budget used if it continues |
|---|---|---|---|---|
| Page (sev2) | 14.4x | 1 h and 5 min | above 0.72% | 2% in 1 hour |
| Ticket (sev3) | 6x | 6 h and 30 min | above 0.30% | 5% in 6 hours |
| Ticket (sev3) | 1x | 3 days and 6 h | above 0.05% | 10% in 3 days |
| Ticket (sev3) | n/a | 30 days | availability below 99.95% | budget already spent |

Error rate is the share of 5xx responses among valid requests, from `ts2_http_requests_total` split by status class.

### Latency and disk

Latency comes from `ts2_http_request_duration_seconds`, per `operation` label (`GetObject`, `PutObject`, `HeadObject`, `ListObjects`, and so on). Tighter than the customer target, for the same reason as above.

| Signal | Ticket (sev3) | Page (sev2) |
|---|---|---|
| p99 `GetObject`, `HeadObject` for 1 MiB or smaller | above 80 ms for 30 min | above 100 ms for 10 min |
| p99 `PutObject` for 1 MiB or smaller | above 200 ms for 30 min | above 250 ms for 10 min |
| Disk usage on a ChunkStoreService node | above 80% | above 90% (writes stop at 95%) |
| Disk write latency (fsync p99) on a node | above 50 ms for 30 min | above 200 ms for 10 min |

Disk and fsync metrics do not exist yet. They arrive with the data-plane dashboards in v0.2, so those two rows are the intended thresholds, not live alerts. The request metrics above are live today.

## Availability heat map

Made-up data, last 7 days. Each cell is the availability for one day. Colours follow the thresholds above.

<p class="heatmap-legend"><span class="g">99.95% or higher</span><span class="a">99.9% to 99.95%</span><span class="r">below 99.9%</span></p>

**By region** (traffic-weighted across the region's cells):

<table class="heatmap">
<thead><tr><th>Region</th><th>Nov 16</th><th>Nov 17</th><th>Nov 18</th><th>Nov 19</th><th>Nov 20</th><th>Nov 21</th><th>Nov 22</th></tr></thead>
<tbody>
<tr><td>us-west</td><td class="g">99.99</td><td class="g">99.98</td><td class="g">99.99</td><td class="g">99.97</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.98</td></tr>
<tr><td>us-east</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.98</td><td class="g">99.99</td><td class="a">99.91</td><td class="g">99.99</td><td class="g">99.99</td></tr>
<tr><td>eu-central</td><td class="g">99.99</td><td class="g">99.96</td><td class="a">99.93</td><td class="g">99.99</td><td class="g">99.98</td><td class="g">99.99</td><td class="g">99.99</td></tr>
</tbody>
</table>

us-east on Nov 20 is amber, which is a region-level blip. Drilling in shows where it came from.

**us-east by cell** (same days, one row per cell):

<table class="heatmap">
<thead><tr><th>Cell</th><th>Nov 16</th><th>Nov 17</th><th>Nov 18</th><th>Nov 19</th><th>Nov 20</th><th>Nov 21</th><th>Nov 22</th></tr></thead>
<tbody>
<tr><td>us-east-cell-a</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td></tr>
<tr><td>us-east-cell-b</td><td class="g">99.99</td><td class="g">99.99</td><td class="a">99.93</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td></tr>
<tr><td>us-east-cell-c</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="r">99.67</td><td class="g">99.99</td><td class="g">99.99</td></tr>
<tr><td>us-east-cell-d</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.99</td></tr>
</tbody>
</table>

One cell, `us-east-cell-c`, had a bad day on Nov 20. Because cells are separate, the other three were fine, and the region number only dipped to amber. That is the point of cells: the blast radius of that day was one quarter of the region's buckets.

## Dashboards

- **Frontdoor dashboard (live).** Request rate, errors, latency by `operation`, bytes in and out. Start it with `make up-obs` and open [Grafana at localhost:3000](http://localhost:3000), dashboard `frontdoor`. It only works while the stack is running.
- **Ops Review dashboard (planned, next).** Availability by region and by cell with the SLO lines drawn on, the same view as the heat map but from real data. It is on the [backlog](../backlog.md).

## Weekly ops review

1. Open the heat map. Anything amber or red in the last 7 days gets a line in the notes.
2. Drill from region to cell. Find the cell and the day, then open the frontdoor dashboard for that window and look at the `operation` with the errors.
3. Check the 30-day availability and how much error budget is left. If less than half is left, the next week favours reliability work over features.
4. Review every page and ticket from the week: was the threshold right, did we act before customers noticed?
5. Write follow-ups into the backlog, with an owner.

Further reading: [alerting on SLOs](https://sre.google/workbook/alerting-on-slos/) in the Google SRE workbook is the source of the burn-rate numbers.
