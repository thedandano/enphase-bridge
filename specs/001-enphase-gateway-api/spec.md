# Feature Specification: Enphase Gateway Data Service

**Feature Branch**: `001-enphase-gateway-api`  
**Created**: 2026-04-26  
**Status**: Draft  
**Input**: User description: "I want to be able to query my enphase local gateway api and store data. Perhaps i want to vend it to a webapp or mobile app."

## Clarifications

### Session 2026-04-26

- Q: Should the HTTP data API require any form of authentication? → A: No auth — LAN-trust model; anyone on the local network can read energy data.
- Q: What data should the service collect from the gateway? → A: Aggregate production, consumption, and grid flow in 15-minute windows; plus per-microinverter power production snapshots at each 15-minute interval. Also ingest SDGE TOU rate data to enable NEM true-up cost estimation.
- Q: How should SDGE TOU rate data be ingested? → A: OpenEI Utility Rate Database (URDB) API as the sole source (self-service free API key, SDGE rates confirmed present). No manual fallback. Endpoint: developer.nlr.gov (migrated from developer.nrel.gov — new endpoint must be used).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Poll and Store Solar Data (Priority: P1)

As a homeowner with an Enphase IQ Gateway on my local network, I want a background service to periodically query my gateway for solar production and consumption data and persist it to a local store, so I have a growing historical record of my energy usage.

**Why this priority**: Without this data collection loop, nothing else is possible — it is the foundation of the system.

**Independent Test**: Can be validated by starting the service, waiting for a polling cycle to complete, and confirming a new energy record exists in storage. Delivers standalone value as a headless data logger.

**Acceptance Scenarios**:

1. **Given** the service is running and the gateway is reachable, **When** a polling interval elapses, **Then** current production and consumption readings are fetched and saved with a timestamp.
2. **Given** the gateway returns an error or is temporarily unreachable, **When** a polling cycle fails, **Then** the failure is logged with context and the service retries on the next interval without crashing.
3. **Given** the service has been running for 24 hours, **When** a user queries stored data, **Then** readings are available for every polling interval during that period.

---

### User Story 2 - Authenticate with Local Gateway (Priority: P1)

As a homeowner, I want the service to handle authentication with my Enphase IQ Gateway transparently, so I don't need to manually manage tokens to keep data flowing.

**Why this priority**: The gateway requires a JWT token for local API access. Without authentication, no data can be fetched.

**Independent Test**: Can be validated by configuring credentials, starting the service, and confirming it successfully retrieves a data sample without manual token intervention.

**Acceptance Scenarios**:

1. **Given** valid Enphase cloud credentials are configured, **When** the service starts, **Then** it obtains a local access token and uses it for subsequent gateway requests.
2. **Given** a token is nearing expiry, **When** the service detects this before the next poll, **Then** it refreshes the token automatically without interrupting data collection.
3. **Given** credentials are invalid or revoked, **When** the service attempts authentication, **Then** it logs a clear error and halts polling until credentials are corrected.

---

### User Story 3 - Query Per-Inverter and Aggregate Energy Data via API (Priority: P2)

As a developer building a web or mobile app, I want to query stored energy data — both aggregate system totals and per-microinverter output — in 15-minute windows through a structured API, so I can display historical charts, identify underperforming panels, and show live-ish readings.

**Why this priority**: This unlocks all consumer-facing use cases (dashboards, mobile apps) built on top of the collector.

**Independent Test**: Can be validated by hitting the API endpoint and confirming it returns paginated 15-minute window records with per-inverter breakdowns in a defined format. No UI required.

**Acceptance Scenarios**:

1. **Given** data has been collected, **When** a client requests energy readings for a time range, **Then** the API returns matching 15-minute window records ordered by timestamp, each including aggregate production, consumption, and grid flow.
2. **Given** data has been collected, **When** a client requests per-inverter data for a specific 15-minute window, **Then** the API returns the power output for each microinverter recorded in that window.
3. **Given** the client requests the latest reading, **When** the API responds, **Then** the most recently stored 15-minute window is returned.
4. **Given** an invalid time range is provided, **When** the API receives the request, **Then** it returns a clear error response with a description of what was wrong.

