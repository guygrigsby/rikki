import sys


def main():
    sys.stderr.write(
        "rikki is now nevla (binaries nevla/nv, files .nv).\n"
        "Install it:  uv tool install nevla\n"
        "Why: https://github.com/guygrigsby/nevla/blob/main/docs/adr/0014-rename-to-nevla.md\n"
    )
    return 1
