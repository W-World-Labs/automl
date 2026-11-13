"""
Auto.ml GENESIS-001 — Economics & Finance Analysis Experiment
==============================================================
Demonstrates quantitative finance and econometric methods.
Template for Auto Society (economics, finance, political economy).

This experiment:
  1. Generates synthetic multi-asset return data (3 assets × 252 days)
  2. Computes risk metrics (Sharpe, Sortino, VaR, CVaR, MaxDrawdown)
  3. Runs Markowitz mean-variance portfolio optimization
  4. Tests Efficient Market Hypothesis via autocorrelation (Ljung-Box)
  5. Performs Augmented Dickey-Fuller stationarity test
  6. Generates publication-quality financial figures

No external finance libraries — pure numpy/scipy/pandas.

Usage:
  uv run python experiments/exp_financial_analysis.py
"""

import json
from pathlib import Path

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from scipy import stats, optimize

# ── Configuration ──────────────────────────────────────────────
SEED = 42
OUTPUT_DIR = Path("outputs/finance")
FIGURE_DIR = OUTPUT_DIR / "figures"
TRADING_DAYS = 252  # 1 year
RISK_FREE_RATE = 0.04  # 4% annual
np.random.seed(SEED)


def setup():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    FIGURE_DIR.mkdir(exist_ok=True)


# ═══════════════════════════════════════════════════════════════
# 1. Data Generation — Synthetic Multi-Asset Returns
# ═══════════════════════════════════════════════════════════════

def generate_market_data() -> pd.DataFrame:
    """Generate correlated multi-asset daily returns via Cholesky decomposition."""
    print("\n▶ Generating Synthetic Market Data")

    # Mean annual returns and volatilities
    assets = {
        "Equity_Index": {"mu": 0.10, "sigma": 0.18},
        "Gov_Bond_10Y": {"mu": 0.04, "sigma": 0.06},
        "Commodity_ETF": {"mu": 0.07, "sigma": 0.22},
    }

    # Correlation matrix
    corr = np.array([
        [1.00, -0.20, 0.35],
        [-0.20, 1.00, 0.05],
        [0.35, 0.05, 1.00],
    ])

    names = list(assets.keys())
    daily_mu = np.array([assets[n]["mu"] / TRADING_DAYS for n in names])
    daily_sigma = np.array([assets[n]["sigma"] / np.sqrt(TRADING_DAYS) for n in names])

    # Cholesky decomposition for correlated returns
    cov = np.outer(daily_sigma, daily_sigma) * corr
    L = np.linalg.cholesky(cov)
    z = np.random.standard_normal((TRADING_DAYS, len(names)))
    returns = z @ L.T + daily_mu

    dates = pd.date_range("2025-01-02", periods=TRADING_DAYS, freq="B")
    df = pd.DataFrame(returns, columns=names, index=dates)
    df.index.name = "date"

    # Compute cumulative prices (start at 100)
    prices = (1 + df).cumprod() * 100
    prices.to_csv(OUTPUT_DIR / "price_series.csv")
    df.to_csv(OUTPUT_DIR / "daily_returns.csv")

    print(f"  📊 Generated {TRADING_DAYS} days × {len(names)} assets")
    for name in names:
        annual_ret = float((1 + df[name]).prod() ** (252 / len(df)) - 1)
        annual_vol = float(df[name].std() * np.sqrt(252))
        print(f"  {name:20s} return={annual_ret:>7.2%}  vol={annual_vol:>6.2%}")

    return df


# ═══════════════════════════════════════════════════════════════
# 2. Risk Metrics
# ═══════════════════════════════════════════════════════════════