---

### User Story 4 - Estimate NEM True-Up Cost Using SDGE TOU Rates (Priority: P2)

As a homeowner enrolled in SDGE net energy metering (NEM), I want the service to apply SDGE TOU rate schedules to my collected energy data so I can see an estimated annual true-up cost (or credit), so I understand my real electricity economics before my billing anniversary.

**Why this priority**: Without TOU rate context, the raw watt-hour data has no financial meaning. True-up estimation is the primary economic output of the system.

**Independent Test**: Can be validated by loading a known TOU rate schedule, feeding in a known set of energy readings, and confirming the computed true-up estimate matches a hand-calculated expected value.

**Acceptance Scenarios**:

1. **Given** SDGE TOU rates are loaded and energy data exists, **When** a client requests a true-up estimate for a date range, **Then** the service returns estimated net import/export cost broken down by TOU period (peak, off-peak, super-off-peak).
2. **Given** the TOU rate schedule changes mid-year, **When** the estimate is computed, **Then** the correct historical rates are applied to each time window (rates are not retroactively updated).
3. **Given** no TOU data is loaded, **When** a true-up estimate is requested, **Then** the API returns a clear error indicating TOU rates must be configured first.

---

### User Story 5 - View Live and Historical Data in a Dashboard (Priority: P3)

As a homeowner, I want a simple web dashboard showing current solar production, home consumption, and historical trends, so I can understand my energy patterns without writing code.

**Why this priority**: Nice-to-have consumer surface; the core system is useful without it, and a third-party app can be used instead.

**Independent Test**: Can be validated by opening the dashboard URL and confirming current readings and a time-series chart are visible.

**Acceptance Scenarios**:

1. **Given** the service is running, **When** I open the dashboard, **Then** I see current production and consumption values updated at least once per minute.
2. **Given** data exists for the past 7 days, **When** I view the historical chart, **Then** daily production and consumption totals are displayed.

---

### Edge Cases

- What happens when the gateway is on the LAN but returns malformed JSON?
- How does the service behave if the local storage reaches capacity or the disk is full?
- What happens when the Enphase cloud is unreachable during token refresh but the gateway is still available locally?
- How are duplicate readings (same timestamp, different values) handled in storage?
- What happens if the polling interval is shorter than the gateway's data refresh rate?
- What happens if one or more microinverters go offline or are missing from a 15-minute snapshot?
- What happens when a SDGE TOU rate schedule is updated — are past estimates invalidated or preserved?
- How is the true-up period defined — rolling 12 months, calendar year, or NEM billing anniversary?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The data collection service MUST poll the Enphase IQ Gateway and record aggregate production, consumption, and net grid flow readings aligned to 15-minute windows with timestamps. The polling interval is configurable (default: 60 seconds) but data is bucketed and queryable in 15-minute increments.
- **FR-002**: The service MUST authenticate with the Enphase IQ Gateway using JWT tokens obtained via the Enphase cloud authentication flow.
- **FR-003**: The service MUST automatically refresh authentication tokens before they expire without human intervention.
- **FR-004**: The service MUST persist collected readings to a local data store with enough fidelity to reconstruct time-series charts.
- **FR-005**: The service MUST expose an HTTP API that allows clients to retrieve energy readings filtered by time range and to fetch the most recent reading.
- **FR-006**: The service MUST log all polling cycles, authentication events, and errors in structured format.
- **FR-007**: The service MUST handle gateway unavailability gracefully — retrying on the next interval and logging the failure — without crashing.
- **FR-008**: The data API MUST be accessible over the local network so web and mobile clients can consume it. The API requires no authentication — local network access is the trust boundary (LAN-trust model).
- **FR-009**: The backend service MUST be implemented in Rust.
- **FR-010**: The service MUST record a per-microinverter power production snapshot at each 15-minute interval boundary, capturing the output (watts) for every microinverter in the system.
- **FR-011**: The service MUST fetch SDGE TOU rate schedule data from the OpenEI Utility Rate Database (URDB) API (endpoint: `developer.nlr.gov`) using a configured API key, and store versioned rate schedules locally for use in cost calculations.
- **FR-012**: The service MUST provide an endpoint that computes an estimated NEM true-up cost (or credit) by applying stored SDGE TOU rates to collected energy import/export data for a specified date range.

