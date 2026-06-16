"""Put the forkb package dir on sys.path so tests can `from _schema import ...`, `from sue import ...`, etc."""
import os
import sys

_FORKB = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
if _FORKB not in sys.path:
    sys.path.insert(0, _FORKB)
