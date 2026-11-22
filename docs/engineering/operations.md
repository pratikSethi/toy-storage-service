# Operations

The view from the people who run TS2. The customer promise is on the [service levels](../guide/service-levels.md) page, and the internal definitions are in [service levels for engineers](service-levels.md). This page covers how we stay ahead of it.

!!! warning "Illustrative"
    The heat map data is invented. No real traffic produced it.

## Region, availability zone, cell

- A **region** is a geographic location, such as us-west.
- An **availability zone** (AZ) is an isolated site inside a region.
- A **cell** is a self-contained copy of the data plane (one ObjectIndexService plus its ChunkStoreService nodes) that serves a fixed set of buckets. Cells never talk to each other. A cell is the unit of **blast radius**: a bad deploy or a corrupted index hurts one cell only. See [cells in the system overview](../design/000-system-overview.md).

Today there is one location and one cell. Letting customers pick a region, and later an AZ, per bucket is planned (see the [backlog](../backlog.md)). Operators will watch availability per region and per cell.

## Service levels

The service has five published service levels: durability, Tier 0 data availability, Tier 1 management and listing availability, latency and consistency. Each has an internal SLO that is stricter than the customer number, and burn-rate alerts that page or open a ticket. The definitions, thresholds and error budget are on [service levels for engineers](service-levels.md). This page covers the regions and cells we watch, and the weekly review.

## Availability heat map

Made-up data, last 7 days.[^ill] Each cell is the availability for one day, split by tier. Colours follow the internal SLO of each tier, because that is where we start to care.

<p class="heatmap-legend"><span class="g">at or above the internal SLO</span><span class="a">between the customer SLO and the internal SLO</span><span class="r">below the customer SLO</span></p>

| | Green | Amber | Red |
|---|---|---|---|
| Tier 0 | 99.995% or higher | 99.99% to 99.995% | below 99.99% |
| Tier 1 | 99.95% or higher | 99.9% to 99.95% | below 99.9% |

**By region, Tier 0** (traffic-weighted across the region's cells):

<table class="heatmap">
<thead><tr><th>Region</th><th>Nov 16</th><th>Nov 17</th><th>Nov 18</th><th>Nov 19</th><th>Nov 20</th><th>Nov 21</th><th>Nov 22</th></tr></thead>
<tbody>
<tr><td>us-west</td><td class="g">99.999</td><td class="g">99.998</td><td class="g">99.999</td><td class="g">99.997</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.998</td></tr>
<tr><td>us-east</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.998</td><td class="g">99.999</td><td class="r">99.972</td><td class="g">99.999</td><td class="g">99.999</td></tr>
<tr><td>eu-central</td><td class="g">99.999</td><td class="g">99.997</td><td class="a">99.993</td><td class="g">99.999</td><td class="g">99.998</td><td class="g">99.999</td><td class="g">99.999</td></tr>
</tbody>
</table>

**By region, Tier 1:**

<table class="heatmap">
<thead><tr><th>Region</th><th>Nov 16</th><th>Nov 17</th><th>Nov 18</th><th>Nov 19</th><th>Nov 20</th><th>Nov 21</th><th>Nov 22</th></tr></thead>
<tbody>
<tr><td>us-west</td><td class="g">99.99</td><td class="g">99.98</td><td class="g">99.99</td><td class="g">99.97</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.98</td></tr>
<tr><td>us-east</td><td class="g">99.99</td><td class="g">99.99</td><td class="g">99.98</td><td class="g">99.99</td><td class="a">99.91</td><td class="g">99.99</td><td class="g">99.99</td></tr>
<tr><td>eu-central</td><td class="g">99.99</td><td class="g">99.96</td><td class="a">99.93</td><td class="g">99.99</td><td class="g">99.98</td><td class="g">99.99</td><td class="g">99.99</td></tr>
</tbody>
</table>

us-east on Nov 20 is red for Tier 0 and amber for Tier 1. Tier 0 has less room, so the same incident shows up harder there. Drilling in shows where it came from.

**us-east by cell, Tier 0** (same days, one row per cell):

<table class="heatmap">
<thead><tr><th>Cell</th><th>Nov 16</th><th>Nov 17</th><th>Nov 18</th><th>Nov 19</th><th>Nov 20</th><th>Nov 21</th><th>Nov 22</th></tr></thead>
<tbody>
<tr><td>us-east-cell-a</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td></tr>
<tr><td>us-east-cell-b</td><td class="g">99.999</td><td class="g">99.999</td><td class="a">99.993</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td></tr>
<tr><td>us-east-cell-c</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="r">99.890</td><td class="g">99.999</td><td class="g">99.999</td></tr>
<tr><td>us-east-cell-d</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td><td class="g">99.999</td></tr>
</tbody>
</table>

One cell, `us-east-cell-c`, had a bad day on Nov 20. Because cells are separate, the other three were fine, and the region number dipped less than the cell's. That is the point of cells: the blast radius of that day was one quarter of the region's buckets.

[^ill]: Illustrative, made up for the demo. No real traffic produced these numbers.

## Dashboards

- **Frontdoor dashboard (live).** Request rate, errors, latency by `operation`, bytes in and out. Start it with `make up-obs` and open [Grafana at localhost:3000](http://localhost:3000), dashboard `frontdoor`. It only works while the stack is running.
- **Ops Review dashboard (live, partly synthetic).** `TS2 Ops Review` in Grafana. The top sections come from real metrics: Tier 0 and Tier 1 availability, error budget remaining, 1h and 5m burn rate with the 14.4x page line, PutObject and GetObject p99 against their targets, and the one durability proxy that exists today. The bottom section is region x day availability and a region and cell drill-down. That section is **synthetic** (Grafana TestData) until TS2 runs several regions and cells. To open it: run `make up-obs`, go to [localhost:3000](http://localhost:3000), sign in as `admin` / `admin`, and pick `TS2 Ops Review`. The definitions behind it are in [service levels](service-levels.md). The error budget panel uses the time range as its window, so set 30d for the monthly view. The dashboard file is `deploy/compose/grafana/dashboards/ops-review.json`.

## Weekly ops review

1. Open the heat map, Tier 0 first. Anything amber or red in the last 7 days gets a line in the notes.
2. Drill from region to cell. Find the cell and the day, then open the frontdoor dashboard for that window and look at the `operation` with the errors.
3. Check the 30-day availability of each tier and how much error budget is left. If less than half is left, the next week favours reliability work over features.
4. Review every page and ticket from the week: was the threshold right, did we act before customers noticed?
5. Write follow-ups into the backlog, with an owner.

Further reading: [alerting on SLOs](https://sre.google/workbook/alerting-on-slos/) in the Google SRE workbook is the source of the burn-rate numbers.
