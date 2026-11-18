#!/usr/bin/env python3
"""Unit tests for the perf-smoke comparison.

Run from the repository root with:
    python3 -m unittest discover -s scripts/ci -p 'test_*.py'
"""

import unittest

from perf_smoke import evaluate_scenario, median

# dom-build samples from CI run 44351785562 (PR #25, no DOM change). The runner
# was bimodal (~114 ms vs ~165 ms) and the old median gate failed at 1.43x.
NOISY_BASE_MS = [188.8, 114.3, 114.8, 115.9, 114.5]
NOISY_CANDIDATE_MS = [114.5, 115.5, 166.1, 164.6, 164.6]
FLAT_RSS_KIB = [40_000] * 5


def samples(base_ms, candidate_ms, base_rss=FLAT_RSS_KIB, candidate_rss=FLAT_RSS_KIB):
    return {
        "base": {"latency_ms": list(base_ms), "rss_kib": list(base_rss)},
        "candidate": {"latency_ms": list(candidate_ms), "rss_kib": list(candidate_rss)},
    }


class EvaluateScenarioTests(unittest.TestCase):
    def test_bimodal_runner_noise_passes(self):
        _, failures = evaluate_scenario("dom-build", samples(NOISY_BASE_MS, NOISY_CANDIDATE_MS))

        self.assertEqual(failures, [])

    def test_slow_outliers_on_the_candidate_only_pass(self):
        # Noise that hits only the candidate side must not fail the gate.
        base = [114.3, 114.8, 115.9, 114.5, 114.6]
        candidate = [114.5, 166.1, 164.6, 164.6, 188.8]

        _, failures = evaluate_scenario("dom-build", samples(base, candidate))

        self.assertEqual(failures, [])

    def test_noisy_samples_would_fail_on_the_median(self):
        # Guards the reason for the statistic: the same data trips a median gate.
        ratio = median(NOISY_CANDIDATE_MS) / median(NOISY_BASE_MS)

        self.assertGreater(ratio, 1.20)

    def test_uniform_regression_fails(self):
        slower = [value * 1.5 for value in NOISY_BASE_MS]

        _, failures = evaluate_scenario("dom-build", samples(NOISY_BASE_MS, slower))

        self.assertEqual(failures, ["dom-build latency is 1.50x the base"])

    def test_regression_under_bimodal_noise_fails(self):
        slower = [value * 1.5 for value in NOISY_CANDIDATE_MS]

        _, failures = evaluate_scenario("dom-build", samples(NOISY_BASE_MS, slower))

        self.assertEqual(len(failures), 1)
        self.assertIn("dom-build latency", failures[0])

    def test_small_absolute_delta_does_not_fail(self):
        # 2 ms to 3 ms is +50% but under the 10 ms floor.
        _, failures = evaluate_scenario("tiny", samples([2.0, 2.1], [3.0, 3.1]))

        self.assertEqual(failures, [])

    def test_rss_regression_still_uses_median_and_fails(self):
        bloated = [value * 1.5 for value in FLAT_RSS_KIB]

        _, failures = evaluate_scenario(
            "dom-build",
            samples(NOISY_BASE_MS, NOISY_BASE_MS, candidate_rss=bloated),
        )

        self.assertEqual(failures, ["dom-build RSS is 1.50x the base"])


if __name__ == "__main__":
    unittest.main()
