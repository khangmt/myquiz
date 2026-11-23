# Load test, 23 Nov 2026

NFR-02, NFR-03 and design section 11: how many games of 200 players one
machine carries while live events still reach players in time.

## How it was run

- **Tool:** `crates/server/examples/loadtest.rs`. Each simulated game has a host and 200 players.
  - The host opens the game through the real API and clicks through 5 questions, waiting 2 s on each reveal and leaderboard.
  - Players join over about a second. Each answers 0.3 to 4 s after the options show (70% right), so every question ends early, on the last answer.
  - Games start spread over 20 s, so all games hit their questions within seconds of each other. That is harsher than a real day.
- **Machine:** one 4-core Xeon (2.3 GHz) VM with 15 GB of RAM.
  - It ran PostgreSQL 16, Redis 7, the API and three game servers (release builds), with no Docker and no Caddy.
  - The load generator ran on the same machine and used 1 to 1.3 cores of its own at the top sizes.
- **Measures:** delays are measured from the host's click (or the last answer) to each player's screen, on a clock everyone shares.

## Results

Delays are in milliseconds, as p95 / max. NFR-02 asks for p95 ≤ 250 and max ≤ 500 for events that show at once (reveal, leaderboard).
A question starts with a 3 s "get ready" countdown, so its delay only matters past 3 s.

| Games × players | Players | Question start | Leaderboard | Result after last answer | Answer accepted | Errors | Game servers, CPU peak / RAM |
|---|---|---|---|---|---|---|---|
| 10 × 200 | 2,000 | 5 / 8 | 5 / 6 | 15 / 19 | 0 / 11 | 0 | 0.3 cores / 320 MB¹ |
| 50 × 200 | 10,000 | 6 / 20 | 7 / 14 | 20 / 62 | 1 / 22 | 0 | 1.0 cores / 306 MB |
| 100 × 200 | 20,000 | 14 / 43 | 10 / 46 | 31 / 98 | 3 / 57 | 0 | 1.5 cores / 1.26 GB² |
| 150 × 200 | 30,000 | 44 / 388 | 24 / 168 | 53 / 305 | 11 / 143 | 0 | 1.6 cores / 1.24 GB |
| 200 × 200 | 40,000 | 868 / 3,165 | 161 / 871 | 1,619 / 6,227 | 111 / 703 | 0 | 1.8 cores / 1.29 GB |

¹ This run used the old 128 KiB socket buffers. With them, 50 games took 1.48 GB, about 150 KB per player, so 200 games would have gone over the 2 GB the two game containers may use.
Since the change, a player costs about 30 to 40 KB.

² From 100 games on, three game servers shared the players instead of two.

- **Up to 150 games** every target is met: no question arrived after its countdown, and the slowest reveal took 305 ms.
- **At 200 games** every game still finished with no errors, but the machine was full.
  - The load generator, game servers, PostgreSQL and Redis together wanted more than the 4 cores.
  - Reveals then took up to 6 s, which misses NFR-02.
  - Nothing piled up or crashed; the work was simply queued behind the CPU.

## Decision

- **Launch limit.** `MAX_ACTIVE_GAMES` is 100 at launch, as the design planned.
  - That is two thirds of what this smaller machine carried with the load generator on board.
  - Game 101 is refused with a clear message, and running games are not slowed.
- **Raising it.** The faculty server's cores are shared with GPU users' jobs. After the 200-game run there (from a second machine; it is one of the open questions for the server admin), raise the limit to the highest size that met NFR-02.
  - Raising it is one line in `infra/.env` and an api restart.
- **Socket buffers.** These are now 8 KiB instead of the 128 KiB default. That cut memory per player by about 5 times; nothing sent to a player is anywhere near 8 KiB.

## Run it again

```sh
cargo build --release --bin myquiz --example loadtest
# PostgreSQL and Redis running, then the API and one or more game servers:
MAX_ACTIVE_GAMES=400 ./target/release/myquiz api &
BIND_ADDR=127.0.0.1:8081 ./target/release/myquiz game &
BIND_ADDR=127.0.0.1:8082 ./target/release/myquiz game &

ulimit -n 20000   # one open file per simulated player
./target/release/examples/loadtest --games 50 --players 200 \
  --ws ws://127.0.0.1:8081/ws,ws://127.0.0.1:8082/ws --json result.json
./target/release/examples/loadtest --cleanup   # removes the test accounts
```

Against the composed stack, point `--api` at Caddy and `--ws` at `ws://<host>/ws`, and give the tool the database and Redis addresses so it can create the hosts' accounts.

Above about 18,000 players, run several copies with different `--tag` values, because one process can only open about 20,000 files.
