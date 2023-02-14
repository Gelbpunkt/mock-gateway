# mock-gateway

This is heavy WIP, but you can try it already. Many features aren't implemented yet.

## Concept

This mock gateway serves a completely[^1] functioning fake Discord gateway that supports identifying, resuming, heartbeating and so on. The data sent by the mock gateway can be configured in the configuration file, `config.json`. Application and bot data is specified explicitly, while for data for guilds, users, voice states and channels, only the amount of each is specified and then generated randomly at runtime.

You can make the mock gateway behave abnormal by enabling specific scenarios in the configuration file that will simulate for example heartbeat timeouts or failing resumes.

The main feature of the mock gateway is that it allows scripting its behaviour in a file called `script.txt`. Here, you can configure how it will behave once the client is ready (identified or resumed). The script allows you to disconnect clients, invalidate their sessions and send random or custom payloads to allow you to test your client's handling of edge cases.

[^1]: At some point. Currently only very basic functionality.

## Configuration

Please look at `config.example.json` for a full configuration file example and `script.example.txt` for an idea of how the scripting language works.

## Roadmap

- [x] Clients can connect, identify and resume
- [x] Basic scripts can be run (sleep/invalidate session/custom event dispatch/heartbeats)
- [x] Scenarios for heartbeat timeouts and failing resumes
- [ ] Add random guild/channel/member/user/voice state generators
- [ ] Use these generators to send startup `GUILD_CREATE` mock events
- [ ] Implement script instructions for random events
- [ ] Implement script instructions for disconnects
- [ ] More scenarios?
- [ ] "Wait until client reconnected and is ready" script instruction
- [ ] Loops in scripts
- [ ] Implement server responses to client-side requests (query members, etc.)
