"""A subset of the NIST SP 800-90B non-IID min-entropy estimators.

Pure-Python reimplementation of the four estimators from SP 800-90B
§6.3 that apply to non-binary samples and are cheap at the sample sizes
an 8-bit MCU can stream in minutes: Most Common Value (6.3.1), t-Tuple
(6.3.5), MultiMCW prediction (6.3.7) and Lag prediction (6.3.8). The
assessed min-entropy is the minimum over the estimators, as in the
standard.

This is NOT a substitute for NIST's `ea_non_iid`, which also runs the
LRS, MultiMMC, LZ78Y and bitstring estimators. On this project's
captures it matched `ea_non_iid` exactly on MCV and t-tuple, but missed
the lower MultiMMC/compression bounds on the nRF52840 sources -- use it
as a quick bench-side check, and quote `ea_non_iid` numbers.
"""

import math
from collections import Counter, defaultdict

Z = 2.576  # 99% (two-sided z used by SP 800-90B)


def mcv(s):
    n = len(s)
    p = max(Counter(s).values()) / n
    pu = min(1.0, p + Z * math.sqrt(p * (1 - p) / (n - 1)))
    return -math.log2(pu)


def t_tuple(s, cutoff=35):
    n = len(s)
    pmax = 0.0
    t = 1
    while True:
        c = Counter(tuple(s[i:i + t]) for i in range(n - t + 1))
        q = max(c.values())
        if q < cutoff:
            break
        p = q / (n - t + 1)
        pmax = max(pmax, p ** (1 / t))
        t += 1
        if t > 32:
            break
    if pmax == 0.0:
        # Not even single symbols repeat `cutoff` times: too few samples
        # for this estimator to say anything.
        return math.inf
    pu = min(1.0, pmax + Z * math.sqrt(pmax * (1 - pmax) / (n - 1)))
    return -math.log2(pu)


def _p_local(n_pred, r):
    """Largest p whose longest-run probability is still >= 1%, i.e. the
    SP 800-90B §6.3.7 step 10 local bound, by bisection in log space.
    `r` is the longest run of correct predictions plus one."""
    log_target = math.log(0.99)

    def log_f(p):
        q = 1 - p
        x = 1.0
        for _ in range(10):
            x = 1 + q * p ** r * x ** (r + 1)
        num = 1 - p * x
        den = (r + 1 - r * x) * q
        if num <= 0 or den <= 0:
            return -math.inf
        return math.log(num / den) - (n_pred + 1) * math.log(x)

    lo, hi = 0.0, 1.0
    for _ in range(80):
        mid = (lo + hi) / 2
        # The probability of no run longer than r decreases as p grows.
        if log_f(mid) > log_target:
            lo = mid
        else:
            hi = mid
    return lo


def _predictor_result(correct, k):
    n = len(correct)
    c = sum(correct)
    pg = c / n
    pg_u = 1 - 0.01 ** (1 / n) if c == 0 else min(1.0, pg + Z * math.sqrt(pg * (1 - pg) / (n - 1)))
    r = run = 0
    for x in correct:
        run = run + 1 if x else 0
        r = max(r, run)
    pl = _p_local(n, r + 1)
    p = max(pg_u, pl, 1 / k)
    return -math.log2(p)


def lag_prediction(s, D=128):
    k = len(set(s))
    scores = [0] * D
    winner = 0
    correct = []
    for i in range(D, len(s)):
        correct.append(s[i - winner - 1] == s[i])
        for d in range(D):
            if s[i - d - 1] == s[i]:
                scores[d] += 1
                if scores[d] >= scores[winner]:
                    winner = d
    return _predictor_result(correct, k)


def multi_mcw(s, windows=(63, 255, 1023, 4095)):
    k = len(set(s))
    w_max = windows[-1]
    scores = [0] * len(windows)
    winner = 0
    counts = [Counter() for _ in windows]
    correct = []
    for i in range(len(s)):
        preds = []
        for j, w in enumerate(windows):
            if i >= w:
                # most common in window, ties broken by most recent
                c = counts[j]
                m = max(c.values())
                best = None
                for back in range(1, w + 1):
                    v = s[i - back]
                    if c[v] == m:
                        best = v
                        break
                preds.append(best)
            else:
                preds.append(None)
        if i >= w_max:
            correct.append(preds[winner] == s[i])
        for j, p in enumerate(preds):
            if p is not None and p == s[i]:
                scores[j] += 1
                if scores[j] >= scores[winner]:
                    winner = j
        for j, w in enumerate(windows):
            counts[j][s[i]] += 1
            if i - w >= 0:
                counts[j][s[i - w]] -= 1
    return _predictor_result(correct, k)


def assess(s):
    res = {
        "mcv": mcv(s),
        "t_tuple": t_tuple(s),
        "lag": lag_prediction(s),
        "multi_mcw": multi_mcw(s),
    }
    res["min"] = min(res.values())
    return res
