# AMRI Shadow Race

Shadow Race is an instant consensus gate for route switching. The probe layer launches a small bounded set of measurements concurrently while the active route keeps carrying traffic. The core then evaluates the completed burst synchronously.

Default policy:

- at least 3 parallel samples;
- at least two thirds must beat the active route;
- improvement at least 8%;
- challenger confidence at least 55%.

The median improvement and median confidence must also pass their thresholds. One isolated spike cannot authorize a switch. Emergency failover from unavailable or quarantined routes remains the responsibility of the existing health/runtime path and is never delayed by Shadow Race.
