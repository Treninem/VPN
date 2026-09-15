# AMRI Route Proof

Route Proof is a local, tamper-evident receipt for an AMRI routing decision.

A receipt contains the algorithm version, time, selected node, explanation, normalized candidate evidence, the previous proof hash and its HMAC. Host and process identity are inputs only to an installation-keyed 256-bit pseudonym; raw values are not stored. Different installation secrets create unlinkable tokens.

The installation secret must come from the OS CSPRNG and be stored through amri-secrets (DPAPI on Windows, Android Keystore adapter on Android). It is zeroed from the in-memory chain on drop and is never federated.

verify detects changed evidence or route choice. verify_chain also detects removal or reordering of interior records. Clearing learning intentionally begins a new chain.
