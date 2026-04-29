## 1. GatewayClient session auth

- [x] 1.1 Add `session_id: Option<String>` field to `GatewayClient`
- [x] 1.2 Implement `check_jwt(&mut self) -> Result<(), AppError>` — POST to `/auth/check_jwt`, extract `sessionId` from `Set-Cookie` response header, store in `self.session_id`
- [x] 1.3 Add `cookie_header(&self) -> Option<String>` helper that returns `Some("sessionId=<token>")` when session is present
- [x] 1.4 Update `get_meter_readings(&mut self)` to attach the session cookie, and retry once with fresh `check_jwt()` on 401

## 2. Scheduler startup

- [x] 2.1 Call `self.gateway.check_jwt().await` in `Scheduler::run()` before the poll loop; propagate error as fatal if it fails

## 3. Error handling

- [x] 3.1 Add a `GatewayError::Unauthorized` variant to represent 401 responses distinct from general `Unreachable`
- [x] 3.2 Ensure `check_jwt` failure logs a clear `event = "session_auth_failed"` message

## 4. Tests

- [x] 4.1 Unit test: `parse_session_cookie` extracts `sessionId` correctly from a `Set-Cookie` header value
- [x] 4.2 Integration test: `get_meter_readings` returns `GatewayError::Unauthorized` when session cookie is missing/invalid (mock 401 response)
