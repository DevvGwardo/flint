"""Small statistics helpers."""


def mean(values):
    if not values:
        raise ValueError("mean() of an empty list")
    return sum(values) / len(values)


def median(values):
    if not values:
        raise ValueError("median() of an empty list")
    ordered = sorted(values)
    mid = len(ordered) // 2
    return ordered[mid]


def spread(values):
    return max(values) - min(values)