### Key Entities

- **EnergyWindow**: A 15-minute aggregated snapshot of system-level power data. Includes: window start timestamp, watt-hours produced (solar), watt-hours consumed (home), net grid flow (import/export watt-hours), and data completeness flag (whether all polls in the window succeeded).
- **MicroinverterSnapshot**: A per-device power reading taken at each 15-minute boundary. Includes: window start timestamp, microinverter serial number, watts output at snapshot time, and online status.
- **TOURateSchedule**: A versioned SDGE time-of-use rate schedule. Includes: effective date, expiry date, and rate entries each specifying day type (weekday/weekend/holiday), hour range, period name (peak/off-peak/super-off-peak), and rate in $/kWh for both import and export.
- **TrueUpEstimate**: A computed NEM cost estimate for a date range. Includes: period start/end, total import cost by TOU period, total export credit by TOU period, net balance, and the TOU schedule version used.
- **GatewayConfig**: Connectivity and credential configuration for the local gateway. Includes: gateway IP/hostname, token, token expiry.
- **PollingConfig**: Runtime settings for data collection. Includes: poll interval, retry policy.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Energy readings are collected and stored with no more than a 5-second delay beyond the configured poll interval under normal conditions.
- **SC-002**: The service recovers and resumes polling within one poll cycle after a transient gateway error.
- **SC-003**: The data API returns a time-range query result in under 500 milliseconds for up to 30 days of readings.
- **SC-004**: Token refresh occurs without any missed polling cycles (zero data gaps caused by authentication).
- **SC-005**: The system runs continuously for 30 days without manual intervention or service restart under normal operating conditions.
- **SC-006**: A client app (web or mobile) can retrieve the latest 15-minute window and 7-day history using a single API call each.
- **SC-007**: A true-up estimate for a 12-month period is computed and returned in under 2 seconds.
- **SC-008**: Per-microinverter data for any 15-minute window is available via the API within 30 seconds of the window boundary passing.

## Assumptions

- The Enphase IQ Gateway is accessible on the same local network as the host running this service.
- Authentication follows the JWT-based local token flow documented in the Enphase IQ Gateway Local API documentation.
- Data storage is local (on the same host) for v1; cloud sync is out of scope.
- The web dashboard (User Story 4) is a stretch goal for v1 and may be delivered separately.
- Mobile app is out of scope for v1; the API is designed to be mobile-consumable but no native app is included.
- The user is a single homeowner; multi-user access control is out of scope for v1.
- The polling interval lower bound is 15 seconds to avoid overloading the gateway.
- The service runs on a Linux-capable host (e.g., Raspberry Pi, home server, or Mac).
- The homeowner is enrolled in SDGE net energy metering (NEM) and subject to SDGE TOU rates.
- SDGE TOU rate schedules change infrequently (a few times per year); the service stores historical rate versions so past estimates remain reproducible.
- The true-up period is the 12-month NEM billing anniversary cycle, not a calendar year.
- TOU rates are sourced exclusively from the OpenEI URDB API (`developer.nlr.gov`). A free API key obtained via self-service signup is required at first run. The legacy `developer.nrel.gov` endpoint MUST NOT be used — migration to `developer.nlr.gov` occurred April 2026.
- The OpenEI URDB API does not require approval; signup is instant and free.
