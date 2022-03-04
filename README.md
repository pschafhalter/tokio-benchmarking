# Tokio Profiling

## Takeaways

- `tokio::task::spawn_blocking` is about 5 us slower than `tokio::task::spawn(async { tokio::task::block_in_place(...) })` (12 us vs 12 us)
- No blocking hint reduces down to 10 us.

4 producers, 2 consumers, noop.
- 2 consumers: 10 us
- consumer manager, `spawn_blocking`: 64 us
- consumer manager, `spawn(block_in_place(..))`: 28 us
- consumer manager, `spawn`: 12 us

4 producers, 2 consumers, 100us spin loop.
- 2 consumers: 11 us
- consumer manager, `spawn_blocking`: 73 us
- consumer manager, `spawn(block_in_place(..))`: 37 us
- consumer manager, `spawn`: 13 us
