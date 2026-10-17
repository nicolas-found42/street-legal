# 0005: Exact SDAT saves

Supersedes 0002. The original scripts run unchanged and save through native methods that write SDAT.
Keeping JSON would mean reimplementing those natives with different behaviour, which contradicts ADR
0004. Saves are therefore read and written as SDAT, byte-exact with the original, so original save files
also load. The JSON saves of the approximate slice are dropped with it.
