import sys

import geometry


def main(argv):
    width, height = (int(v) for v in argv[1:3])
    print(geometry.area_of_rect(width, height))


if __name__ == "__main__":
    main(sys.argv)
