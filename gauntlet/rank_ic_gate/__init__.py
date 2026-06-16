"""
rank_ic_gate — deflated rank-IC kill-test apparatus.

Pure modules for a powered rank-IC micro-test (does signal X rank-predict forward returns of
asset Y, with honest effective-N and family-wise deflation):
  - rank_ic_calculator   : Spearman rank-IC + Fisher-z SE (1/sqrt(n-3)) + tanh CI
  - power_gate           : corrected correlation-VIF N_eff + MDE_IC (+ rho-sensitivity band)
  - deflation            : Holm family-wise correction over per-cell permutation p-values
  - signal_return_pairer : PIT-invariant pairing of a bar-open signal with a forward return
  - pit_validator        : backtest PIT (available_at < forward_window_start) assertions

All modules are pure (numpy/pandas, no I/O, deterministic).
"""
