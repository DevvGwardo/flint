from geometry import area_of_rect, perimeter_of_rect


def summary(rooms):
    """One line per (width, height) room, plus the total area."""
    lines = []
    total = 0
    for width, height in rooms:
        area = area_of_rect(width, height)
        total += area
        lines.append(f"{width}x{height}: area {area}, perimeter {perimeter_of_rect(width, height)}")
    lines.append(f"total area {total}")
    return "\n".join(lines)
