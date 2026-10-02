/** Shared, credentialed first-party API client. It never persists tokens. */
export type PairingPreview = { request_id: string; device_display_name: string; device_type: string; app_version?: string; verification_phrase: string; short_code: string; expires_at: string; status: "pending"; account_display_name?: string };
export type ApiError = { code: string; message: string; request_id: string; details: Record<string, unknown> };
export type RegistrationOptions = { challenge_id: string; options: Omit<PublicKeyCredentialCreationOptions, "challenge" | "user"> & { challenge: string; user: Omit<PublicKeyCredentialUserEntity, "id"> & { id: string }; } };
export type AuthenticationOptions = { challenge_id: string; options: Omit<PublicKeyCredentialRequestOptions, "challenge" | "allowCredentials"> & { challenge: string; allowCredentials?: Array<PublicKeyCredentialDescriptor & { id: string }>; } };
export type BrowserDevice = { device_id: string; device_display_name: string; device_type: string; connected_at: string; last_seen_at?: string; session_status: "active" | "inactive" };
export type BrowserAccount = { account_display_name: string; device_limit: number; devices: BrowserDevice[]; yandex_home_connected: boolean };
export type YandexHomeProperty = { property_type: string; instance: string; name: string; value: unknown; unit?: string; formatted_value: string; updated_at?: string };
export type YandexHomeDevice = { id: string; name: string; device_type?: string; room_name?: string; properties: YandexHomeProperty[] };
export type YandexHomeSensor = { device_name: string; room_name?: string; property: string; name: string; value: unknown; unit?: string; formatted_value: string; temperature?: number; updated_at?: string };
export type YandexHomeSensorsResponse = { devices: YandexHomeDevice[]; sensors: YandexHomeSensor[] };
export type AdminPage<T> = { items: T[]; limit: number; offset: number; has_more: boolean };
export type AdminDevice = { product: "RockCast" | "RockMobile"; device_type: string; display_name: string; status: string; created_at: string; last_seen_at?: string };
export type AdminAuditEntry = { occurred_at: string; action: string; outcome: string };
export type AdminStation = { station_id: string; name: string; tags: string[]; language?: string; country_code?: string; health: string };
export type AdminIconJob = { id: string; status: "running" | "completed" | "interrupted" | "failed"; selected: number; processed: number; ready: number; missing: number; retryable_error: number; permanent_error: number; skipped: number };
export type StationItem = { id: string; name: string; stream_url?: string; homepage_url?: string; tags: string[]; language?: string; country_code?: string; codec?: string; bitrate_kbps?: number; health?: string; favicon_url?: string };
export type SearchResponse = { request_id: string; stations: StationItem[]; total?: number; has_more?: boolean };
export type StationNowPlaying = { stationId?: string; station_id?: string; state: "fresh" | "stale" | "missing"; rawTitle?: string | null; raw_title?: string | null; updatedAt?: string | null; updated_at?: string | null };
export type PersonalSyncRecord = { record_id: string; station_id?: string; added_at: string; updated_at: string; deleted_at?: string };
export type PersonalHistoryRecord = { record_id: string; station_id?: string; started_at: string; last_played_at: string; ended_at?: string; play_duration_ms?: number; metadata?: Record<string, unknown>; updated_at: string; deleted_at?: string };
export type BrowserSyncResponse = { server_revision: number; server_time: string; favourites: { records: PersonalSyncRecord[] }; history: { records: PersonalHistoryRecord[] }; stations: StationItem[] };
export type BrowserSyncRequest = { since_revision?: number; favourites?: { upserts?: Array<{ record_id: string; station_id: string; added_at: string; updated_at: string }>; deletes?: Array<{ record_id: string; updated_at: string }> }; history?: { upserts?: Array<{ record_id: string; station_id: string; started_at: string; last_played_at: string; ended_at?: string; play_duration_ms?: number; metadata?: Record<string, unknown>; updated_at: string }>; deletes?: Array<{ record_id: string; updated_at: string }> } };

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, { ...init, credentials: "same-origin", headers: { "Content-Type": "application/json", ...init.headers } });
  if (response.status === 204) return undefined as T;
  const body = await response.json().catch(() => undefined);
  if (!response.ok || !body) throw (body ?? { code: "server_unavailable", message: "", request_id: "", details: {} }) as ApiError;
  return body as T;
}