def compute_risk_metrics(df: pd.DataFrame) -> dict:
    """Compute standard financial risk metrics for each asset."""
    print("\n▶ Computing Risk Metrics")

    metrics = {}
    for col in df.columns:
        r = df[col].values
        annual_ret = float((1 + df[col]).prod() ** (252 / len(df)) - 1)
        annual_vol = float(df[col].std() * np.sqrt(252))

        # Sharpe Ratio
        sharpe = (annual_ret - RISK_FREE_RATE) / annual_vol if annual_vol > 0 else 0

        # Sortino Ratio (downside deviation)
        downside = r[r < 0]
        downside_std = float(np.std(downside) * np.sqrt(252)) if len(downside) > 0 else 0.001
        sortino = (annual_ret - RISK_FREE_RATE) / downside_std

        # Value at Risk (95%)
        var_95 = float(np.percentile(r, 5))

        # Conditional VaR (Expected Shortfall)
        cvar_95 = float(r[r <= var_95].mean()) if len(r[r <= var_95]) > 0 else var_95

        # Maximum Drawdown
        cum = (1 + df[col]).cumprod()
        peak = cum.cummax()
        drawdown = (cum - peak) / peak
        max_dd = float(drawdown.min())

        metrics[col] = {
            "annual_return": round(annual_ret, 4),
            "annual_volatility": round(annual_vol, 4),
            "sharpe_ratio": round(sharpe, 4),
            "sortino_ratio": round(sortino, 4),
            "var_95": round(var_95, 6),
            "cvar_95": round(cvar_95, 6),
            "max_drawdown": round(max_dd, 4),
        }
        print(f"  {col:20s} Sharpe={sharpe:>6.3f}  VaR95={var_95:>8.4%}  MaxDD={max_dd:>7.2%}")

    return metrics


# ═══════════════════════════════════════════════════════════════
# 3. Markowitz Portfolio Optimization
# ═══════════════════════════════════════════════════════════════

def markowitz_optimization(df: pd.DataFrame) -> dict:
    """Mean-variance optimization: find minimum variance and maximum Sharpe portfolios."""
    print("\n▶ Markowitz Mean-Variance Optimization")

    mu = df.mean().values * 252  # annualized
    cov = df.cov().values * 252
    n = len(mu)
    names = df.columns.tolist()

    def portfolio_stats(w):
        ret = w @ mu
        vol = np.sqrt(w @ cov @ w)
        sharpe = (ret - RISK_FREE_RATE) / vol if vol > 0 else 0
        return ret, vol, sharpe

    # Constraints: weights sum to 1, all >= 0
    constraints = {"type": "eq", "fun": lambda w: np.sum(w) - 1}
    bounds = [(0, 1)] * n

    # Maximum Sharpe portfolio
    def neg_sharpe(w):
        r, v, s = portfolio_stats(w)
        return -s

    w0 = np.ones(n) / n
    res_sharpe = optimize.minimize(neg_sharpe, w0, method="SLSQP", bounds=bounds, constraints=constraints)
    max_sharpe_w = res_sharpe.x
    ms_ret, ms_vol, ms_sharpe = portfolio_stats(max_sharpe_w)

    # Minimum variance portfolio
    def variance(w):
        return w @ cov @ w

    res_minvar = optimize.minimize(variance, w0, method="SLSQP", bounds=bounds, constraints=constraints)
    minvar_w = res_minvar.x
    mv_ret, mv_vol, mv_sharpe = portfolio_stats(minvar_w)

    print(f"  Max Sharpe:  {dict(zip(names, [round(float(x), 3) for x in max_sharpe_w]))}")
    print(f"               return={ms_ret:.2%}, vol={ms_vol:.2%}, Sharpe={ms_sharpe:.3f}")
    print(f"  Min Variance: {dict(zip(names, [round(float(x), 3) for x in minvar_w]))}")
    print(f"               return={mv_ret:.2%}, vol={mv_vol:.2%}, Sharpe={mv_sharpe:.3f}")

    # Efficient frontier (Monte Carlo)
    frontier = []
    for _ in range(5000):
        w = np.random.dirichlet(np.ones(n))
        r, v, s = portfolio_stats(w)
        frontier.append({"return": round(float(r), 4), "volatility": round(float(v), 4), "sharpe": round(float(s), 4)})

    return {
        "max_sharpe_portfolio": {
            "weights": {n: round(float(w), 4) for n, w in zip(names, max_sharpe_w)},
            "return": round(float(ms_ret), 4),
            "volatility": round(float(ms_vol), 4),
            "sharpe": round(float(ms_sharpe), 4),
        },
        "min_variance_portfolio": {
            "weights": {n: round(float(w), 4) for n, w in zip(names, minvar_w)},
            "return": round(float(mv_ret), 4),
            "volatility": round(float(mv_vol), 4),
            "sharpe": round(float(mv_sharpe), 4),
        },
        "frontier_samples": len(frontier),
        "_frontier": frontier,  # for plotting
    }


