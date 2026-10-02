"""Shape helpers."""


def area_of_rect(width, height):
    """Area of a width x height rectangle."""
    if width < 0 or height < 0:
        raise ValueError("sides must be non-negative")
    return width * height


def perimeter_of_rect(width, height):
    return 2 * (width + height)
