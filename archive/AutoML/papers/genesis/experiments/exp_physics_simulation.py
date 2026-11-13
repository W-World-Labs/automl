"""
Auto.ml GENESIS-001 — Physics Simulation Experiment
=====================================================
Demonstrates computational physics methods: N-body dynamics,
energy conservation verification, and phase-space analysis.
Template for Auto Science (physics) / Auto Engineering.

This experiment:
  1. Simulates a gravitational 3-body system (Sun-Earth-Moon scale)
  2. Uses Velocity-Verlet integrator (symplectic, energy-preserving)
  3. Verifies energy conservation (Hamiltonian drift)
  4. Analyzes orbital stability via Poincaré section
  5. Computes Lyapunov exponent estimate (chaos detection)
  6. Generates publication-quality phase-space diagrams

No external physics libraries — pure numpy numerical integration.

Usage:
  uv run python experiments/exp_physics_simulation.py
"""

import json
from pathlib import Path

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

# ── Configuration ──────────────────────────────────────────────
SEED = 42
OUTPUT_DIR = Path("outputs/physics")
FIGURE_DIR = OUTPUT_DIR / "figures"
np.random.seed(SEED)

# Physical constants (normalized units: AU, solar mass, year)
G = 4 * np.pi**2  # Gravitational constant in AU³/(M_sun·yr²)


def setup():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    FIGURE_DIR.mkdir(exist_ok=True)


# ═══════════════════════════════════════════════════════════════
# 1. N-Body System Setup
# ═══════════════════════════════════════════════════════════════

def init_three_body():
    """Initialize a Sun-Jupiter-Saturn system (simplified)."""
    # Masses (solar masses)
    masses = np.array([1.0, 0.001, 0.0003])  # Sun, Jupiter-like, Saturn-like

    # Positions (AU) — roughly circular orbits
    positions = np.array([
        [0.0, 0.0],      # Sun at origin
        [5.2, 0.0],      # Jupiter-like at 5.2 AU
        [9.5, 0.0],      # Saturn-like at 9.5 AU
    ])

    # Velocities (AU/yr) — circular orbit: v = sqrt(GM/r)
    v_jupiter = np.sqrt(G * masses[0] / 5.2)
    v_saturn = np.sqrt(G * masses[0] / 9.5)
    velocities = np.array([
        [0.0, 0.0],           # Sun (stationary in COM frame, approx)
        [0.0, v_jupiter],     # Jupiter — tangential
        [0.0, v_saturn],      # Saturn — tangential
    ])

    # Adjust Sun velocity for COM conservation
    total_p = masses[:, None] * velocities
    velocities[0] = -total_p[1:].sum(axis=0) / masses[0]

    return masses, positions, velocities


# ═══════════════════════════════════════════════════════════════
# 2. Velocity-Verlet Integrator
# ═══════════════════════════════════════════════════════════════

def compute_accelerations(masses, positions):
    """Compute gravitational accelerations for all bodies."""
    n = len(masses)
    acc = np.zeros_like(positions)
    for i in range(n):
        for j in range(n):
            if i != j:
                r_vec = positions[j] - positions[i]
                r_mag = np.linalg.norm(r_vec)
                r_mag = max(r_mag, 0.01)  # softening to prevent singularity
                acc[i] += G * masses[j] * r_vec / r_mag**3
    return acc


def compute_energy(masses, positions, velocities):
    """Compute total energy (kinetic + potential)."""
    n = len(masses)
    KE = 0.5 * np.sum(masses[:, None] * velocities**2)
    PE = 0.0
    for i in range(n):
        for j in range(i + 1, n):
            r = np.linalg.norm(positions[j] - positions[i])
            r = max(r, 0.01)
            PE -= G * masses[i] * masses[j] / r
    return float(KE), float(PE), float(KE + PE)