export const api = {
  registrationOptions(payload?: { account_display_name: string }) { return request<RegistrationOptions>("/api/v1/auth/passkeys/registration/options", { method: "POST", body: JSON.stringify(payload ?? {}) }); },
  registrationVerify(payload: unknown) { return request<{ account_display_name: string; csrf_token: string }>("/api/v1/auth/passkeys/registration/verify", { method: "POST", body: JSON.stringify(payload) }); },
  authenticationOptions() { return request<AuthenticationOptions>("/api/v1/auth/passkeys/authentication/options", { method: "POST", body: "{}" }); },
  authenticationVerify(payload: unknown) { return request<{ csrf_token: string }>("/api/v1/auth/passkeys/authentication/verify", { method: "POST", body: JSON.stringify(payload) }); },
  browserSession() { return request<{ account_display_name: string; csrf_token: string }>("/api/v1/auth/browser-session", { method: "POST", body: "{}" }); },
  browserAccount() { return request<BrowserAccount>("/api/v1/browser/account"); },
  yandexHomeAuthorization(csrfToken: string) { return request<{ authorization_url: string }>("/api/v1/browser/yandex-home/authorize", { method: "POST", headers: { "X-CSRF-Token": csrfToken }, body: "{}" }); },
  yandexHomeSensors() { return request<YandexHomeSensorsResponse>("/api/v1/browser/yandex-home/sensors"); },
  disconnectYandexHome(csrfToken: string) { return request<void>("/api/v1/browser/yandex-home", { method: "DELETE", headers: { "X-CSRF-Token": csrfToken } }); },
  renameDevice(deviceId: string, device_display_name: string, csrfToken: string) { return request<void>(`/api/v1/browser/devices/${encodeURIComponent(deviceId)}`, { method: "PATCH", headers: { "X-CSRF-Token": csrfToken }, body: JSON.stringify({ device_display_name }) }); },
  revokeDevice(deviceId: string, csrfToken: string) { return request<void>(`/api/v1/browser/devices/${encodeURIComponent(deviceId)}`, { method: "DELETE", headers: { "X-CSRF-Token": csrfToken } }); },
  logoutBrowser(csrfToken: string) { return request<void>("/api/v1/auth/browser-logout", { method: "POST", headers: { "X-CSRF-Token": csrfToken }, body: "{}" }); },
  pairing(code: string) { return request<PairingPreview>(`/api/v1/pairing-requests/lookup?code=${encodeURIComponent(code)}`); },
  approvePairing(requestId: string, approvalSecret: string, verificationPhrase: string, csrfToken: string) {
    return request<void>(`/api/v1/pairing-requests/${requestId}/approve`, { method: "POST", headers: { "X-CSRF-Token": csrfToken }, body: JSON.stringify({ approval_secret: approvalSecret, verification_phrase: verificationPhrase }) });
  },
  searchStations(query: string, limit = 20, offset = 0) {
    return request<SearchResponse>("/api/v1/search", { method: "POST", body: JSON.stringify({ query, limit, offset }) });
  },
  stationNowPlaying(stationId: string) {
    return request<StationNowPlaying>(`/api/v1/stations/${encodeURIComponent(stationId)}/now-playing`);
  },
  browserSync(payload: BrowserSyncRequest, csrfToken: string) {
    return request<BrowserSyncResponse>("/api/v1/browser/sync", {
      method: "POST",
      headers: { "X-CSRF-Token": csrfToken },
      body: JSON.stringify(payload),
    });
  },
};

async function adminRequest<T>(path: string, token: string, init: RequestInit = {}): Promise<T> {
  return request<T>(path, { ...init, headers: { Authorization: `Bearer ${token}`, ...init.headers } });
}

