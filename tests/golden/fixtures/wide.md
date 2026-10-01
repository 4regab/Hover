A wide table and a long line of code, which scroll sideways.

| Path | Status | Owner | Reviewed | Notes |
|---|---|---|---|---|
| src/auth/refresh_token_rotation_service.ts | changed | platform-identity-team | 2026-09-27 | expiry_check_moved_out_of_the_hot_path |
| tests/refresh.test.ts | added | platform-identity-team | 2026-09-28 | covers_the_thirty_day_window |

```sh
dotnet test tests/Hover.Tests/Hover.Tests.csproj --filter "FullyQualifiedName~Refresh" --logger "console;verbosity=detailed"
```

A short table still fits:

| a | b |
|---|---|
| 1 | 2 |
