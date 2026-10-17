# 0002: JSON saves, not SDAT (superseded by 0004: saves are exact SDAT)

SDAT bodies are undecoded, so writing them would risk corrupting or mimicking saves we cannot
validate. Saves are versioned JSON, written atomically (tmp + rename) and validated on load.
SDAT import/export is future work.