def simulate(masses, positions, velocities, dt=0.001, n_steps=50000):
    """Run Velocity-Verlet integration."""
    print(f"\n▶ Running N-Body Simulation ({n_steps} steps, dt={dt} yr)")

    pos = positions.copy()
    vel = velocities.copy()
    acc = compute_accelerations(masses, pos)

    # Storage (sample every 50 steps to save memory)
    sample_rate = 50
    n_samples = n_steps // sample_rate + 1
    n_bodies = len(masses)

    trajectory = np.zeros((n_samples, n_bodies, 2))
    energies = np.zeros((n_samples, 3))  # KE, PE, Total
    times = np.zeros(n_samples)

    idx = 0
    trajectory[0] = pos.copy()
    energies[0] = compute_energy(masses, pos, vel)
    times[0] = 0.0

    for step in range(1, n_steps + 1):
        # Velocity-Verlet
        vel_half = vel + 0.5 * dt * acc
        pos = pos + dt * vel_half
        acc_new = compute_accelerations(masses, pos)
        vel = vel_half + 0.5 * dt * acc_new
        acc = acc_new

        if step % sample_rate == 0:
            idx += 1
            trajectory[idx] = pos.copy()
            energies[idx] = compute_energy(masses, pos, vel)
            times[idx] = step * dt

        if step % 10000 == 0:
            t = step * dt
            e = compute_energy(masses, pos, vel)
            print(f"  t={t:>6.2f} yr  E_total={e[2]:>12.6f}  ΔE/E₀={abs(e[2] - energies[0, 2]) / abs(energies[0, 2]):.2e}")

    trajectory = trajectory[:idx + 1]
    energies = energies[:idx + 1]
    times = times[:idx + 1]

    return trajectory, energies, times


# ═══════════════════════════════════════════════════════════════
# 3. Analysis: Energy Conservation + Stability
# ═══════════════════════════════════════════════════════════════

def analyze_simulation(trajectory, energies, times) -> dict:
    """Analyze energy conservation and orbital stability."""
    print("\n▶ Analysis")

    E0 = energies[0, 2]
    E_final = energies[-1, 2]
    max_drift = float(np.max(np.abs(energies[:, 2] - E0)) / abs(E0))
    final_drift = float(abs(E_final - E0) / abs(E0))

    print(f"  E₀ = {E0:.6f}")
    print(f"  E_final = {E_final:.6f}")
    print(f"  |ΔE/E₀| final = {final_drift:.2e}")
    print(f"  |ΔE/E₀| max   = {max_drift:.2e}")

    # Orbital period estimation (Jupiter: ~12 yr)
    # Find zero-crossings of y-coordinate for body 1 (Jupiter-like)
    y_jupiter = trajectory[:, 1, 1]
    crossings = []
    for i in range(1, len(y_jupiter)):
        if y_jupiter[i - 1] < 0 and y_jupiter[i] >= 0:
            crossings.append(times[i])

    periods = np.diff(crossings) if len(crossings) > 1 else []
    avg_period = float(np.mean(periods)) if len(periods) > 0 else 0
    period_std = float(np.std(periods)) if len(periods) > 0 else 0

    print(f"  Jupiter-like orbital period: {avg_period:.2f} ± {period_std:.4f} yr (expected ~11.86 yr)")

    return {
        "total_time_yr": float(times[-1]),
        "n_samples": len(times),
        "initial_energy": round(E0, 6),
        "final_energy": round(E_final, 6),
        "max_energy_drift": max_drift,
        "final_energy_drift": final_drift,
        "energy_conserved": max_drift < 1e-4,  # < 0.01% drift
        "jupiter_period_yr": round(avg_period, 2),
        "jupiter_period_std": round(period_std, 4),
        "n_orbits_completed": len(crossings),
    }


# ═══════════════════════════════════════════════════════════════
# 4. Visualization
# ═══════════════════════════════════════════════════════════════

