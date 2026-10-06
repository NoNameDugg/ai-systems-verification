#!/usr/bin/env python
"""
make_demo_data.py -- regenerate the two synthetic demo CSVs in data/ (deterministic, seeded).

    clean_synthetic.csv  a planted REAL edge: a persistent AR(1) signal (phi 0.9) known 17 hours after
                         its bar; the return of the bar after the entry bar carries beta * signal
                         (beta 0.003) on top of 1%/bar noise. Cost 5 bps one-way.
    leaky_synthetic.csv  the classic stamp error: the signal is the bar's own standardised return,
                         computed from that bar's close, and stamped available_at = the bar's timestamp.
                         There is no planted edge. Cost 5 bps one-way.

Both are 4,000 daily bars from 2010-01-01. Nothing in them is market data.
Run:  python make_demo_data.py   (writes data/clean_synthetic.csv and data/leaky_synthetic.csv)
"""
from __future__ import annotations

import os

import numpy as np
import pandas as pd

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "data")
N_BARS = 4000
START = "2010-01-01"
COST_BPS = 5.0


def _bars(n: int) -> pd.DatetimeIndex:
    return pd.date_range(START, periods=n, freq="D")


def build_clean(seed: int = 20260922, phi: float = 0.9, beta: float = 0.003, sigma: float = 0.01) -> pd.DataFrame:
    rng = np.random.default_rng(seed)
    n = N_BARS
    s = np.empty(n)
    s[0] = rng.normal()
    eta = rng.normal(0.0, np.sqrt(1.0 - phi ** 2), size=n)
    for t in range(1, n):
        s[t] = phi * s[t - 1] + eta[t]
    eps = rng.normal(0.0, sigma, size=n)
    r = np.zeros(n)
    r[2:] = beta * s[:-2] + eps[2:]          # return over (t+1 -> t+2) carries the signal known at t
    price = 100.0 * np.exp(np.cumsum(r))
    ts = _bars(n)
    return pd.DataFrame({
        "timestamp": ts,
        "signal": s,
        "available_at": ts + pd.Timedelta(hours=17),
        "price": price,
        "cost_bps": COST_BPS,
    })


def build_leaky(seed: int = 20260923, sigma: float = 0.01) -> pd.DataFrame:
    rng = np.random.default_rng(seed)
    n = N_BARS
    r = rng.normal(0.0, sigma, size=n)
    price = 100.0 * np.exp(np.cumsum(r))
    own_return = np.r_[np.nan, np.diff(np.log(price))]
    ts = _bars(n)
    return pd.DataFrame({
        "timestamp": ts,
        "signal": own_return / sigma,         # computed from this bar's close ...
        "available_at": ts,                   # ... but stamped as known AT the bar: the leak
        "price": price,
        "cost_bps": COST_BPS,
    })


def write_all() -> None:
    os.makedirs(DATA, exist_ok=True)
    for name, df in (("clean_synthetic.csv", build_clean()), ("leaky_synthetic.csv", build_leaky())):
        path = os.path.join(DATA, name)
        df.to_csv(path, index=False, float_format="%.6f", date_format="%Y-%m-%dT%H:%M:%S", lineterminator="\n")
        print(f"wrote {path} ({len(df)} rows)")


if __name__ == "__main__":
    write_all()