/** First-party admin calls accept a caller-owned in-memory bearer and never persist it. */
export const adminApi = {
  login(username: string, password: string) { return request<{ access_token: string }>("/api/v1/admin/auth/login", { method: "POST", body: JSON.stringify({ username, password }) }); },
  logout(token: string) { return adminRequest<void>("/api/v1/admin/auth/logout", token, { method: "POST", body: "{}" }); },
  devices(token: string, offset: number) { return adminRequest<AdminPage<AdminDevice>>(`/api/v1/admin/devices?limit=25&offset=${offset}`, token); },
  audit(token: string, offset: number, filters: { from?: string; until?: string; action?: string; outcome?: string }) { const params = new URLSearchParams({ limit: "25", offset: String(offset) }); Object.entries(filters).forEach(([key, value]) => { if (value) params.set(key, value); }); return adminRequest<AdminPage<AdminAuditEntry>>(`/api/v1/admin/audit?${params}`, token); },
  stations(token: string, offset: number, query: string) { const params = new URLSearchParams({ limit: "50", offset: String(offset) }); if (query.trim()) params.set("q", query.trim()); return adminRequest<AdminPage<AdminStation>>(`/api/v1/admin/stations?${params}`, token); },
  latestIconImport(token: string) { return adminRequest<AdminIconJob | null>("/api/v1/admin/icons/import", token); },
  startIconImport(token: string) { return adminRequest<AdminIconJob>("/api/v1/admin/icons/import", token, { method: "POST", body: "{}" }); },
  iconImport(token: string, jobId: string) { return adminRequest<AdminIconJob>(`/api/v1/admin/icons/import/${encodeURIComponent(jobId)}`, token); },
  replaceStationIcon(token: string, stationId: string, file: File) { return adminRequest<void>(`/api/v1/admin/stations/${encodeURIComponent(stationId)}/icon`, token, { method: "PUT", headers: { "Content-Type": file.type || "application/octet-stream" }, body: file }); },
  removeStationIcon(token: string, stationId: string) { return adminRequest<void>(`/api/v1/admin/stations/${encodeURIComponent(stationId)}/icon`, token, { method: "DELETE", body: JSON.stringify({ confirmation: "DELETE" }) }); },
};

export const base64url = (bytes: ArrayBuffer | Uint8Array) => {
  const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let binary = ""; data.forEach(byte => { binary += String.fromCharCode(byte); });
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};
const decode = (value: string) => { const padded = value.replace(/-/g, "+").replace(/_/g, "/") + "==="; const binary = atob(padded.slice(0, padded.length - (padded.length % 4))); return Uint8Array.from(binary, char => char.charCodeAt(0)); };

/** Converts server JSON options into the browser's WebAuthn request shape. */
export const browserRegistrationOptions = (value: RegistrationOptions): PublicKeyCredentialCreationOptions => ({ ...value.options, challenge: decode(value.options.challenge).buffer, user: { ...value.options.user, id: decode(value.options.user.id).buffer } });
/** Converts a browser registration credential into the API's base64url wire shape. */
export const serializeRegistration = (credential: PublicKeyCredential) => { const response = credential.response as AuthenticatorAttestationResponse; return { id: base64url(credential.rawId), attestationObject: base64url(response.attestationObject), clientDataJSON: base64url(response.clientDataJSON), transports: response.getTransports?.() ?? [] }; };
/** Converts server JSON options into the browser's WebAuthn assertion shape. */
export const browserAuthenticationOptions = (value: AuthenticationOptions): PublicKeyCredentialRequestOptions => ({ ...value.options, challenge: decode(value.options.challenge).buffer, allowCredentials: value.options.allowCredentials?.map(item => ({ ...item, id: decode(item.id).buffer })) });
/** Converts a browser assertion into the API's base64url wire shape. */
export const serializeAuthentication = (credential: PublicKeyCredential) => { const response = credential.response as AuthenticatorAssertionResponse; return { id: base64url(credential.rawId), authenticatorData: base64url(response.authenticatorData), signature: base64url(response.signature), clientDataJSON: base64url(response.clientDataJSON), userHandle: response.userHandle ? base64url(response.userHandle) : null }; };