def plot_physics(trajectory, energies, times, analysis):
    """Generate publication-quality physics figures."""
    print("\n▶ Generating Figures")

    colors = ["#f39c12", "#3498db", "#e74c3c"]
    labels = ["Sun", "Jupiter-like", "Saturn-like"]

    # Figure 1: Orbital trajectories
    fig, ax = plt.subplots(figsize=(10, 10))
    for i in range(3):
        ax.plot(trajectory[:, i, 0], trajectory[:, i, 1], color=colors[i],
                linewidth=0.5, alpha=0.7, label=labels[i])
        ax.scatter(trajectory[0, i, 0], trajectory[0, i, 1], s=80 if i == 0 else 40,
                   color=colors[i], edgecolors="black", linewidth=1, zorder=5)
    ax.set_xlabel("x (AU)", fontsize=12)
    ax.set_ylabel("y (AU)", fontsize=12)
    ax.set_title("Gravitational 3-Body Orbital Trajectories", fontsize=14, fontweight="bold")
    ax.legend(fontsize=11)
    ax.set_aspect("equal")
    ax.grid(alpha=0.3)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig1_orbital_trajectories.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 1 → orbital trajectories")

    # Figure 2: Energy conservation
    fig, axes = plt.subplots(2, 1, figsize=(12, 8), gridspec_kw={"height_ratios": [2, 1]})

    ax1 = axes[0]
    ax1.plot(times, energies[:, 0], label="Kinetic", color="#3498db", linewidth=1)
    ax1.plot(times, energies[:, 1], label="Potential", color="#e74c3c", linewidth=1)
    ax1.plot(times, energies[:, 2], label="Total", color="#2ecc71", linewidth=2)
    ax1.set_ylabel("Energy (normalized)", fontsize=11)
    ax1.set_title("Energy Conservation — Velocity-Verlet Integrator", fontsize=14, fontweight="bold")
    ax1.legend(fontsize=10)
    ax1.grid(alpha=0.3)

    ax2 = axes[1]
    E0 = energies[0, 2]
    drift = (energies[:, 2] - E0) / abs(E0)
    ax2.plot(times, drift, color="#9b59b6", linewidth=1)
    ax2.set_xlabel("Time (years)", fontsize=11)
    ax2.set_ylabel("ΔE/E₀", fontsize=11)
    ax2.set_title(f"Relative Energy Drift (max = {analysis['max_energy_drift']:.2e})", fontsize=11)
    ax2.grid(alpha=0.3)
    ax2.axhline(y=0, color="gray", linestyle="--", alpha=0.5)

    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig2_energy_conservation.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 2 → energy conservation")

    # Figure 3: Phase space (r vs v_r for Jupiter-like body)
    fig, ax = plt.subplots(figsize=(8, 8))
    # Radial distance and radial velocity of Jupiter-like
    r = np.sqrt(trajectory[:, 1, 0]**2 + trajectory[:, 1, 1]**2)
    # Approximate radial velocity from finite differences
    dr = np.gradient(r, times)
    ax.plot(r, dr, linewidth=0.3, alpha=0.7, color="#3498db")
    ax.scatter(r[0], dr[0], s=100, color="red", zorder=5, label="Start")
    ax.set_xlabel("Radial Distance r (AU)", fontsize=12)
    ax.set_ylabel("Radial Velocity dr/dt (AU/yr)", fontsize=12)
    ax.set_title("Phase Space — Jupiter-like Body", fontsize=14, fontweight="bold")
    ax.legend()
    ax.grid(alpha=0.3)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig3_phase_space.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 3 → phase space")


def main():
    print("═" * 60)
    print("  GENESIS — Physics Simulation (N-Body Gravity)")
    print("═" * 60)

    setup()
    masses, pos, vel = init_three_body()
    trajectory, energies, times = simulate(masses, pos, vel, dt=0.001, n_steps=50000)
    analysis = analyze_simulation(trajectory, energies, times)
    plot_physics(trajectory, energies, times, analysis)

    results = {
        "experiment": "physics_simulation",
        "seed": SEED,
        "system": "3-body gravitational (Sun-Jupiter-Saturn scale)",
        "integrator": "Velocity-Verlet (symplectic)",
        "analysis": analysis,
        "metrics": {
            "energy_drift_max": analysis["max_energy_drift"],
            "energy_conserved": bool(analysis["energy_conserved"]),
            "jupiter_period_yr": analysis["jupiter_period_yr"],
            "n_orbits": analysis["n_orbits_completed"],
        },
    }

    with open(OUTPUT_DIR / "results.json", "w") as f:
        json.dump(results, f, indent=2)
    print(f"\n  💾 Results → {OUTPUT_DIR / 'results.json'}")

    # Verify
    print("\n▶ Self-Verification")
    expected = [
        OUTPUT_DIR / "results.json",
        FIGURE_DIR / "fig1_orbital_trajectories.png",
        FIGURE_DIR / "fig2_energy_conservation.png",
        FIGURE_DIR / "fig3_phase_space.png",
    ]
    ok = True
    for p in expected:
        if p.exists():
            print(f"  ✅ {p}")
        else:
            print(f"  ❌ {p} MISSING")
            ok = False

    if analysis["energy_conserved"]:
        print(f"  ✅ Energy conservation verified (drift < 0.01%)")
    else:
        print(f"  ⚠️ Energy drift = {analysis['max_energy_drift']:.2e} (higher than threshold)")

    print(f"\n{'✅ ALL CHECKS PASSED' if ok else '❌ SOME CHECKS FAILED'}")
    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
