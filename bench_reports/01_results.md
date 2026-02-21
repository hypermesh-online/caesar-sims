# Benchmark Report 01 - Initial Scenario Suite

**Date**: 2026-02-21T04:54:41.906Z
**Engine Version**: 0.6.0
**Node Count**: 24 (6x4 grid)
**Ticks per Scenario**: 200
**Branch**: `main` @ commit `8e1c631`

---

## Scenario Results

| Scenario | Settled | Reverted | Avg Fee | Conservation Error | Peak Fee | Total Input | Total Output | Result |
|----------|---------|----------|---------|-------------------|----------|-------------|-------------|--------|
| Pax Romana | 0 | 6 | 65.71% | ~0 (9.09e-12) | 67.91% | $14,700 | $0.21 | **FAIL** |
| Firehose | 24 | 140 | 0.40% | ~0 (2.10e-9) | 57.01% | $774,600 | $528.78 | **PASS** |
| Bank Run | 0 | 8 | 34.31% | ~0 (9.09e-12) | 41.65% | $15,800 | $0.38 | **FAIL** |
| Flash Crash | 23 | 573 | 0.40% | ~0 (6.52e-9) | 0.50% | $1,160,000 | $553.46 | **PASS** |
| Drought | 0 | 0 | ~0% | 0 | ~0% | $0 | $0 | **FAIL** |

**Overall: 2/5 PASS**

---

## Scenario Parameters

| Scenario | Gold Price | Demand Factor | Panic Level | Intent |
|----------|-----------|---------------|-------------|--------|
| Pax Romana | $2,600 | 0.2 | 0.0 | Stable baseline, golden era |
| Firehose | $2,600 | 0.9 | 0.1 | High throughput, stable price |
| Bank Run | $2,000 | 0.5 | 0.9 | Price crash + mass panic exit |
| Flash Crash | $2,000 | 0.8 | 0.3 | Price crash + high demand |
| Drought | $2,600 | 0.05 | 0.0 | Near-zero demand |

---

## Pass/Fail Criteria

- Conservation error < 1.0 (all scenarios)
- Settlement count > 0 (all scenarios)
- Bank Run: avg fee > 5% (defensive pricing)
- Drought: avg fee > 1% (minimum floor)

---

## Network State at Export

All 24 nodes showed:
- **Trust scores**: 0.5 (baseline, no movement from benchmarks since fresh engine per scenario)
- **Fees earned**: 0.0 across all nodes (exported state is from last live sim, not benchmark)
- **Pressure**: 0.0 across all nodes
- **Strategies**: Cyclic assignment (RiskAverse, Greedy, Passive)
- **Role distribution**: 6 Ingress, 6 Egress, 6 Transit, 6 NGauge (i % 4)

---

## Raw Export Data

```json
{
  "benchResults": [
    {
      "scenario": "Pax Romana",
      "settlementCount": 0,
      "revertCount": 6,
      "avgFee": 65.70601413653976,
      "conservationError": 9.094947017729282e-12,
      "totalInput": 14700,
      "totalOutput": 0.21222928289646736,
      "pass": false,
      "ticks": 200,
      "peakFee": 67.90621088727208
    },
    {
      "scenario": "Firehose",
      "settlementCount": 24,
      "revertCount": 140,
      "avgFee": 0.4,
      "conservationError": 2.0954757928848267e-9,
      "totalInput": 774600,
      "totalOutput": 528.7801071578169,
      "pass": true,
      "ticks": 200,
      "peakFee": 57.012230543235596
    },
    {
      "scenario": "Bank Run",
      "settlementCount": 0,
      "revertCount": 8,
      "avgFee": 34.31089998890655,
      "conservationError": 9.094947017729282e-12,
      "totalInput": 15800,
      "totalOutput": 0.38265832379783477,
      "pass": false,
      "ticks": 200,
      "peakFee": 41.64667187413207
    },
    {
      "scenario": "Flash Crash",
      "settlementCount": 23,
      "revertCount": 573,
      "avgFee": 0.4,
      "conservationError": 6.51925802230835e-9,
      "totalInput": 1160000,
      "totalOutput": 553.4583911169855,
      "pass": true,
      "ticks": 200,
      "peakFee": 0.5
    },
    {
      "scenario": "Drought",
      "settlementCount": 0,
      "revertCount": 0,
      "avgFee": 2.7777777777777776e-9,
      "conservationError": 0,
      "totalInput": 0,
      "totalOutput": 0,
      "pass": false,
      "ticks": 200,
      "peakFee": 2.7777777777777776e-9
    }
  ],
  "timestamp": "2026-02-21T04:54:41.906Z",
  "version": "0.6.0"
}
```
