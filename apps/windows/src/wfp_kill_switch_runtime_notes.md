# WFP kill switch runtime contract

The WFP policy is intentionally dynamic and must only be held while a protected AMRI route owns the Windows TUN. The production integration must activate it after Wintun/tproxy setup, before a connection is reported Ready, keep the guard alive for the full protected-route lifetime, and drop it during reverse-order teardown. Endpoint permits are restricted to addresses resolved and pinned before default-route capture. No credential-bearing node URI is written to the filter engine or logs.