# ═══════════════════════════════════════════════════════════════
# 4. Econometric Tests
# ═══════════════════════════════════════════════════════════════

def econometric_tests(df: pd.DataFrame) -> dict:
    """Test EMH (autocorrelation) and stationarity (ADF)."""
    print("\n▶ Econometric Tests")

    results = {}
    for col in df.columns:
        r = df[col].values

        # Ljung-Box test for autocorrelation (proxy: use lag-1 autocorrelation + chi2)
        n = len(r)
        acf_1 = float(np.corrcoef(r[:-1], r[1:])[0, 1])
        lb_stat = n * (n + 2) * (acf_1**2 / (n - 1))  # simplified Ljung-Box lag-1
        lb_p = float(1 - stats.chi2.cdf(lb_stat, df=1))

        # Augmented Dickey-Fuller (simplified: t-test on AR(1) coefficient)
        y = r[1:]
        x = r[:-1]
        slope, intercept, r_val, p_val, std_err = stats.linregress(x, y)
        adf_stat = (slope - 1) / std_err  # test H0: slope = 1 (unit root)
        # Critical values (approximate): -3.43 (1%), -2.86 (5%), -2.57 (10%)
        is_stationary = adf_stat < -2.86

        results[col] = {
            "acf_lag1": round(acf_1, 4),
            "ljung_box_stat": round(float(lb_stat), 4),
            "ljung_box_p": round(lb_p, 4),
            "emh_consistent": bool(lb_p > 0.05),  # no significant autocorrelation → EMH holds
            "adf_statistic": round(float(adf_stat), 4),
            "is_stationary": bool(is_stationary),
        }
        emh = "✅ EMH consistent" if lb_p > 0.05 else "❌ Autocorrelation detected"
        stat = "✅ Stationary" if is_stationary else "⚠️ Non-stationary"
        print(f"  {col:20s} ACF1={acf_1:>7.4f}  LB_p={lb_p:>6.4f} ({emh})")
        print(f"  {'':20s} ADF={adf_stat:>7.4f} ({stat})")

    return results


# ═══════════════════════════════════════════════════════════════
# 5. Visualization
# ═══════════════════════════════════════════════════════════════

def plot_finance(df: pd.DataFrame, risk: dict, portfolio: dict):
    """Generate publication-quality financial figures."""
    print("\n▶ Generating Figures")

    colors = {"Equity_Index": "#3498db", "Gov_Bond_10Y": "#2ecc71", "Commodity_ETF": "#e74c3c"}

    # Figure 1: Cumulative Returns
    fig, ax = plt.subplots(figsize=(12, 6))
    cum = (1 + df).cumprod()
    for col in df.columns:
        ax.plot(cum.index, cum[col], label=col, color=colors[col], linewidth=1.5)
    ax.set_xlabel("Date", fontsize=11)
    ax.set_ylabel("Cumulative Return (base = 1)", fontsize=11)
    ax.set_title("Cumulative Returns — Multi-Asset Portfolio", fontsize=14, fontweight="bold")
    ax.legend(fontsize=10)
    ax.grid(alpha=0.3)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig1_cumulative_returns.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 1 → cumulative returns")

    # Figure 2: Risk-Return Scatter + Efficient Frontier
    fig, ax = plt.subplots(figsize=(10, 7))
    frontier = portfolio["_frontier"]
    vols = [p["volatility"] for p in frontier]
    rets = [p["return"] for p in frontier]
    sharpes = [p["sharpe"] for p in frontier]
    sc = ax.scatter(vols, rets, c=sharpes, cmap="viridis", s=5, alpha=0.5, label="Random Portfolios")
    fig.colorbar(sc, ax=ax, label="Sharpe Ratio")

    # Individual assets
    for col in df.columns:
        m = risk[col]
        ax.scatter(m["annual_volatility"], m["annual_return"], s=120, marker="D",
                   color=colors[col], edgecolors="black", linewidth=1, zorder=5, label=col)

    # Optimal portfolios
    ms = portfolio["max_sharpe_portfolio"]
    mv = portfolio["min_variance_portfolio"]
    ax.scatter(ms["volatility"], ms["return"], s=200, marker="*", color="gold",
               edgecolors="black", linewidth=1, zorder=6, label=f"Max Sharpe ({ms['sharpe']:.2f})")
    ax.scatter(mv["volatility"], mv["return"], s=200, marker="*", color="silver",
               edgecolors="black", linewidth=1, zorder=6, label=f"Min Variance")

    ax.set_xlabel("Annualized Volatility", fontsize=12)
    ax.set_ylabel("Annualized Return", fontsize=12)
    ax.set_title("Efficient Frontier — Markowitz Mean-Variance", fontsize=14, fontweight="bold")
    ax.legend(fontsize=9, loc="upper left")
    ax.grid(alpha=0.3)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig2_efficient_frontier.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 2 → efficient frontier")

    # Figure 3: Return Distribution + VaR
    fig, axes = plt.subplots(1, 3, figsize=(15, 5))
    fig.suptitle("Return Distribution with VaR/CVaR", fontsize=14, fontweight="bold")
    for ax, col in zip(axes, df.columns):
        r = df[col]
        ax.hist(r, bins=40, alpha=0.7, color=colors[col], edgecolor="black", linewidth=0.3, density=True)
        var = risk[col]["var_95"]
        cvar = risk[col]["cvar_95"]
        ax.axvline(var, color="red", linestyle="--", linewidth=2, label=f"VaR95={var:.3%}")
        ax.axvline(cvar, color="darkred", linestyle=":", linewidth=2, label=f"CVaR={cvar:.3%}")
        ax.set_title(col, fontsize=11, fontweight="bold")
        ax.legend(fontsize=8)
        ax.set_xlabel("Daily Return")
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig3_var_distributions.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 3 → VaR distributions")


def main():
    print("═" * 60)
    print("  GENESIS — Economics & Finance Analysis")
    print("═" * 60)

    setup()
    df = generate_market_data()
    risk = compute_risk_metrics(df)
    portfolio = markowitz_optimization(df)
    econ = econometric_tests(df)
    plot_finance(df, risk, portfolio)

    # Save results (strip frontier for compactness)
    portfolio_clean = {k: v for k, v in portfolio.items() if k != "_frontier"}
    results = {
        "experiment": "financial_analysis",
        "seed": SEED,
        "trading_days": TRADING_DAYS,
        "risk_free_rate": RISK_FREE_RATE,
        "risk_metrics": risk,
        "portfolio_optimization": portfolio_clean,
        "econometric_tests": econ,
        "metrics": {
            "max_sharpe_ratio": portfolio["max_sharpe_portfolio"]["sharpe"],
            "equity_annual_return": risk["Equity_Index"]["annual_return"],
            "equity_max_drawdown": risk["Equity_Index"]["max_drawdown"],
        },
    }

    with open(OUTPUT_DIR / "results.json", "w") as f:
        json.dump(results, f, indent=2)
    print(f"\n  💾 Results → {OUTPUT_DIR / 'results.json'}")

    # Verify
    print("\n▶ Self-Verification")
    expected = [
        OUTPUT_DIR / "results.json",
        OUTPUT_DIR / "price_series.csv",
        OUTPUT_DIR / "daily_returns.csv",
        FIGURE_DIR / "fig1_cumulative_returns.png",
        FIGURE_DIR / "fig2_efficient_frontier.png",
        FIGURE_DIR / "fig3_var_distributions.png",
    ]
    ok = True
    for p in expected:
        if p.exists():
            print(f"  ✅ {p}")
        else:
            print(f"  ❌ {p} MISSING")
            ok = False

    # Sanity check: Sharpe should be positive for equity
    if portfolio["max_sharpe_portfolio"]["sharpe"] > 0:
        print(f"  ✅ Max Sharpe = {portfolio['max_sharpe_portfolio']['sharpe']:.3f} (positive)")
    else:
        print(f"  ❌ Max Sharpe negative (unexpected with positive returns)")
        ok = False

    print(f"\n{'✅ ALL CHECKS PASSED' if ok else '❌ SOME CHECKS FAILED'}")
    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
